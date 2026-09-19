//! A fast concurrent hash map.
//!
//! Concurrent Map
//!
//! This is an implementation of a fast concurrent hash map. Unlike
//! ordinary hash tables, this impl just aborts if it becomes full.
//! So you need to give a correct estimation of the final size before
//! using it. We use this hash map to uniquify pieces of data in
//! mergeable sections.
//!
//! We've implemented this ourselves because the performance of
//! conrurent hash map is critical for our linker.
//!
//! The map is an open-addressing table. Insertion is lock-free: a thread
//! claims an empty bucket with a compare-and-swap on its key pointer,
//! initializes the value, and then publishes the key; a thread that finds
//! a claimed bucket spins until the key appears. Probing stays within a
//! shard of the table, so the entries of a shard can be laid out
//! independently of the others. Keys are byte strings that live for the
//! whole link, such as string literals inside input files, hashed once by
//! the caller.

use rayon::prelude::*;
#[cfg(windows)]
use std::alloc::{Layout, alloc_zeroed, dealloc};
use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicPtr, Ordering};

pub const NUM_SHARDS: usize = 64;

// MIN_NBUCKETS is chosen so that even the smallest map has
// MAX_RETRY buckets per shard; probing is confined to a shard, and
// a probe that visits MAX_RETRY distinct occupied slots aborts.
const MIN_NBUCKETS: usize = 16384;
const MAX_RETRY: usize = 256;

/// The key pointer of a bucket that a thread has claimed but not yet
/// published: -1 marks the slot as claimed until its value has been
/// initialized.
const CLAIMED: *mut u8 = usize::MAX as *mut u8;

// In order to avoid unnecessary cache-line false sharing, we want
// to make this object to be aligned to a reasonably large
// power-of-two address.
#[repr(C, align(32))]
struct Entry<T> {
    key: AtomicPtr<u8>,
    keylen: UnsafeCell<u32>,
    value: UnsafeCell<MaybeUninit<T>>,
}

#[cfg(not(windows))]
fn allocate_entries<T>(bufsize: usize) -> *mut Entry<T> {
    // SAFETY: this creates fresh private anonymous storage.
    let entries = unsafe {
        libc::mmap(
            ptr::null_mut(),
            bufsize,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_ANONYMOUS | libc::MAP_PRIVATE,
            -1,
            0,
        )
    };
    if entries == libc::MAP_FAILED {
        panic!("mmap of {bufsize} bytes failed: {}", std::io::Error::last_os_error());
    }
    entries.cast()
}

#[cfg(windows)]
fn allocate_entries<T>(bufsize: usize) -> *mut Entry<T> {
    let layout = Layout::from_size_align(bufsize, std::mem::align_of::<Entry<T>>())
        .expect("invalid concurrent-map layout");
    // SAFETY: the layout has nonzero size.
    let entries = unsafe { alloc_zeroed(layout).cast::<Entry<T>>() };
    if entries.is_null() {
        panic!("cannot allocate {bufsize} bytes for concurrent map");
    }
    entries
}

#[cfg(not(windows))]
unsafe fn deallocate_entries<T>(entries: *mut Entry<T>, bufsize: usize) {
    // SAFETY: `entries` was mapped by `allocate_entries` with this size.
    let _ = unsafe { libc::munmap(entries.cast(), bufsize) };
}

#[cfg(windows)]
unsafe fn deallocate_entries<T>(entries: *mut Entry<T>, bufsize: usize) {
    let layout = Layout::from_size_align(bufsize, std::mem::align_of::<Entry<T>>())
        .expect("invalid concurrent-map layout");
    // SAFETY: `entries` was allocated by `allocate_entries` with this layout.
    unsafe { dealloc(entries.cast(), layout) };
}

/// A stable reference to an occupied map entry.
pub(crate) struct MapEntryRef<T>(NonNull<Entry<T>>);

impl<T> Clone for MapEntryRef<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for MapEntryRef<T> {}

// SAFETY: map entries are shared between threads only through atomics and
// values whose synchronization is provided by T.
unsafe impl<T: Send + Sync> Send for MapEntryRef<T> {}
unsafe impl<T: Send + Sync> Sync for MapEntryRef<T> {}

/// The index of a bucket, which identifies an entry for the life of
/// the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntryId(u32);

impl EntryId {
    pub fn raw(self) -> u32 {
        self.0
    }

    #[inline]
    pub(crate) fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

/// A map under construction. Freeze it once concurrent insertion is complete
/// to look up and traverse its entries.
pub struct ConcurrentMap<T> {
    entries: *mut Entry<T>,
    nbuckets: usize,
}

// SAFETY: entries are shared between threads only through atomics and
// values that are initialized before they become reachable.
unsafe impl<T: Send + Sync> Send for ConcurrentMap<T> {}
unsafe impl<T: Send + Sync> Sync for ConcurrentMap<T> {}

impl<T> Default for ConcurrentMap<T> {
    /// A map without buckets, to be replaced before use.
    fn default() -> Self {
        Self { entries: ptr::null_mut(), nbuckets: 0 }
    }
}

impl<T> ConcurrentMap<T> {
    /// A map with room for about `nkeys` distinct keys.
    pub fn with_capacity(nkeys: usize) -> Self {
        let nbuckets = nkeys.next_power_of_two().max(MIN_NBUCKETS);
        let bufsize = Self::bufsize(nbuckets);
        // mmap is faster than malloc + memset on Unix; the platform allocator
        // returns equivalently zeroed storage on Windows.
        let entries = allocate_entries(bufsize);
        // SAFETY: the range is the fresh allocation; the advice is only a
        // hint on targets that support it.
        unsafe { crate::util::madvise_hugepage(entries.cast(), bufsize) };
        Self { entries, nbuckets }
    }

    fn bufsize(nbuckets: usize) -> usize {
        std::mem::size_of::<Entry<T>>().checked_mul(nbuckets).expect("table size overflow")
    }

    /// The number of entries, counted.
    fn len(&self) -> usize {
        (0..self.nbuckets).filter(|&idx| self.is_occupied(idx)).count()
    }

    fn entry(&self, idx: usize) -> &Entry<T> {
        debug_assert!(idx < self.nbuckets);
        // SAFETY: idx is within the allocation.
        unsafe { &*self.entries.add(idx) }
    }

    fn is_occupied(&self, idx: usize) -> bool {
        let key = self.entry(idx).key.load(Ordering::Acquire);
        !key.is_null() && key != CLAIMED
    }

    fn value_at(&self, idx: usize) -> &T {
        // SAFETY: callers only ask for values of published entries, which
        // were initialized before publication.
        unsafe { (*self.entry(idx).value.get()).assume_init_ref() }
    }

    /// The probe sequence for a hash: the buckets of one shard, starting
    /// from the hash's home bucket and wrapping around within the shard.
    fn probe(&self, hash: u64) -> impl Iterator<Item = usize> {
        let begin = hash as usize & (self.nbuckets - 1);
        let mask = self.nbuckets / NUM_SHARDS - 1;
        (0..MAX_RETRY).map(move |i| (begin & !mask) | ((begin + i) & mask))
    }

    /// Inserts `key` unless it is present, initializing the value with
    /// `init`. Returns the entry, its value and whether it was inserted.
    pub fn insert_with(
        &self,
        key: &'static [u8],
        hash: u64,
        init: impl FnOnce() -> T,
    ) -> (EntryId, &T, bool) {
        self.insert_entry(
            key.as_ptr(),
            hash,
            |ptr, len| {
                // SAFETY: published keys remain live for the link.
                unsafe { std::slice::from_raw_parts(ptr, len as usize) == key }
            },
            || (key.len() as u32, init()),
        )
    }

    /// Inserts a NUL-terminated key without storing its length at the call
    /// site. `initialize` computes the length and value only if this call
    /// claims a new entry.
    ///
    /// # Safety
    ///
    /// `key` must point to a NUL-terminated byte string that remains live for
    /// the lifetime of the map. Its length must fit in u32.
    pub unsafe fn insert_cstr_with(
        &self,
        key: *const u8,
        hash: u64,
        initialize: impl FnOnce(*const u8) -> (u32, T),
    ) -> (EntryId, &T, bool) {
        // This variant avoids storing a length alongside every caller-side key.
        // Only a newly inserted key needs its length computed by `initialize`.
        self.insert_entry(
            key,
            hash,
            |ptr, _| {
                // SAFETY: guaranteed for `key` by the caller and for `ptr` by
                // the entry publication invariant.
                unsafe { libc::strcmp(key.cast(), ptr.cast()) == 0 }
            },
            || initialize(key),
        )
    }

    fn insert_entry(
        &self,
        key: *const u8,
        hash: u64,
        equals: impl Fn(*const u8, u32) -> bool,
        initialize: impl FnOnce() -> (u32, T),
    ) -> (EntryId, &T, bool) {
        assert!(self.nbuckets > 0, "the map hasn't been sized");
        let mut initialize = Some(initialize);

        for idx in self.probe(hash) {
            let ent = self.entry(idx);

            // Avoid an atomic update when the slot is already occupied.
            let mut ptr = ent.key.load(Ordering::Acquire);
            if !ptr.is_null() && ptr != CLAIMED {
                // SAFETY: a published pointer and length name a live key.
                if equals(ptr, unsafe { *ent.keylen.get() }) {
                    return (EntryId(idx as u32), self.value_at(idx), false);
                }
                continue;
            }

            if ptr.is_null() {
                match ent.key.compare_exchange(
                    ptr::null_mut(),
                    CLAIMED,
                    Ordering::Acquire,
                    Ordering::Acquire,
                ) {
                    Ok(_) => {
                        // SAFETY: the claim gives this thread exclusive
                        // access to the bucket until the key is published.
                        unsafe {
                            let (keylen, value) = initialize.take().expect("initialized once")();
                            (*ent.value.get()).write(value);
                            *ent.keylen.get() = keylen;
                        }
                        ent.key.store(key as *mut u8, Ordering::Release);
                        return (EntryId(idx as u32), self.value_at(idx), true);
                    }
                    Err(current) => ptr = current,
                }
            }

            // Wait for the thread that claimed the slot to publish its key.
            while ptr == CLAIMED {
                std::hint::spin_loop();
                ptr = ent.key.load(Ordering::Acquire);
            }
            // SAFETY: the claiming thread published the pointer and length.
            if equals(ptr, unsafe { *ent.keylen.get() }) {
                return (EntryId(idx as u32), self.value_at(idx), false);
            }
        }
        panic!("the concurrent map is full");
    }

    // Prefetch the bucket where a key with the given hash would be
    // probed first. Useful when a caller knows the hashes of upcoming
    // insertions, as probes into a large table miss the cache almost
    // every time.
    #[inline]
    pub fn prefetch(&self, hash: u64) {
        if self.nbuckets > 0 {
            let idx = hash as usize & (self.nbuckets - 1);
            // SAFETY: the masked index is within the allocated table.
            let ptr = unsafe { self.entries.add(idx) };
            crate::util::prefetch(ptr.cast());
        }
    }

    /// Finishes insertion and returns the map for lookup and traversal.
    pub fn freeze(self) -> FrozenMap<T> {
        FrozenMap(self)
    }
}

impl<T> Drop for ConcurrentMap<T> {
    fn drop(&mut self) {
        if self.entries.is_null() {
            return;
        }
        if std::mem::needs_drop::<T>() {
            for idx in 0..self.nbuckets {
                if self.is_occupied(idx) {
                    // SAFETY: the value of a published entry is initialized
                    // and dropped exactly once, here.
                    unsafe { (*self.entry(idx).value.get()).assume_init_drop() };
                }
            }
        }
        // SAFETY: allocated in with_capacity and not yet released.
        unsafe { deallocate_entries(self.entries, Self::bufsize(self.nbuckets)) };
    }
}

impl<T> std::fmt::Debug for ConcurrentMap<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ConcurrentMap({} entries)", self.len())
    }
}

/// A map after all insertions, whose values can be updated from a
/// unique reference.
#[derive(Debug)]
pub struct FrozenMap<T>(ConcurrentMap<T>);

impl<T> Default for FrozenMap<T> {
    fn default() -> Self {
        Self(ConcurrentMap::default())
    }
}

impl<T> FrozenMap<T> {
    fn entry_ref(&self, id: EntryId) -> MapEntryRef<T> {
        MapEntryRef(NonNull::from(self.0.entry(id.0 as usize)))
    }

    /// The published key of a bucket, if any.
    fn key_at(&self, idx: usize) -> Option<&'static [u8]> {
        let ent = self.0.entry(idx);
        let key = ent.key.load(Ordering::Acquire);
        if key.is_null() || key == CLAIMED {
            return None;
        }
        // SAFETY: a published key is a live 'static slice whose length was
        // written before the key pointer.
        Some(unsafe { std::slice::from_raw_parts(key, *ent.keylen.get() as usize) })
    }

    /// Returns a published entry identified by an ID obtained from this map.
    /// Linker callers only retain IDs returned by insertion or traversal of
    /// the owning map; they never probe a missing key or pass another map's
    /// ID here. That invariant makes the unchecked, initialized-value lookup
    /// below valid without a second occupancy check on this hot path.
    pub fn get(&self, id: EntryId) -> &T {
        self.0.value_at(id.0 as usize)
    }

    #[inline]
    pub(crate) fn prefetch(&self, id: EntryId) {
        crate::util::prefetch(std::ptr::from_ref(self.0.entry(id.0 as usize)).cast());
    }

    pub fn key(&self, id: EntryId) -> &'static [u8] {
        self.key_at(id.0 as usize).expect("an occupied bucket")
    }

    /// Returns the map entries in a deterministic order.
    ///
    /// Linear probing fills the same set of buckets whatever the order
    /// keys were inserted in, but which of two colliding keys got the
    /// earlier bucket depends on it, so each run of adjacent occupied
    /// buckets is sorted by key.
    pub fn sorted_entries(&self, shard: usize) -> Vec<EntryId> {
        if self.0.nbuckets == 0 {
            return Vec::new();
        }
        let shard_size = self.0.nbuckets / NUM_SHARDS;
        let begin = shard * shard_size;
        let mut end = begin + shard_size;
        let occupied = |idx: usize| self.0.is_occupied(idx);

        let size = (begin..end).filter(|&idx| occupied(idx)).count();
        let mut vec: Vec<EntryId> = Vec::with_capacity(size);

        // Since the shard is circular, we need to handle the last entries
        // as if they were next to the first entries.
        while begin < end && occupied(end - 1) {
            end -= 1;
            vec.push(EntryId(end as u32));
        }

        let sort_run = |run: &mut [EntryId]| {
            run.sort_unstable_by(|&a, &b| {
                let (ka, kb) = (self.key(a), self.key(b));
                ka.len().cmp(&kb.len()).then_with(|| ka.cmp(kb))
            });
        };

        // Find entries contiguous in the buckets and sort them.
        let mut last = 0;
        let mut i = begin;
        while i < end {
            while i < end && occupied(i) {
                vec.push(EntryId(i as u32));
                i += 1;
            }
            sort_run(&mut vec[last..]);
            last = vec.len();
            while i < end && !occupied(i) {
                i += 1;
            }
        }
        vec
    }

    /// Returns all map entries as stable references in deterministic order.
    pub(crate) fn sorted_entry_refs_all(&self) -> Vec<MapEntryRef<T>>
    where
        T: Send + Sync,
    {
        (0..NUM_SHARDS)
            .into_par_iter()
            .flat_map_iter(|shard| {
                self.sorted_entries(shard).into_iter().map(|id| self.entry_ref(id))
            })
            .collect()
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
}

impl<T> MapEntryRef<T> {
    fn entry(self, _owner: &FrozenMap<T>) -> &Entry<T> {
        // SAFETY: the reference was made from an occupied bucket in this map,
        // and the owner keeps the map's mmap allocation live.
        unsafe { self.0.as_ref() }
    }

    pub(crate) fn value(self, owner: &FrozenMap<T>) -> &T {
        // SAFETY: sorted entry references name published entries, whose values
        // were initialized before publication.
        unsafe { (*self.entry(owner).value.get()).assume_init_ref() }
    }

    pub(crate) fn value_mut_ptr(self, owner: &FrozenMap<T>) -> *mut T {
        self.entry(owner).value.get().cast()
    }

    pub(crate) fn key(self, owner: &FrozenMap<T>) -> &'static [u8] {
        let ent = self.entry(owner);
        let key = ent.key.load(Ordering::Acquire);
        debug_assert!(!key.is_null() && key != CLAIMED);
        // SAFETY: this is a published key whose length was written before the
        // key pointer, and map keys remain live for the complete link.
        unsafe { std::slice::from_raw_parts(key, *ent.keylen.get() as usize) }
    }

    pub(crate) fn key_len(self, owner: &FrozenMap<T>) -> usize {
        // SAFETY: the entry is published, so its key length is initialized and
        // immutable.
        unsafe { *self.entry(owner).keylen.get() as usize }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_lookup() {
        let map: ConcurrentMap<u32> = ConcurrentMap::with_capacity(100);
        let keys: Vec<&'static [u8]> = (0..1000)
            .map(|i| &*Box::leak(format!("key{i}").into_bytes().into_boxed_slice()))
            .collect();
        let hash = |k: &[u8]| xxhash_rust::xxh3::xxh3_64(k);
        let mut ids = Vec::with_capacity(keys.len());
        for (i, &k) in keys.iter().enumerate() {
            let (id, v, inserted) = map.insert_with(k, hash(k), || i as u32);
            assert!(inserted);
            assert_eq!(*v, i as u32);
            ids.push(id);
        }
        for (i, &k) in keys.iter().enumerate() {
            let (id, v, inserted) = map.insert_with(k, hash(k), || 99999);
            assert!(!inserted);
            assert_eq!(id, ids[i]);
            assert_eq!(*v, i as u32);
        }
        let map = map.freeze();
        for (i, &id) in ids.iter().enumerate() {
            assert_eq!(map.key(id), keys[i]);
            assert_eq!(*map.get(id), i as u32);
        }
        assert_eq!(map.len(), 1000);
        let sorted: usize = (0..NUM_SHARDS).map(|s| map.sorted_entries(s).len()).sum();
        assert_eq!(sorted, 1000);
    }
}
