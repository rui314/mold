//! The Objective-C sections the linker synthesizes: the
//! _objc_msgSend$<selector> stubs, the method lists rewritten in
//! relative form, and the merged __objc_imageinfo record.

use crate::macho::arch::Arch;
use crate::macho::context::Context;
use crate::macho::format::*;
use crate::macho::output_chunks::{ChunkHeader, OutputSectionId};
use crate::macho::passes::{ObjcMethList, ObjcRef};
use crate::macho::symbol::SymbolId;

/// __TEXT,__objc_stubs: linker-synthesized _objc_msgSend$<selector>
/// stubs, with the selector strings and references they load (laid
/// out as the tails of the __objc_methname and __objc_selrefs output
/// sections, see Tail).
#[derive(Debug)]
pub struct ObjcStubsSection {
    pub hdr: ChunkHeader,
    /// _objc_msgSend$<selector> symbols, in entry order, with their
    /// selector names.
    pub symbols: Vec<(SymbolId, String)>,
    /// Per stub: an input __objc_selrefs slot for its selector that the
    /// stub loads instead of a synthesized one (u32::MAX for none), and
    /// its slot's index in the tail when it has one.
    pub selref: Vec<u32>,
    pub tail: Vec<u32>,
    /// The number of stub slots in the __objc_selrefs tail; the extra
    /// selector references follow them.
    pub tail_slots: usize,
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

impl Default for ObjcStubsSection {
    fn default() -> Self {
        Self::new()
    }
}

impl ObjcStubsSection {
    pub fn new() -> ObjcStubsSection {
        let mut hdr = ChunkHeader::new("__TEXT", "__objc_stubs");
        hdr.flags = S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
        hdr.p2align = 5;
        ObjcStubsSection {
            hdr,
            symbols: Vec::new(),
            selref: Vec::new(),
            tail: Vec::new(),
            tail_slots: 0,
            extra_selrefs: Vec::new(),
            methname_data: Vec::new(),
            methname_offs: Vec::new(),
            methname: None,
            selrefs: None,
            msgsend_sym: None,
        }
    }
}

pub mod objc_stubs {
    use super::*;

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        E::write_objc_stubs(ctx, ctx.objc_stubs.hdr.addr, buf);
    }
}

/// __TEXT,__objc_methlist: the Objective-C method lists rewritten in
/// the relative (12-byte entry) form, which needs no fixups.
#[derive(Debug)]
pub struct ObjcMethlistSection {
    pub hdr: ChunkHeader,
    /// The rewritten lists, each with its synthetic subsection here.
    pub lists: Vec<ObjcMethList>,
}

impl Default for ObjcMethlistSection {
    fn default() -> Self {
        Self::new()
    }
}

impl ObjcMethlistSection {
    pub fn new() -> ObjcMethlistSection {
        let mut hdr = ChunkHeader::new("__TEXT", "__objc_methlist");
        hdr.p2align = 3;
        ObjcMethlistSection { hdr, lists: Vec::new() }
    }
}

pub mod objc_methlist {
    use super::*;

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        let addr_of = |r: ObjcRef| -> u64 {
            match r {
                ObjcRef::Isec(isec, off) => ctx.isec_addr(isec as usize) + off,
                ObjcRef::Sym(id, addend) => (ctx.sym_addr(id) as i64 + addend) as u64,
                ObjcRef::TailSelref(n) => ctx.objc_selref_addr(n),
                ObjcRef::Null => 0,
            }
        };
        for list in &ctx.objc_methlist.lists {
            let isec = &ctx.isecs[list.isec as usize];
            let base = isec.offset as usize;
            let addr = ctx.objc_methlist.hdr.addr + base as u64;
            let count = list.methods.len() as u32;
            buf[base..base + 4].copy_from_slice(&(12u32 | 0x8000_0000).to_le_bytes());
            buf[base + 4..base + 8].copy_from_slice(&count.to_le_bytes());
            for (i, m) in list.methods.iter().enumerate() {
                let at = base + 8 + 12 * i;
                let field = addr + 8 + 12 * i as u64;
                for (k, r) in [m.name, m.types, m.imp].into_iter().enumerate() {
                    let target = addr_of(r);
                    let rel = if target == 0 {
                        0
                    } else {
                        target.wrapping_sub(field + 4 * k as u64) as i64
                    };
                    if rel != rel as i32 as i64 {
                        crate::fatal!("relative method list entry out of range");
                    }
                    buf[at + 4 * k..at + 4 * k + 4].copy_from_slice(&(rel as i32).to_le_bytes());
                }
            }
        }
    }
}

/// The merged __objc_imageinfo section: the Objective-C runtime reads
/// exactly one 8-byte record per image.
#[derive(Debug)]
pub struct ObjcImageInfoSection {
    pub hdr: ChunkHeader,
    /// The merged flags word.
    pub flags: u32,
}

impl Default for ObjcImageInfoSection {
    fn default() -> Self {
        Self::new()
    }
}

impl ObjcImageInfoSection {
    pub fn new() -> ObjcImageInfoSection {
        let mut hdr = ChunkHeader::new("__DATA", "__objc_imageinfo");
        hdr.p2align = 2;
        ObjcImageInfoSection { hdr, flags: 0 }
    }
}

pub mod objc_imageinfo {
    use super::*;

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        buf[..4].copy_from_slice(&0u32.to_le_bytes());
        buf[4..8].copy_from_slice(&ctx.objc_imageinfo.flags.to_le_bytes());
    }
}
