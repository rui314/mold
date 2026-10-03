//! __TEXT,__objc_stubs: the linker-synthesized _objc_msgSend$<selector>
//! stubs, with the selector strings and references they load.

use crate::chunks::{ChunkHeader, OutputSectionId};
use crate::context::Context;
use crate::macho::*;
use crate::symbol::SymbolId;
use crate::target::Target;

/// __TEXT,__objc_stubs: linker-synthesized _objc_msgSend$<selector>
/// stubs, with the selector strings and references they load (laid
/// out as the tails of the __objc_methname and __objc_selrefs output
/// sections, see Tail).
#[derive(Debug)]
pub struct ObjcStubsSection {
    pub hdr: ChunkHeader,
    /// _objc_msgSend$<selector> symbols, in entry order, with their
    /// selector names. Stub `i` loads slot `i` of the __objc_selrefs
    /// tail.
    pub symbols: Vec<(SymbolId, &'static [u8])>,
    /// Selector references synthesized for method lists whose selector
    /// no input references: the __objc_methname subsection each points
    /// at. They follow the stubs' slots in the __objc_selrefs tail.
    pub extra_selrefs: Vec<u32>,
    /// Contents of the synthesized __objc_methname tail, and each
    /// selector's offset in it.
    pub methname_data: Vec<u8>,
    pub methname_offs: Vec<u64>,
    /// The output sections carrying the synthesized selector strings
    /// and selector references as their tail.
    pub methname: Option<OutputSectionId>,
    pub selrefs: Option<OutputSectionId>,
    /// The _objc_msgSend symbol, once objc stubs exist.
    pub msgsend_sym: Option<SymbolId>,
}

impl ObjcStubsSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::new(b"__TEXT", b"__objc_stubs");
        hdr.flags = S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
        hdr.p2align = 5;
        Self {
            hdr,
            symbols: Vec::new(),
            extra_selrefs: Vec::new(),
            methname_data: Vec::new(),
            methname_offs: Vec::new(),
            methname: None,
            selrefs: None,
            msgsend_sym: None,
        }
    }
}

impl Default for ObjcStubsSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    E::write_objc_stubs(ctx, ctx.objc_stubs.hdr.addr, buf);
}
