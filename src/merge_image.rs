use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use zerocopy::byteorder::little_endian::{U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

const LIMIT: usize = 8192;
static DISABLED: AtomicBool = AtomicBool::new(false);
static FRAGMENTS: Mutex<BTreeMap<[u8; 32], (Fragment, u32)>> = Mutex::new(BTreeMap::new());

#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub(crate) struct Fragment {
    pub key: [u8; 32],
    pub offset: U64,
    pub length: U32,
    pub reserved: U32,
}
#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub(crate) struct Contribution {
    pub key: [u8; 32],
    pub section: U32,
    pub reserved: U32,
}
pub(crate) fn eligible(name: &[u8], flags: u64, entsize: u64, align: u64) -> bool {
    name.starts_with(b".debug")
        && flags == u64::from(crate::elf::SHF_MERGE | crate::elf::SHF_STRINGS)
        && entsize == 1
        && align <= 1
}
pub(crate) fn key(name: &[u8], bytes: &[u8]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(&(name.len() as u64).to_le_bytes());
    h.update(name);
    h.update(bytes);
    *h.finalize().as_bytes()
}
pub(crate) fn inserted(name: &[u8], flags: u64, entsize: u64, align: u8, bytes: &[u8]) {
    if !crate::incremental::semantic_tracking()
        || DISABLED.load(Ordering::Relaxed)
        || !eligible(name, flags, entsize, 1u64.checked_shl(u32::from(align)).unwrap_or(u64::MAX))
    {
        return;
    }
    let mut fragments = FRAGMENTS.lock().unwrap();
    if fragments.len() >= LIMIT {
        fragments.clear();
        DISABLED.store(true, Ordering::Relaxed);
        return;
    }
    let key = key(name, bytes);
    let record = fragments.entry(key).or_insert((
        Fragment {
            key,
            offset: u64::MAX.into(),
            length: (bytes.len() as u32).into(),
            reserved: 0.into(),
        },
        0,
    ));
    if let Some(count) = record.1.checked_add(1) {
        record.1 = count;
    } else {
        fragments.clear();
        DISABLED.store(true, Ordering::Relaxed);
    }
}
pub(crate) fn placed(name: &[u8], bytes: &[u8], offset: u64) {
    if DISABLED.load(Ordering::Relaxed) {
        return;
    }
    if let Some((record, _)) = FRAGMENTS.lock().unwrap().get_mut(&key(name, bytes)) {
        record.offset = offset.into();
    }
}
pub(crate) fn take() -> Option<(Vec<Fragment>, Vec<U32>)> {
    let fragments = std::mem::take(&mut *FRAGMENTS.lock().unwrap());
    if DISABLED.swap(false, Ordering::Relaxed)
        || fragments.values().any(|(f, n)| f.offset.get() == u64::MAX || *n == 0)
    {
        return None;
    }
    Some(fragments.into_values().map(|(f, n)| (f, U32::new(n))).unzip())
}
const _: () = {
    assert!(size_of::<Fragment>() == 48);
    assert!(size_of::<Contribution>() == 40);
};
