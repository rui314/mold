//! Target descriptions.
//!
//! Every target is a zero-sized marker type implementing [`Target`], which
//! selects a CPU type and carries the constants and code that differ
//! between targets: the stub and thunk shapes, and the target-dependent
//! relocation handling. The rest of the linker is generic over it. Each
//! target is instantiated in a crate of its own under arch/.

mod arm64;
mod x86_64;

pub use arm64::Arm64;
pub use x86_64::X86_64;

use std::path::Path;

use crate::context::Context;
use crate::error::RawPath;
use crate::input_sections::{Reloc, RelocTarget};
use crate::macho::{MachRel, MachSection};

/// How a relocation type uses its target symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelocClass {
    /// A branch, which needs a stub if the target is imported.
    Branch,
    /// A reference through the GOT.
    Got,
    /// A load through the GOT that can relax to a direct address
    /// computation when the target is local.
    GotLoad,
    /// A reference to a thread-local variable pointer.
    Tlv,
    /// A direct reference.
    Plain,
}

/// How LC_SEGMENT_SPLIT_INFO records a reference a relocation type
/// makes, when it crosses sections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitRef {
    /// An absolute address in data, recorded wherever it points.
    Pointer,
    /// The first of a SUBTRACTOR/UNSIGNED pair: a distance, recorded
    /// wherever it points.
    Subtractor,
    /// arm64's adrp and the ldr or add under it, and its 26-bit
    /// branch: recorded only when they reach another section.
    Page,
    PageOff,
    Branch26,
    /// A 32-bit PC-relative displacement: recorded when it reaches
    /// another section.
    PcRel32,
}

/// How a relocation reaches a symbol of a dylib dyld loads lazily, or
/// initializes at its first use (see passes::create_lazy_loads and
/// delay_init::create_delay_init).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LazyRef {
    /// A branch: to the symbol's call helper or stub.
    Call,
    /// The instruction that computes a GOT slot's address for a load
    /// (arm64's adrp, x86-64's movq): it calls a load helper instead.
    Load,
    /// The instruction that loads from the slot the adrp above found
    /// (arm64's ldr): from the symbol's __lazy_load_got slot, or a
    /// delay-init dylib's symbol's __got slot as ever.
    Slot,
    /// x86-64's cmpq $0 of the GOT slot: it calls a compare helper
    /// instead.
    Cmp,
    /// A reference that can't be made lazy or delayed: a pointer or a
    /// difference, which dyld would have to fill in at launch, or a
    /// form of instruction no helper stands in for.
    Unsupported,
}

/// One form of a relocation record - its pcrel, length (log2 of the
/// field's size) and extern fields - as a bit in a set of the forms a
/// type takes. The fields are bits 24 to 27 of the record's second
/// word, which select the bit.
pub const fn reloc_form(pcrel: bool, length: u32, ext: bool) -> u16 {
    1 << (pcrel as u32 | length << 1 | (ext as u32) << 3)
}

/// Whether a record's form is one of `forms`, a set of reloc_form bits.
/// A target lists each type's set in a match, which compiles to a
/// table: a lookup is much cheaper than testing the fields one by one.
#[inline]
pub fn has_reloc_form(r: &MachRel, forms: u16) -> bool {
    forms >> ((r.bits >> 24) & 0xf) & 1 != 0
}

/// Reports relocation record `r` of section `hdr` of object `file`,
/// which the linker can't apply for the reason `what`.
#[cold]
#[inline(never)]
pub fn bad_reloc(file: &Path, hdr: &MachSection, r: &MachRel, what: &str) -> ! {
    crate::fatal!(
        "{}:({},{}): {what} at 0x{:x}: r_type={}, r_length={}, r_pcrel={}, r_extern={}",
        file.raw(),
        crate::error::raw(hdr.segname()),
        crate::error::raw(hdr.sectname()),
        r.r_address,
        r.r_type(),
        r.r_length(),
        r.is_pcrel() as u8,
        r.is_extern() as u8
    )
}

pub trait Target: Copy + Default + Send + Sync + 'static {
    const NAME: &'static str;
    const CPUTYPE: u32;
    const CPUSUBTYPE: u32;
    const PAGE_SIZE: u64;
    /// The size of one __stubs entry.
    const STUB_SIZE: u64;
    /// The size of the __stub_helper header (the common code that
    /// enters dyld_stub_binder), and the distance from one stub's entry
    /// to the next.
    const STUB_HELPER_HEADER_SIZE: u64;
    const STUB_HELPER_ENTRY_SIZE: u64;
    /// The compact unwind encoding mode meaning "use DWARF instead".
    const UNWIND_MODE_DWARF: u32;
    /// The size of one __objc_stubs entry, and of one under
    /// -objc_stubs_small (see Args::objc_stubs_small).
    const OBJC_STUB_SIZE: u64;
    const OBJC_SMALL_STUB_SIZE: u64;
    /// The alignment of __lazy_helpers, and whether a symbol's call
    /// helper goes through a __lazy_load_got slot of its own, apart
    /// from the one its GOT loads read (ld-prime's x86-64 ones do).
    const LAZY_HELPERS_P2ALIGN: u32;
    const LAZY_CALL_OWN_SLOT: bool;
    /// The size of a __delay_stubs entry and of a dlopen helper, and the
    /// alignment of __delay_stubs and __delay_helper.
    const DELAY_STUB_SIZE: u64;
    const DLOPEN_HELPER_SIZE: u32;
    const DELAY_P2ALIGN: u32;
    /// The span a branch instruction can cover (both directions
    /// together), and the size of one range-extension thunk entry.
    const BRANCH_RANGE: u64;
    const THUNK_SIZE: u64;
    /// The relocation types for a plain absolute word, a subtraction
    /// pair, and a GOT-relative pointer.
    const RELOC_UNSIGNED: u8;
    const RELOC_SUBTRACTOR: u8;
    const RELOC_GOTPC: u8;
    /// The explicit-addend relocation type, for targets that have one.
    const RELOC_ADDEND: u8;
    /// Where the linker's own code materializes an address
    /// PC-relatively, for LC_SEGMENT_SPLIT_INFO: the split-info kinds
    /// of one such address (arm64's adrp, then the ldr or add 4 bytes
    /// on; x86-64's 32-bit displacement), and where it sits in a
    /// __stubs entry (its pointer slot), the __stub_helper header
    /// (__dyld_private, then dyld_stub_binder's GOT slot) and an
    /// __objc_stubs entry (its selector reference, then
    /// _objc_msgSend's GOT slot).
    const SPLIT_PCREL_KINDS: &'static [u8];
    const STUB_REF_OFF: u64;
    const STUB_HELPER_REF_OFFS: [u64; 2];
    const OBJC_STUB_REF_OFFS: [u64; 2];
    /// The register state LC_UNIXTHREAD holds: its flavor, its size in
    /// 32-bit words, and the byte offsets of the stack pointer and the
    /// program counter in it.
    const THREAD_STATE_FLAVOR: u32;
    const THREAD_STATE_COUNT: u32;
    const THREAD_STATE_SP_OFFSET: usize;
    const THREAD_STATE_PC_OFFSET: usize;

    /// Whether re-emitting this relocation type in a relocatable output
    /// needs an explicit addend record when its addend is nonzero.
    fn relocatable_needs_addend(r_type: u8) -> bool;

    /// The distance folded into a pcrel relocation's embedded addend
    /// beyond the field itself (x86-64's SIGNED_1/2/4), which a field
    /// written back for a relocatable output must leave out again.
    fn reloc_bias(_r_type: u8) -> i64 {
        0
    }

    /// Classifies a relocation type by how it uses its target.
    fn classify_reloc(r_type: u8) -> RelocClass;

    /// How LC_SEGMENT_SPLIT_INFO records a relocation type's reference.
    fn split_ref(r_type: u8) -> SplitRef;

    /// Writes the __stubs section: for each symbol in `ctx.stubs.symbols`, a
    /// jump through the symbol's __got slot. `addr` is the section's
    /// address and `buf` its bytes in the output.
    fn write_stubs(ctx: &Context<Self>, addr: u64, buf: &mut [u8]);

    /// Writes the __TEXT,__stub_helper section: the header that pushes
    /// __dyld_private and jumps to dyld_stub_binder through the GOT,
    /// then one entry per stub that loads the stub's lazy-bind record
    /// offset and branches to the header. `addr` is the section's
    /// address and `buf` its bytes.
    fn write_stub_helper(ctx: &Context<Self>, addr: u64, buf: &mut [u8]);

    /// Writes the __objc_stubs section: for each _objc_msgSend$<sel>
    /// symbol, code that loads the selector from its __objc_selrefs
    /// slot and tail-calls _objc_msgSend through the GOT.
    fn write_objc_stubs(ctx: &Context<Self>, addr: u64, buf: &mut [u8]);

    /// Writes the __lazy_helpers section: each helper of
    /// `ctx.lazy_helpers.helpers` (see chunks::lazy_helpers).
    fn write_lazy_helpers(ctx: &Context<Self>, addr: u64, buf: &mut [u8]);

    /// The size of a __lazy_helpers entry of a kind.
    fn lazy_helper_size(kind: crate::chunks::lazy_helpers::LazyUse) -> u32;

    /// Writes the __delay_stubs section: each stub of
    /// `ctx.delay_init.stubs` (see chunks::delay_init).
    fn write_delay_stubs(ctx: &Context<Self>, addr: u64, buf: &mut [u8]);

    /// Writes the __delay_helper section: the helpers for GOT loads,
    /// then the dlopen helpers (see chunks::delay_init).
    fn write_delay_helper(ctx: &Context<Self>, addr: u64, buf: &mut [u8]);

    /// The size of a __delay_helper entry for GOT loads of a kind.
    fn delay_helper_size(kind: crate::chunks::delay_init::DelayUse) -> u32;

    /// Where a delay-init stub or helper refers to what, for
    /// LC_SEGMENT_SPLIT_INFO: offsets in it, with the split-info kind
    /// of each reference.
    fn delay_refs(
        code: crate::chunks::delay_init::DelayCode,
    ) -> Vec<(u32, u8, crate::chunks::delay_init::DelayTarget)>;

    /// Where a __lazy_helpers entry of a kind refers to what, for
    /// LC_SEGMENT_SPLIT_INFO: offsets in the entry, with the split-info
    /// kind of each reference.
    fn lazy_helper_refs(
        kind: crate::chunks::lazy_helpers::LazyUse,
    ) -> Vec<(u32, u8, crate::chunks::lazy_helpers::LazyTarget)>;

    /// How relocation `r`, of a subsection whose contents are `data`,
    /// reaches a symbol of a dylib dyld loads lazily, or a delay-init
    /// dylib's.
    fn lazy_ref(r: &Reloc, data: &[u8]) -> LazyRef;

    /// For a GOT load of such a symbol, the instruction at `offset` of
    /// the subsection whose contents are `data` (see LazyRef::Load):
    /// the register it loads, and whether the helper it calls must be
    /// its own (arm64's frameless code: see LazyUse::Load).
    fn lazy_load_site(data: &[u8], offset: u32) -> (u8, bool);

    /// The name of a register a load helper fills, as ld-prime puts it
    /// in the helper's symbol.
    fn lazy_register_name(reg: u8) -> String;

    /// Writes one range-extension thunk's entries. `addr` is the
    /// thunk's address and `buf` its bytes.
    fn write_thunk(
        ctx: &Context<Self>,
        addr: u64,
        syms: &[crate::symbol::SymbolId],
        buf: &mut [u8],
    );

    /// Converts raw relocation records of one input section, `hdr`,
    /// into [`Reloc`]s, failing the link on one it can't apply (see
    /// bad_reloc). Mach-O encodes addends target-dependently: some are
    /// embedded in the relocated field, some are separate records.
    /// `contents` is the section's bytes.
    fn read_relocs(
        file_name: &Path,
        sections: &[MachSection],
        hdr: &MachSection,
        contents: &[u8],
        rels: &[MachRel],
    ) -> Vec<Reloc>;

    /// Applies the relocations of one input section to `buf`, its bytes
    /// in the output. `isec` is the subsection's arena index and `base`
    /// its output address.
    fn apply_relocs(ctx: &Context<Self>, rels: &[Reloc], isec: usize, base: u64, buf: &mut [u8]);

    /// Applies LC_LINKER_OPTIMIZATION_HINT rewrites after relocation.
    /// Only arm64 defines hints; the default does nothing.
    fn apply_optimization_hints(_ctx: &Context<Self>, _buf: &mut [u8]) {}
}

/// The section a non-extern record `r` of object `file` refers to, and
/// the offset in it of `addr`, the address the record points at. The
/// section is the one r_symbolnum names (a 1-based ordinal), wherever
/// `addr` lies: only the ordinal tells apart sections that share an
/// address - an empty one and its successor, or one section's end and
/// the next one's start.
pub fn section_target(
    file: &Path,
    sections: &[MachSection],
    r: &MachRel,
    addr: u64,
) -> (RelocTarget, i64) {
    let i = (r.r_section() as usize).wrapping_sub(1);
    let Some(sec) = sections.get(i) else {
        crate::fatal!("{}: bad relocation: {}", file.raw(), r.r_address);
    };
    (RelocTarget::Section(i as u32), addr.wrapping_sub(sec.addr) as i64)
}

/// The helper that relocation `r` of subsection `isec` calls in place
/// of a GOT load (or x86-64's compare), if it reaches a symbol of a
/// dylib dyld loads lazily (see LazyUse) or of a delay-init dylib (see
/// DelayUse): its address, and whether it is the site's own, which
/// branches back to the site rather than returning.
pub fn load_helper<E: Target>(ctx: &Context<E>, isec: usize, r: &Reloc) -> Option<(u64, bool)> {
    use crate::chunks::delay_init::DelayUse;
    use crate::chunks::lazy_helpers::LazyUse;

    let site = (isec as u32, r.offset);
    let lazy = &ctx.lazy_helpers;
    if !lazy.sites.is_empty()
        && let Some(&i) = lazy.sites.get(&site)
    {
        let own = matches!(lazy.helpers[i as usize].kind, LazyUse::Load { site: Some(_), .. });
        return Some((ctx.lazy_helper_addr(i as usize), own));
    }
    let delay = &ctx.delay_init;
    if !delay.sites.is_empty()
        && let Some(&i) = delay.sites.get(&site)
    {
        let own = matches!(delay.helpers[i as usize].kind, DelayUse::Load { site: Some(_), .. });
        return Some((ctx.delay_helper_addr(i as usize), own));
    }
    None
}

/// The canonical name of a target named on the command line, borrowed
/// from static storage so that a restart for that target can carry it.
pub fn canonical_name(name: &str) -> Option<&'static str> {
    [Arm64::NAME, X86_64::NAME].into_iter().find(|&canonical| canonical == name)
}

/// Returns the target name for a Mach-O CPU type, if we know it.
pub fn cputype_name(cputype: u32) -> Option<&'static str> {
    use crate::macho::*;
    match cputype {
        CPU_TYPE_ARM64 => Some("arm64"),
        CPU_TYPE_X86_64 => Some("x86_64"),
        _ => None,
    }
}
