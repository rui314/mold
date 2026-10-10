//! The __delay_stubs and __delay_helper sections of the __TEXT segment hold
//! the code through which an image reaches the symbols of its delay-init
//! dylibs (see crate::delay_init for the feature). A call of such a symbol
//! branches to a stub, `_foo$delayInitStub`, and a GOT load of it calls a
//! helper (see DelayUse). Each dylib also has a dlopen helper, last in
//! __delay_helper, which calls dlopen() on the dylib and sets a flag word.
//! A stub or helper checks the flag, calls the dlopen helper the first
//! time, and then goes on through the symbol's __got slot.

use crate::arch::Target;
use crate::chunks::split_info::{Entry, Place, Places, push};
use crate::chunks::symtab::{NamedEntry, local_msym};
use crate::chunks::{ChunkHeader, ChunkId, stubs};
use crate::context::Context;
use crate::input_sections::Reloc;
use crate::macho::*;
use crate::symbol::SymbolId;

/// A __delay_stubs entry, which the calls of a symbol branch to:
/// `_foo$delayInitStub`. It jumps through a __got slot of its own when
/// GOT loads of the symbol read another one.
#[derive(Debug)]
pub struct DelayStub {
    pub sym: SymbolId,
    /// The stub's local symbol.
    pub name: &'static [u8],
    /// The dlopen helper of the symbol's dylib (an index into
    /// DelayInit::dlopens), and the stub's __got slot.
    pub dlopen: u32,
    pub got: u32,
}

/// What a __delay_helper entry before the dlopen helpers stands in for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DelayUse {
    /// A load of the symbol's address from the GOT into register
    /// `reg`: arm64's adrp of the slot's page becomes a call of
    /// `_foo$loadHelper_<reg>`, which returns with the slot's page in
    /// the register, for the ldr under the adrp to load from it;
    /// x86-64's movq becomes a call of one that loads the slot. arm64
    /// code that may not have saved its link register branches instead
    /// to a helper of its own, which branches back past the adrp:
    /// `site` names that adrp (subsection, offset).
    Load { reg: u8, site: Option<(u32, u32)> },
    /// x86-64's test of a weak import, cmpq $0 of its GOT slot: it
    /// becomes a call of `_foo$cmpHelper`, which compares the slot
    /// instead, leaving the flags for the code after the call.
    Cmp,
}

/// A delay-init stub or helper, as LC_SEGMENT_SPLIT_INFO tells them
/// apart (see Target::delay_refs).
#[derive(Clone, Copy, Debug)]
pub enum DelayCode {
    Stub,
    Helper(DelayUse),
    Dlopen,
}

/// What a delay-init stub or helper refers to: the flag word, the GOT
/// slot it goes through, the dlopen helper it calls, the code past the
/// site a frameless load helper returns to, and a dlopen helper's
/// install name and _dlopen's stub.
#[derive(Clone, Copy, Debug)]
pub enum DelayTarget {
    Flag,
    Slot,
    DlopenHelper,
    Site,
    Name,
    Dlopen,
}

/// A __delay_helper entry for GOT loads or compares of a symbol.
#[derive(Debug)]
pub struct DelayHelper {
    pub sym: SymbolId,
    pub kind: DelayUse,
    /// The helper's local symbol, as ld-prime names it.
    pub name: &'static [u8],
    pub dlopen: u32,
    /// Where the helper lies in __delay_helper.
    pub offset: u32,
}

/// The dlopen helper of a dylib, last in __delay_helper:
/// `_dlopenHelper$libfoo.dylib`. It calls dlopen(install name, 0) with
/// the argument registers saved and then sets the flag word,
/// `_dlopenHelperFlag$libfoo.dylib` in __data.
#[derive(Debug)]
pub struct DlopenHelper {
    pub install_name: Vec<u8>,
    /// The helper's local symbol and its flag's.
    pub name: &'static [u8],
    pub flag_name: &'static [u8],
    /// The subsections of the flag and of the install name's C string
    /// in __cstring.
    pub flag: u32,
    pub string: u32,
    pub offset: u32,
}

#[derive(Debug)]
pub struct DelayInit {
    pub stubs_hdr: ChunkHeader,
    pub helper_hdr: ChunkHeader,
    /// The stubs by name, the helpers for GOT loads by name, and the
    /// dlopen helpers by install name, as ld-prime lays them out.
    pub stubs: Vec<DelayStub>,
    pub helpers: Vec<DelayHelper>,
    pub dlopens: Vec<DlopenHelper>,
    /// The helper each rewritten GOT load goes to, by the subsection
    /// and offset of the instruction it replaces.
    pub sites: hashbrown::HashMap<(u32, u32), u32>,
    /// _dlopen, which the dlopen helpers call through its stub.
    pub dlopen_sym: Option<SymbolId>,
}

impl DelayInit {
    pub fn new() -> Self {
        let flags = S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
        let mut stubs_hdr = ChunkHeader::new(b"__TEXT", b"__delay_stubs");
        stubs_hdr.flags = flags;
        let mut helper_hdr = ChunkHeader::new(b"__TEXT", b"__delay_helper");
        helper_hdr.flags = flags;
        Self {
            stubs_hdr,
            helper_hdr,
            stubs: Vec::new(),
            helpers: Vec::new(),
            dlopens: Vec::new(),
            sites: Default::default(),
            dlopen_sym: None,
        }
    }

    /// Returns the address of __delay_stubs entry `i`.
    pub fn stub_addr<E: Target>(&self, i: usize) -> u64 {
        self.stubs_hdr.addr + stub_offset::<E>(i as u32)
    }

    /// Returns the address of __delay_helper's load helper `i`.
    pub fn helper_addr(&self, i: usize) -> u64 {
        self.helper_hdr.addr + self.helpers[i].offset as u64
    }

    /// Returns the address of __delay_helper's dlopen helper `i`.
    pub fn dlopen_helper_addr(&self, i: usize) -> u64 {
        self.helper_hdr.addr + self.dlopens[i].offset as u64
    }
}

impl Default for DelayInit {
    fn default() -> Self {
        Self::new()
    }
}

/// The helper that relocation `r` of subsection `isec` calls in place
/// of a GOT load (or x86-64's compare), if it reaches a symbol of a
/// delay-init dylib (see DelayUse): its address, and whether it is the
/// site's own, which branches back to the site rather than returning.
pub fn load_helper<E: Target>(ctx: &Context<E>, isec: usize, r: &Reloc) -> Option<(u64, bool)> {
    let delay = &ctx.delay_init;
    if delay.sites.is_empty() {
        return None;
    }
    let &i = delay.sites.get(&(isec as u32, r.offset))?;
    let own = matches!(delay.helpers[i as usize].kind, DelayUse::Load { site: Some(_), .. });
    Some((delay.helper_addr(i as usize), own))
}

/// The offset of stub `idx` in __delay_stubs.
pub fn stub_offset<E: Target>(idx: u32) -> u64 {
    idx as u64 * E::DELAY_STUB_SIZE
}

/// Sizes the stubs, which go where the other stubs do.
pub fn update_stubs_shdr<E: Target>(ctx: &mut Context<E>) {
    let delay = &mut ctx.delay_init;
    delay.stubs_hdr.segname = ctx.stubs.hdr.segname;
    delay.stubs_hdr.p2align = E::DELAY_P2ALIGN;
    delay.stubs_hdr.size = delay.stubs.len() as u64 * E::DELAY_STUB_SIZE;
}

/// Sizes the helpers, which go where the stubs do: the dlopen helpers
/// come last.
pub fn update_helper_shdr<E: Target>(ctx: &mut Context<E>) {
    let delay = &mut ctx.delay_init;
    let last = delay.dlopens.last().unwrap();
    delay.helper_hdr.segname = ctx.stubs.hdr.segname;
    delay.helper_hdr.p2align = E::DELAY_P2ALIGN;
    delay.helper_hdr.size = (last.offset + E::DLOPEN_HELPER_SIZE) as u64;
}

/// The stubs' local symbols, like selector stubs' with N_PEXT set.
pub fn populate_stubs_symtab<E: Target>(ctx: &Context<E>, out: &mut Vec<NamedEntry>) {
    let delay = &ctx.delay_init;
    for (i, stub) in delay.stubs.iter().enumerate() {
        let addr = delay.stub_addr::<E>(i);
        let ent = MachSym { n_type: N_PEXT | N_SECT, ..local_msym(delay.stubs_hdr.sect_idx, addr) };
        out.push((stub.name, ent, None));
    }
}

/// The helpers' local symbols, the dlopen helpers' last. (Their flags'
/// are Context::extra_local_syms.)
pub fn populate_helper_symtab<E: Target>(ctx: &Context<E>, out: &mut Vec<NamedEntry>) {
    let delay = &ctx.delay_init;
    let sect = delay.helper_hdr.sect_idx;
    for (i, h) in delay.helpers.iter().enumerate() {
        let addr = delay.helper_addr(i);
        out.push((h.name, local_msym(sect, addr), None));
    }
    for (i, d) in delay.dlopens.iter().enumerate() {
        let addr = delay.dlopen_helper_addr(i);
        out.push((d.name, local_msym(sect, addr), None));
    }
}

pub fn copy_stubs<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    E::write_delay_stubs(ctx, ctx.delay_init.stubs_hdr.addr, buf);
}

pub fn copy_helper<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    E::write_delay_helper(ctx, ctx.delay_init.helper_hdr.addr, buf);
}

/// The stubs' and helpers' references to other sections, for
/// LC_SEGMENT_SPLIT_INFO (see DelayTarget).
pub(crate) fn split_info_entries<E: Target>(p: &Places<'_, E>, out: &mut Vec<Entry>) {
    let ctx = p.ctx;
    let delay = &ctx.delay_init;
    let Some(dlopen) = delay.dlopen_sym else { return };
    let dlopen_stub = stubs::entry_offset::<E>(ctx.symbols[dlopen].stub_idx(&ctx.symbols).unwrap());
    let dlopen_helper =
        |i: u32| p.chunk_addr(ChunkId::DelayHelper, delay.dlopen_helper_addr(i as usize));
    let mut push_refs = |from: Place, code: DelayCode, resolve: &dyn Fn(DelayTarget) -> _| {
        for (off, kind, to) in E::delay_refs(code) {
            let from = (from.0, from.1 + off as u64);
            let to: Option<Place> = resolve(to);
            if to.is_some_and(|to| to.0 != from.0) {
                push(out, from, kind, to);
            }
        }
    };
    for (i, stub) in delay.stubs.iter().enumerate() {
        let from = p.chunk(ChunkId::DelayStubs, stub_offset::<E>(i as u32));
        let flag = delay.dlopens[stub.dlopen as usize].flag;
        push_refs(from, DelayCode::Stub, &|to| match to {
            DelayTarget::Flag => p.isec(flag as usize),
            DelayTarget::Slot => Some(p.got_index(stub.got as usize)),
            DelayTarget::DlopenHelper => Some(dlopen_helper(stub.dlopen)),
            _ => None,
        });
    }
    for (i, h) in delay.helpers.iter().enumerate() {
        let flag = delay.dlopens[h.dlopen as usize].flag;
        push_refs(p.delay_helper(i), DelayCode::Helper(h.kind), &|to| match to {
            DelayTarget::Flag => p.isec(flag as usize),
            DelayTarget::Slot => Some(p.got_slot(h.sym)),
            DelayTarget::DlopenHelper => Some(dlopen_helper(h.dlopen)),
            DelayTarget::Site => match h.kind {
                DelayUse::Load { site: Some((isec, off)), .. } => {
                    p.isec(isec as usize).map(|(n, o)| (n, o + off as u64 + 4))
                }
                _ => None,
            },
            _ => None,
        });
    }
    for (i, d) in delay.dlopens.iter().enumerate() {
        push_refs(dlopen_helper(i as u32), DelayCode::Dlopen, &|to| match to {
            DelayTarget::Flag => p.isec(d.flag as usize),
            DelayTarget::Name => p.isec(d.string as usize),
            DelayTarget::Dlopen => Some(p.chunk(ChunkId::Stubs, dlopen_stub)),
            _ => None,
        });
    }
}
