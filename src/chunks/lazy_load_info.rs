//! The LC_LAZY_LOAD_DYLIB_INFO records in __LINKEDIT: for each dylib
//! dyld loads lazily, what __dyld_lazy_load needs to load it and bind
//! the image's slots for its symbols.

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::macho::DYLD_CHAINED_PTR_64_OFFSET;
use crate::symbol::SymbolId;
use crate::target::Target;

/// A dylib dyld loads lazily, one of whose symbols the image uses.
#[derive(Debug)]
pub struct LazyDylib {
    /// The dylib, an index into ctx.dylibs.
    pub dylib: u32,
    /// The subsection of its flag word in __DATA,__data
    /// (_lazyLoadFlag$<leaf name>), which dyld sets once it has loaded
    /// the dylib.
    pub flag: u32,
    /// The symbol of each of its __lazy_load_got slots, which follow
    /// one another from `got_start`, by name.
    pub syms: Vec<SymbolId>,
    pub got_start: u32,
    /// Where its record lies in this chunk, and its size.
    pub offset: u32,
    pub size: u32,
}

/// Each lazy dylib's record, which a load command of its own points at
/// (dyld reads each record on its own). A record is 32-bit words - the offset of the dylib's install name
/// (from the record's start), the image offset of its flag word, the
/// pointer format of its __lazy_load_got chain in the high half with
/// flags in the low half (1: a weak dylib, which may be missing), the
/// image offset of its chain's first slot, the number of symbols, and
/// the offset of an array holding each symbol's name's offset - then
/// that array, the install name and the symbol names, padded to 8
/// bytes.
#[derive(Debug)]
pub struct LazyLoadInfoSection {
    pub hdr: ChunkHeader,
    /// The dylibs, in load order.
    pub dylibs: Vec<LazyDylib>,
}

impl LazyLoadInfoSection {
    pub fn new() -> Self {
        Self { hdr: ChunkHeader::linkedit(), dylibs: Vec::new() }
    }
}

impl Default for LazyLoadInfoSection {
    fn default() -> Self {
        Self::new()
    }
}

/// The size of a record naming `install_name` and `syms`.
pub fn record_size<E: Target>(ctx: &Context<E>, install_name: &[u8], syms: &[SymbolId]) -> u32 {
    let names: usize = syms.iter().map(|&id| ctx.symbols[id].name().len() + 1).sum();
    (24 + syms.len() * 4 + install_name.len() + 1 + names).next_multiple_of(8) as u32
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let base = ctx.mach_header.hdr.addr;
    for d in &ctx.lazy_load_info.dylibs {
        let dylib = &ctx.dylibs[d.dylib as usize];
        let rec = &mut buf[d.offset as usize..(d.offset + d.size) as usize];
        let nsyms = d.syms.len();
        let mut strs = 24 + nsyms * 4;
        let format = (DYLD_CHAINED_PTR_64_OFFSET as u32) << 16 | dylib.is_weak as u32;
        let got = ctx.lazy_load_got.slot_addr(d.got_start);
        let header = [
            strs as u32,
            (ctx.isec_addr(d.flag as usize) - base) as u32,
            format,
            (got - base) as u32,
            nsyms as u32,
            24,
        ];
        for (i, word) in header.iter().enumerate() {
            rec[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        let mut put = |rec: &mut [u8], s: &[u8]| {
            let at = strs;
            rec[at..at + s.len()].copy_from_slice(s);
            strs += s.len() + 1;
            at as u32
        };
        put(rec, &dylib.install_name);
        for (i, &id) in d.syms.iter().enumerate() {
            let at = put(rec, ctx.symbols[id].name());
            rec[24 + i * 4..28 + i * 4].copy_from_slice(&at.to_le_bytes());
        }
    }
}
