//! Symbols and the global symbol table.

use crate::macho::input_files::FileId;

/// A symbol index, u32 as in mold-rust: every per-symbol and
/// per-nlist vector of ids is half the size of a usize one.
pub type SymbolId = u32;

/// A symbol's owning file in one u32 - none, an object index, or a
/// dylib index with the DYLIB bit - mold-rust's SymbolFile. The
/// dynamic-lookup import (FileId::Dylib(u32::MAX): a dylib index
/// naming no dylib) packs as the one value the bit and NONE leave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SymbolFile(u32);

impl SymbolFile {
    const DYLIB: u32 = 1 << 31;
    const NONE: u32 = u32::MAX;
    const DYNAMIC_LOOKUP: u32 = u32::MAX - 1;

    #[inline]
    fn none() -> SymbolFile {
        SymbolFile(Self::NONE)
    }

    #[inline]
    fn some(file: FileId) -> SymbolFile {
        SymbolFile(match file {
            FileId::Obj(i) => {
                debug_assert!(i < Self::DYLIB);
                i
            }
            FileId::Dylib(u32::MAX) => Self::DYNAMIC_LOOKUP,
            FileId::Dylib(i) => {
                debug_assert!(i < Self::DYLIB - 1);
                i | Self::DYLIB
            }
        })
    }

    #[inline]
    fn get(self) -> Option<FileId> {
        match self.0 {
            Self::NONE => None,
            Self::DYNAMIC_LOOKUP => Some(FileId::Dylib(u32::MAX)),
            raw if raw & Self::DYLIB != 0 => Some(FileId::Dylib(raw & !Self::DYLIB)),
            raw => Some(FileId::Obj(raw)),
        }
    }
}

#[derive(Debug)]
pub struct Symbol {
    /// The name, as a pointer and a u32 length rather than a 16-byte
    /// &str - mold-rust's name_ptr/name_len. Read through name().
    name_ptr: usize,
    name_len: u32,
    /// The owning file - the object or dylib that defines the symbol -
    /// or none while it is undefined. Read through file().
    file: SymbolFile,
    /// The defining subsection for an `N_SECT` symbol, or NONE. Read
    /// through isec(); index ctx.isecs with `as usize`.
    isec: u32,
    /// Offset from the start of `isec`, or the absolute value for `N_ABS`
    /// symbols.
    pub value: u64,
    /// Index into the sparse SymAux table (ctx.sym_aux), or NONE: only
    /// symbols with a stub/GOT/TLV/objc slot have an entry - mold-rust's
    /// aux_idx - instead of 16 bytes per symbol whether needed or not.
    pub aux_idx: u32,
    /// The boolean attributes, packed into one atomic word as mold-rust
    /// keeps its Symbol flags: the eight is_* bits, read with plain
    /// loads and written through &mut without an atomic operation, plus
    /// the MARK bit that parallel passes set with a compare-and-swap
    /// (thunk creation dedups its entries that way, in the scan itself).
    flags: std::sync::atomic::AtomicU16,
    pub common_p2align: u8,
}

// Symbol is loaded in every resolution and layout scan, so its width
// is kept minimal. mold-rust's is 48 with more fields (a version index,
// a symbol index); ours packs the same way and lands at 40.
const _: () = assert!(std::mem::size_of::<Symbol>() == 40);

/// "No index" for `isec` and `aux_idx`.
pub const NONE: u32 = u32::MAX;

impl Symbol {
    #[inline]
    pub fn name(&self) -> &'static str {
        // SAFETY: name_ptr/name_len are exactly the bytes of the
        // &'static str the symbol was created with.
        unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                self.name_ptr as *const u8,
                self.name_len as usize,
            ))
        }
    }

    /// The file that owns the symbol: the object or dylib whose
    /// definition won resolution, or None while it is undefined.
    #[inline]
    pub fn file(&self) -> Option<FileId> {
        self.file.get()
    }

    #[inline]
    pub fn set_file(&mut self, file: FileId) {
        self.file = SymbolFile::some(file);
    }

    #[inline]
    pub fn clear_file(&mut self) {
        self.file = SymbolFile::none();
    }

    #[inline]
    pub fn input_section(&self) -> Option<u32> {
        (self.isec != NONE).then_some(self.isec)
    }

    #[inline]
    pub fn set_input_section(&mut self, isec: Option<u32>) {
        self.isec = isec.unwrap_or(NONE);
    }
}

const F_EXTERN: u16 = 1 << 0;
const F_WEAK_DEF: u16 = 1 << 1;
const F_IMPORTED: u16 = 1 << 2;
const F_USED: u16 = 1 << 3;
const F_PRIVATE_EXTERN: u16 = 1 << 4;
const F_WEAK_REF: u16 = 1 << 5;
const F_NO_DEAD_STRIP: u16 = 1 << 6;
const F_COMMON: u16 = 1 << 7;
/// Set by mark(), a transient per-pass flag (mold-rust's IS_MARKED).
const F_MARK: u16 = 1 << 8;
/// Referenced non-weakly by some object: with ld64's default
/// -weak_reference_mismatches non-weak, that makes the import strong
/// whatever other references say.
const F_STRONG_REF: u16 = 1 << 9;

macro_rules! sym_flag {
    ($get:ident, $set:ident, $bit:expr, $doc:expr) => {
        #[doc = $doc]
        #[inline]
        pub fn $get(&self) -> bool {
            self.flags.load(std::sync::atomic::Ordering::Relaxed) & $bit != 0
        }
        #[inline]
        pub fn $set(&mut self, v: bool) {
            let f = self.flags.get_mut();
            if v {
                *f |= $bit;
            } else {
                *f &= !$bit;
            }
        }
    };
}

impl Symbol {
    sym_flag!(is_extern, set_is_extern, F_EXTERN, "An external (global) symbol.");
    sym_flag!(is_weak_def, set_is_weak_def, F_WEAK_DEF, "A weak definition.");
    sym_flag!(
        is_imported,
        set_is_imported,
        F_IMPORTED,
        "The definition is in a dylib, so references need dynamic binding."
    );
    sym_flag!(
        is_used,
        set_is_used,
        F_USED,
        "Some relocation refers to this symbol, so an unresolved symbol is an error."
    );
    sym_flag!(
        is_private_extern,
        set_is_private_extern,
        F_PRIVATE_EXTERN,
        "A private external symbol (visibility hidden): resolves globally at link time but is neither exported nor kept as an external symbol."
    );
    sym_flag!(
        is_strong_ref,
        set_is_strong_ref,
        F_STRONG_REF,
        "Referenced non-weakly by some object."
    );
    sym_flag!(
        is_weak_ref,
        set_is_weak_ref,
        F_WEAK_REF,
        "References may go unresolved at load time (a weak import)."
    );
    sym_flag!(
        no_dead_strip,
        set_no_dead_strip,
        F_NO_DEAD_STRIP,
        "The symbol must survive dead-stripping."
    );
    sym_flag!(
        is_common,
        set_is_common,
        F_COMMON,
        "A tentative definition (common symbol) not yet converted; `value` holds its size."
    );

    /// Atomically sets the transient mark; true if it was clear (the
    /// caller won the race to claim this symbol).
    #[inline]
    pub fn mark(&self) -> bool {
        use std::sync::atomic::Ordering::Relaxed;
        self.flags.fetch_or(F_MARK, Relaxed) & F_MARK == 0
    }
    #[inline]
    pub fn unmark(&self) {
        self.flags.fetch_and(!F_MARK, std::sync::atomic::Ordering::Relaxed);
    }
    #[inline]
    pub fn is_marked(&self) -> bool {
        self.flags.load(std::sync::atomic::Ordering::Relaxed) & F_MARK != 0
    }
}

impl Clone for Symbol {
    fn clone(&self) -> Symbol {
        Symbol {
            name_ptr: self.name_ptr,
            name_len: self.name_len,
            file: self.file,
            isec: self.isec,
            value: self.value,
            aux_idx: self.aux_idx,
            flags: std::sync::atomic::AtomicU16::new(
                self.flags.load(std::sync::atomic::Ordering::Relaxed),
            ),
            common_p2align: self.common_p2align,
        }
    }
}

impl std::fmt::Display for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&crate::macho::util::demangle(self.name()))
    }
}

/// Sentinel for a synthetic-slot index a symbol does not have.
pub const NO_IDX: u32 = u32::MAX;

/// A symbol's synthetic-slot indices (__stubs, __got, __thread_ptrs,
/// __objc_stubs), each `NO_IDX` when absent. Only the few symbols that
/// take a slot ever have one, so these live in a side table indexed by
/// SymbolId - mold-rust's SymbolAux - keeping Symbol itself small, as
/// it is loaded in every symbol scan.
#[derive(Clone, Debug)]
pub struct SymAux {
    pub stub_idx: u32,
    pub got_idx: u32,
    pub tlv_idx: u32,
    pub objc_stub_idx: u32,
    /// The addresses of this symbol's range-extension thunk entries,
    /// sorted, so that applying an out-of-range branch can find the one
    /// within reach - mold-rust's SymbolAux::thunk_addrs.
    pub thunk_addrs: Vec<u64>,
}

impl SymAux {
    pub const NONE: SymAux = SymAux {
        stub_idx: NO_IDX,
        got_idx: NO_IDX,
        tlv_idx: NO_IDX,
        objc_stub_idx: NO_IDX,
        thunk_addrs: Vec::new(),
    };
}

/// The shared "no slots" entry that sym_aux() returns for symbols
/// without an aux entry.
pub static NONE_AUX: SymAux = SymAux::NONE;

impl Default for SymAux {
    fn default() -> SymAux {
        SymAux::NONE
    }
}

impl Symbol {
    pub(crate) fn new(name: &'static str) -> Symbol {
        Symbol {
            name_ptr: name.as_ptr() as usize,
            name_len: u32::try_from(name.len()).expect("symbol name is larger than 4 GiB"),
            file: SymbolFile::none(),
            isec: NONE,
            value: 0,
            aux_idx: NONE,
            flags: std::sync::atomic::AtomicU16::new(0),
            common_p2align: 0,
        }
    }

    pub fn is_defined(&self) -> bool {
        self.file.get().is_some()
    }
}

/// All symbols in this link. Global symbols are interned by name so that
/// all references to one name share a slot; local symbols get anonymous
/// slots of their own.
///
/// The name map is sharded by a hash of the name, following mold's
/// symbol table: keys carry their xxh3 hash, computed in parallel
/// while files are staged, the maps hash by passing that value
/// through, and gather resolves a whole link's worth of names
/// with the shards processed in parallel.
#[derive(Debug)]
pub struct SymbolTable {
    shards: Vec<ShardMap>,
    pub syms: Vec<Symbol>,
}

pub const NUM_SHARDS: usize = 64;

/// The hash of a symbol table key, which the sharded table and its
/// callers share. Computed once per name, at staging time when
/// possible.
pub fn hash_key(key: &str) -> u64 {
    xxhash_rust::xxh3::xxh3_64(key.as_bytes())
}

fn shard_of(hash: u64) -> usize {
    (hash % NUM_SHARDS as u64) as usize
}

/// A map key that hashes by its precomputed hash and compares by the
/// name, as in mold-rust.
#[derive(Clone, Copy, Debug)]
struct Key {
    hash: u64,
    key: &'static str,
}

impl PartialEq for Key {
    fn eq(&self, other: &Key) -> bool {
        self.hash == other.hash && self.key == other.key
    }
}
impl Eq for Key {}

impl std::hash::Hash for Key {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.hash);
    }
}

#[derive(Default)]
struct PassThroughHasher(u64);

impl std::hash::Hasher for PassThroughHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, _: &[u8]) {
        unreachable!("keys hash by their precomputed hash")
    }
    fn write_u64(&mut self, hash: u64) {
        self.0 = hash;
    }
}

type ShardMap = hashbrown::HashMap<Key, SymbolId, std::hash::BuildHasherDefault<PassThroughHasher>>;

/// A hash map over borrowed names with caller-supplied xxh3 hashes -
/// the symbol table's key discipline, reusable wherever names are
/// deduplicated (the output string table).
#[derive(Default)]
pub struct PrehashedMap<V>(
    hashbrown::HashMap<Key, V, std::hash::BuildHasherDefault<PassThroughHasher>>,
);

impl<V> PrehashedMap<V> {
    pub fn get(&self, name: &str, hash: u64) -> Option<&V> {
        // SAFETY: the key is only compared during this call.
        let probe = Key { hash, key: unsafe { std::mem::transmute::<&str, &'static str>(name) } };
        self.0.get(&probe)
    }

    pub fn insert(&mut self, name: &'static str, hash: u64, value: V) {
        self.0.insert(Key { hash, key: name }, value);
    }
}

impl Default for SymbolTable {
    fn default() -> SymbolTable {
        SymbolTable {
            shards: (0..NUM_SHARDS).map(|_| ShardMap::default()).collect(),
            syms: Vec::new(),
        }
    }
}

impl SymbolTable {
    /// Returns the symbol for a global name, creating it if needed.
    pub fn intern(&mut self, name: &'static str) -> SymbolId {
        let hash = hash_key(name);
        *self.shards[shard_of(hash)].entry(Key { hash, key: name }).or_insert_with(|| {
            self.syms.push(Symbol::new(name));
            (self.syms.len() - 1) as u32
        })
    }

    /// Returns the symbol for a global name if it exists.
    pub fn get(&self, name: &str) -> Option<SymbolId> {
        let hash = hash_key(name);
        // SAFETY-free trick from mold: lookups build a key borrowing
        // the probe name; only inserts require 'static.
        let probe = Key {
            hash,
            // The key is only compared during this call.
            key: unsafe { std::mem::transmute::<&str, &'static str>(name) },
        };
        self.shards[shard_of(hash)].get(&probe).copied()
    }

    /// Creates an anonymous slot for a file-local symbol.
    pub fn add_local(&mut self, name: &'static str) -> SymbolId {
        self.syms.push(Symbol::new(name));
        (self.syms.len() - 1) as u32
    }

    /// Interns every (name, precomputed-hash) pair at once, returning
    /// ids aligned with the batch. Names are binned by shard without
    /// touching their bytes, the shards resolve their bins in
    /// parallel, new symbols take contiguous id ranges per shard, and
    /// one serial scatter hands the ids back. Ids depend only on
    /// input order and the hash, so links stay deterministic.
    pub fn gather(&mut self, batch: &[(&'static str, u64)]) -> Vec<SymbolId> {
        use rayon::prelude::*;

        let mut bins: Vec<Vec<u32>> = vec![Vec::new(); NUM_SHARDS];
        for (i, &(_, hash)) in batch.iter().enumerate() {
            bins[shard_of(hash)].push(i as u32);
        }

        enum Resolved {
            Old(SymbolId),
            New(u32),
        }
        let results: Vec<(Vec<(u32, Resolved)>, Vec<(&'static str, u64)>)> = self
            .shards
            .par_iter_mut()
            .zip(bins)
            .map(|(shard, bin)| {
                // Names first seen in this batch, with their hash, so
                // the insert pass below never re-hashes them; final ids
                // are assigned once the shards' ranges are known.
                let mut news: Vec<(&'static str, u64)> = Vec::new();
                let mut newmap: ShardMap = ShardMap::default();
                let mut out = Vec::with_capacity(bin.len());
                for i in bin {
                    let (name, hash) = batch[i as usize];
                    let key = Key { hash, key: name };
                    if let Some(&id) = shard.get(&key) {
                        out.push((i, Resolved::Old(id)));
                        continue;
                    }
                    let idx = *newmap.entry(key).or_insert_with(|| {
                        news.push((name, hash));
                        (news.len() - 1) as u32
                    });
                    out.push((i, Resolved::New(idx as u32)));
                }
                (out, news)
            })
            .collect();

        let mut bases = Vec::with_capacity(NUM_SHARDS);
        let mut base = self.syms.len();
        for (_, news) in &results {
            bases.push(base);
            base += news.len();
        }

        // Initialize the new symbols into their prefix-summed ranges in
        // parallel - the same disjoint-range contract integrate uses for
        // local symbols - so a debug link's millions of new globals are
        // constructed on all cores, not pushed one at a time.
        let old_len = self.syms.len();
        let total_new = base - old_len;
        self.syms.reserve(total_new);
        {
            struct SlotPtr(*mut Symbol);
            unsafe impl Sync for SlotPtr {}
            let ptr = SlotPtr(self.syms.as_mut_ptr());
            let ptr = &ptr;
            results.par_iter().zip(&bases).for_each(|((_, news), &b)| {
                for (k, &(name, _)) in news.iter().enumerate() {
                    // SAFETY: [b, b+news.len()) ranges are disjoint
                    // across shards and lie within the reserved space.
                    unsafe { ptr.0.add(b + k).write(Symbol::new(name)) };
                }
            });
            // SAFETY: every slot in old_len..old_len+total_new was
            // written exactly once above.
            unsafe { self.syms.set_len(old_len + total_new) };
        }

        // Insert the new names with their final ids, reusing the hash
        // computed during staging (no re-hash here).
        self.shards.par_iter_mut().zip(&results).zip(&bases).for_each(
            |((shard, (_, news)), &b)| {
                for (k, &(name, hash)) in news.iter().enumerate() {
                    shard.insert(Key { hash, key: name }, (b + k) as u32);
                }
            },
        );

        // Scatter each batch entry's resolved id in parallel; every
        // batch index appears in exactly one shard's output list, so
        // the writes are disjoint.
        let mut ids = vec![0u32; batch.len()];
        {
            struct IdPtr(*mut u32);
            unsafe impl Sync for IdPtr {}
            let ptr = IdPtr(ids.as_mut_ptr());
            let ptr = &ptr;
            results.par_iter().zip(&bases).for_each(|((out, _), &b)| {
                for &(i, ref r) in out {
                    let v = match r {
                        Resolved::Old(id) => *id,
                        Resolved::New(k) => (b + *k as usize) as u32,
                    };
                    // SAFETY: each batch index i is produced by exactly
                    // one shard, so these writes never overlap.
                    unsafe { *ptr.0.add(i as usize) = v };
                }
            });
        }
        ids
    }
}

impl std::ops::Index<SymbolId> for SymbolTable {
    type Output = Symbol;
    #[inline]
    fn index(&self, id: SymbolId) -> &Symbol {
        &self.syms[id as usize]
    }
}

impl std::ops::IndexMut<SymbolId> for SymbolTable {
    #[inline]
    fn index_mut(&mut self, id: SymbolId) -> &mut Symbol {
        &mut self.syms[id as usize]
    }
}

impl std::ops::Index<usize> for SymbolTable {
    type Output = Symbol;
    #[inline]
    fn index(&self, id: usize) -> &Symbol {
        &self.syms[id]
    }
}

impl std::ops::IndexMut<usize> for SymbolTable {
    #[inline]
    fn index_mut(&mut self, id: usize) -> &mut Symbol {
        &mut self.syms[id]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// C++ template instantiations produce symbol names tens of
    /// thousands of bytes long (ClickHouse's exceed 65535); the cached
    /// length must hold them whole.
    #[test]
    fn accepts_long_symbol_name() {
        let name: &'static str = String::leak("x".repeat(65536));
        let symbol = Symbol::new(name);
        assert_eq!(symbol.name().len(), 65536);
        assert_eq!(symbol.name(), name);
    }
}
