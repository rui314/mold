//! __TEXT,__delay_stubs and __TEXT,__delay_helper: the code through
//! which an image reaches the symbols of the dylibs it initializes at
//! the first use of one (-delay-l and the like). dyld loads and binds
//! such a dylib at launch as any other, but runs its initializers only
//! when the image dlopen()s it: each stub and helper checks the flag
//! word of the dylib's dlopen helper, has the helper dlopen the dylib
//! (which sets the flag) the first time, and then goes on through the
//! symbol's __got slot.

use crate::arch::Target;
use crate::chunks::ChunkHeader;
use crate::context::Context;
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

/// The offset of stub `idx` in __delay_stubs.
pub fn stub_offset<E: Target>(idx: u32) -> u64 {
    idx as u64 * E::DELAY_STUB_SIZE
}

pub fn copy_stubs<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    E::write_delay_stubs(ctx, ctx.delay_init.stubs_hdr.addr, buf);
}

pub fn copy_helper<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    E::write_delay_helper(ctx, ctx.delay_init.helper_hdr.addr, buf);
}
