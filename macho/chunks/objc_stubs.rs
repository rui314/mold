//! __TEXT,__objc_stubs: the linker-synthesized _objc_msgSend$<selector>
//! stubs, with the selector strings and references they load.

use crate::arch::Target;
use crate::chunks::symtab::{NamedEntry, local_msym};
use crate::chunks::{ChunkHeader, OutputSectionId};
use crate::context::Context;
use crate::macho::*;
use crate::symbol::SymbolId;

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

    /// Address of the selector reference slot `i` in the tail of the
    /// __objc_selrefs output section: objc stub `i`'s, or past the
    /// stubs, extra selector reference `i - stubs`.
    pub fn selref_addr<E: Target>(&self, ctx: &Context<E>, i: usize) -> u64 {
        let osec = ctx.output_section(self.selrefs.unwrap());
        osec.hdr.addr + osec.tail_off + i as u64 * 8
    }
}

impl Default for ObjcStubsSection {
    fn default() -> Self {
        Self::new()
    }
}

/// The size of one __objc_stubs entry.
pub fn entry_size<E: Target>(ctx: &Context<E>) -> u64 {
    if ctx.args.objc_stubs_small { E::OBJC_SMALL_STUB_SIZE } else { E::OBJC_STUB_SIZE }
}

/// The offset of objc stub `idx` in __objc_stubs.
pub fn entry_offset<E: Target>(ctx: &Context<E>, idx: u32) -> u64 {
    idx as u64 * entry_size(ctx)
}

pub fn update_shdr<E: Target>(ctx: &mut Context<E>) {
    ctx.objc_stubs.hdr.size = ctx.objc_stubs.symbols.len() as u64 * entry_size(ctx);
    // Code, which -text_exec moves as it does __stubs.
    if ctx.args.text_exec {
        ctx.objc_stubs.hdr.segname = b"__TEXT_EXEC";
    }
    // 32-byte aligned, but arm64's small stubs word-aligned.
    if ctx.args.objc_stubs_small {
        ctx.objc_stubs.hdr.p2align = 2;
    }
}

/// The selector stubs' local symbols, each a non-external symbol with
/// N_PEXT set (nm: "was a private external"), as ld64 lists them -
/// NetNewsWire's debug dylib has 851 _objc_msgSend$... entries.
pub fn populate_symtab<E: Target>(ctx: &Context<E>, out: &mut Vec<NamedEntry>) {
    let hdr = &ctx.objc_stubs.hdr;
    for (i, &(sym, _)) in ctx.objc_stubs.symbols.iter().enumerate() {
        let addr = hdr.addr + entry_offset(ctx, i as u32);
        let ent = MachSym { n_type: N_PEXT | N_SECT, ..local_msym(hdr.sect_idx, addr) };
        out.push((ctx.symbols[sym].name(), ent, None));
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    E::write_objc_stubs(ctx, ctx.objc_stubs.hdr.addr, buf);
}

/// Writes the selector names of the stubs into `buf`, the tail of
/// __objc_methname.
pub fn write_methnames<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let data = &ctx.objc_stubs.methname_data;
    buf[..data.len()].copy_from_slice(data);
}

/// Writes the synthesized selector references into `buf`, the tail of
/// __objc_selrefs: the stubs', each to its selector's name in the tail
/// of __objc_methname, then the method lists' extra ones.
pub fn write_selrefs<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let stubs = &ctx.objc_stubs;
    for i in 0..stubs.symbols.len() {
        // Its selector's name, in the tail of __objc_methname.
        let methname = ctx.output_section(stubs.methname.unwrap());
        let val = methname.hdr.addr + methname.tail_off + stubs.methname_offs[i];
        buf[i * 8..i * 8 + 8].copy_from_slice(&val.to_le_bytes());
    }
    let n = stubs.symbols.len();
    for (j, &name) in stubs.extra_selrefs.iter().enumerate() {
        let val = ctx.isecs[name as usize].addr(ctx);
        buf[(n + j) * 8..(n + j) * 8 + 8].copy_from_slice(&val.to_le_bytes());
    }
}
