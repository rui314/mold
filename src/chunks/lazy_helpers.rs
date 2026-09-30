//! __TEXT,__lazy_helpers: the code through which an image reaches the
//! symbols of a dylib dyld loads lazily (-lazy-l and the like, macOS 27
//! on). Each helper checks the dylib's flag word; once dyld has loaded
//! the dylib and bound its __lazy_load_got slots, it goes on through
//! the symbol's slot, and before that it first has __dyld_lazy_load
//! load the dylib.

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::macho::*;
use crate::symbol::SymbolId;
use crate::target::Target;

/// The reference a helper stands in for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LazyUse {
    /// Calls, which branch to `_foo$lazyLoadStub` instead: it jumps
    /// through the symbol's slot.
    Call,
    /// A load of the symbol's address from the GOT into register `reg`:
    /// arm64's adrp of the GOT slot's page becomes a call of
    /// `_foo$lazyGOT$loadHelper_<reg>`, which returns with the page of
    /// the symbol's __lazy_load_got slot in the register, for the ldr
    /// under the adrp to load from the slot; x86-64's movq becomes a
    /// call of one that loads the slot. arm64 code that may not have
    /// saved its link register branches instead to a helper of its
    /// own, which branches back past the adrp: `site` names that adrp
    /// (subsection, offset).
    Load { reg: u8, site: Option<(u32, u32)> },
    /// x86-64's test of a weak import, cmpq $0 of its GOT slot: it
    /// becomes a call of `_foo$lazyGOT$cmpHelper`, which compares the
    /// slot instead, leaving the flags for the code after the call.
    Cmp,
}

/// What a helper's code refers to, for LC_SEGMENT_SPLIT_INFO: the flag
/// word, the helper's slot, the mach header, __dyld_lazy_load's stub,
/// or the code to return to past the site.
#[derive(Clone, Copy, Debug)]
pub enum LazyTarget {
    Flag,
    Slot,
    Header,
    LazyLoad,
    Site,
}

#[derive(Debug)]
pub struct LazyHelper {
    pub sym: SymbolId,
    pub kind: LazyUse,
    /// The helper's local symbol, as ld-prime names it.
    pub name: &'static str,
    /// The subsection of the flag word of the symbol's dylib, and the
    /// symbol's __lazy_load_got slot the helper goes through.
    pub flag: u32,
    pub slot: u32,
    /// Where the helper lies in the section.
    pub offset: u32,
}

/// __TEXT,__lazy_helpers: the helpers, in name order as ld-prime lays
/// them out.
#[derive(Debug)]
pub struct LazyHelpersSection {
    pub hdr: ChunkHeader,
    pub helpers: Vec<LazyHelper>,
    /// The helper each rewritten GOT load goes to, by the subsection
    /// and offset of the instruction it replaces.
    pub sites: hashbrown::HashMap<(u32, u32), u32>,
    /// __dyld_lazy_load, which the helpers call through its stub.
    pub dyld_lazy_load: Option<SymbolId>,
    /// The empty atom ld-prime keeps __dyld_lazy_load alive from, a
    /// subsection (see passes::add_keep_alive_atom), or u32::MAX.
    pub keep_alive: u32,
}

impl LazyHelpersSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::new("__TEXT", "__lazy_helpers");
        hdr.flags = S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
        Self {
            hdr,
            helpers: Vec::new(),
            sites: Default::default(),
            dyld_lazy_load: None,
            keep_alive: u32::MAX,
        }
    }

    /// The helper a rewritten GOT load at `offset` of subsection `isec`
    /// goes to.
    pub fn site_helper(&self, isec: usize, offset: u32) -> usize {
        self.sites[&(isec as u32, offset)] as usize
    }
}

impl Default for LazyHelpersSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    E::write_lazy_helpers(ctx, ctx.lazy_helpers.hdr.addr, buf);
}
