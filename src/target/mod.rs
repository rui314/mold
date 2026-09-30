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
use crate::input_sections::Reloc;
use crate::macho::{MachRel, MachSection, R_SCATTERED};

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
    /// another section, or anywhere from outside code.
    PcRel32,
}

/// Why an input relocation record is rejected. ld-prime checks each
/// record as it reads the object and gives up on the object at the
/// first bad one; the variants follow its diagnostics.
#[derive(Clone, Copy, Debug)]
pub enum RelocError {
    /// The relocated field runs past the end of its section or atom.
    OutOfBounds,
    /// A scattered record, which only 32-bit targets define.
    Scattered,
    /// A type the target doesn't define, pcrel, length or extern fields
    /// the type doesn't take, or half of a pair.
    Unsupported,
    /// A record that can't apply where it is, such as to the
    /// instruction there; the text says why.
    Invalid(&'static str),
    /// An extern record's symbol index is past the symbol table.
    SymbolOutOfRange,
    /// A section-relative record's ordinal names no section.
    SectionOutOfRange,
}

/// A rejected relocation record, and why.
#[derive(Clone, Copy, Debug)]
pub struct BadReloc {
    pub rel: MachRel,
    pub error: RelocError,
}

impl BadReloc {
    #[cold]
    #[inline(never)]
    pub fn new(rel: &MachRel, error: RelocError) -> Self {
        Self { rel: *rel, error }
    }
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

/// What ld-prime checks of a record before its type: that it is not
/// scattered, and that its field lies within the section's bytes.
#[inline]
pub fn check_reloc_place(r: &MachRel, contents: &[u8]) -> Result<(), BadReloc> {
    if r.r_address & R_SCATTERED != 0 {
        return Err(BadReloc::new(r, RelocError::Scattered));
    }
    if r.r_address as usize + (1 << r.r_length()) > contents.len() {
        return Err(BadReloc::new(r, RelocError::OutOfBounds));
    }
    Ok(())
}

/// What ld-prime checks of a record after its type: that an extern
/// record's symbol index is in the symbol table, and that another's
/// section ordinal names a section.
#[inline]
pub fn check_reloc_index(r: &MachRel, nsects: usize, nsyms: usize) -> Result<(), BadReloc> {
    if r.is_extern() && r.r_symbolnum() as usize >= nsyms {
        return Err(BadReloc::new(r, RelocError::SymbolOutOfRange));
    }
    if !r.is_extern() && !(1..=nsects).contains(&(r.r_section() as usize)) {
        return Err(BadReloc::new(r, RelocError::SectionOutOfRange));
    }
    Ok(())
}

pub trait Target: Copy + Default + Send + Sync + 'static {
    const NAME: &'static str;
    const CPUTYPE: u32;
    const CPUSUBTYPE: u32;
    const PAGE_SIZE: u64;
    /// The size of one __stubs entry.
    const STUB_SIZE: u64;
    /// The size of the __stub_helper header (the common code that
    /// enters dyld_stub_binder), the distance from one stub's entry to
    /// the next, and the zero padding that distance includes after an
    /// entry's code, which the last entry goes without.
    const STUB_HELPER_HEADER_SIZE: u64;
    const STUB_HELPER_ENTRY_SIZE: u64;
    const STUB_HELPER_ENTRY_PADDING: u64;
    /// The compact unwind encoding mode meaning "use DWARF instead".
    const UNWIND_MODE_DWARF: u32;
    /// The size of one __objc_stubs entry.
    const OBJC_STUB_SIZE: u64;
    /// The span a branch instruction can cover (both directions
    /// together), and the size of one range-extension thunk entry.
    const BRANCH_RANGE: u64;
    const THUNK_SIZE: u64;
    /// The relocation types for a plain absolute word, a subtraction
    /// pair, and a GOT-relative pointer.
    const RELOC_UNSIGNED: u8;
    const RELOC_SUBTRACTOR: u8;
    const RELOC_GOTPC: u8;
    /// What a -r output writes into a 4-byte pcrel GOT reference
    /// (RELOC_GOTPC), whose relocation says all there is: ld-prime
    /// writes 4 on arm64, where the assembler leaves arbitrary bytes,
    /// and keeps the object's addend on x86-64.
    const RELOCATABLE_GOTPC_CELL: Option<u32>;
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
    /// 32-bit words, and the byte offset of the program counter in it.
    const THREAD_STATE_FLAVOR: u32;
    const THREAD_STATE_COUNT: u32;
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

    /// True if a relocation of GOT-load type `r_type` at `offset` sits
    /// on an instruction that loads the pointer, not one that computes
    /// its address: the class-reference fold (objc.rs) turns only such
    /// a reference to a slot into a GOT load. An object's own GOT loads
    /// of a local symbol relax whatever the instruction, or ld-prime
    /// refuses them.
    fn can_relax_got_load(data: &[u8], offset: u32, r_type: u8) -> bool;

    /// The GOT-load form of a plain relocation that loads a pointer
    /// from a data slot (adrp/ldr, or a RIP-relative mov): the type a
    /// reference to an __objc_classrefs slot is rewritten to when the
    /// slot folds into __got, so that the ordinary GOT-load handling
    /// then loads the class from its GOT entry or, for a class defined
    /// in the image, relaxes the load to its address. None for other
    /// relocation types.
    fn got_load_form(r_type: u8) -> Option<u8>;

    /// Which half of a two-instruction address or load a relocation
    /// is: Some(true) for the page (arm64's adrp), Some(false) for the
    /// offset into it (the ldr or add that follows); None for one that
    /// stands alone (x86-64's RIP-relative references).
    fn page_pair_half(_r_type: u8) -> Option<bool> {
        None
    }

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

    /// Writes one range-extension thunk's entries. `addr` is the
    /// thunk's address and `buf` its bytes.
    fn write_thunk(
        ctx: &Context<Self>,
        addr: u64,
        syms: &[crate::symbol::SymbolId],
        buf: &mut [u8],
    );

    /// Converts raw relocation records of one input section into
    /// [`Reloc`]s, rejecting a record ld-prime rejects. Mach-O encodes
    /// addends target-dependently: some are embedded in the relocated
    /// field, some are separate records. `contents` is the section's
    /// bytes and `nsyms` the size of the object's symbol table.
    fn read_relocs(
        file_name: &Path,
        sections: &[MachSection],
        hdr: &MachSection,
        contents: &[u8],
        rels: &[MachRel],
        nsyms: usize,
    ) -> Result<Vec<Reloc>, BadReloc>;

    /// Applies the relocations of one input section to `buf`, its bytes
    /// in the output. `isec` is the subsection's arena index and `base`
    /// its output address.
    fn apply_relocs(ctx: &Context<Self>, rels: &[Reloc], isec: usize, base: u64, buf: &mut [u8]);

    /// Applies LC_LINKER_OPTIMIZATION_HINT rewrites after relocation.
    /// Only arm64 defines hints; the default does nothing.
    fn apply_optimization_hints(_ctx: &Context<Self>, _buf: &mut [u8]) {}
}

/// Returns the target name for a Mach-O CPU type, if we know it.
/// The canonical name of a target named on the command line, borrowed
/// from static storage so that a restart for that target can carry it.
/// The section a non-extern relocation targets: the one its
/// r_symbolnum names (a 1-based section ordinal) when the target
/// address lies in it or one past its end, as ld64 reads it, else the
/// section containing the address. Only the ordinal tells apart
/// sections that share an address - an empty one and its successor,
/// or one section's end and the next one's start.
pub fn nonextern_target_section(
    sections: &[MachSection],
    ordinal: u32,
    addr: u64,
) -> Option<usize> {
    if let Some(i) = (ordinal as usize).checked_sub(1)
        && let Some(sec) = sections.get(i)
        && sec.addr <= addr
        && addr <= sec.addr + sec.size
    {
        return Some(i);
    }
    // The address may be one past a section's end: a DWARF range end
    // or high_pc, or a label after the last instruction.
    sections
        .iter()
        .position(|sec| sec.addr <= addr && addr < sec.addr + sec.size)
        .or_else(|| sections.iter().position(|sec| addr == sec.addr + sec.size))
}

pub fn canonical_name(name: &str) -> Option<&'static str> {
    [Arm64::NAME, X86_64::NAME].into_iter().find(|&canonical| canonical == name)
}

pub fn cputype_name(cputype: u32) -> Option<&'static str> {
    use crate::macho::*;
    match cputype {
        CPU_TYPE_ARM64 => Some("arm64"),
        CPU_TYPE_X86_64 => Some("x86_64"),
        _ => None,
    }
}
