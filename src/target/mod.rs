//! Target descriptions.
//!
//! Every target is a zero-sized marker type implementing [`Target`]. The
//! trait names the target's file format (byte order, ELF class and
//! relocation record form), carries the constants that vary between
//! targets, such as the relocation types used for dynamic linking, and
//! provides the code generation hooks that are inherently target-specific:
//! relocation scanning and application, and the PLT stubs.

pub mod arm32;
pub mod arm64;
pub mod i386;
pub mod loongarch;
pub mod m68k;
pub mod ppc32;
pub mod ppc64v1;
pub mod ppc64v2;
pub mod riscv;
pub mod s390x;
pub mod sh4;
pub mod sparc64;
pub mod x86_64;

pub use arm32::{Arm32, Arm32Be};
pub use arm64::{Arm64, Arm64Be};
pub use i386::I386;
pub use loongarch::{LoongArch32, LoongArch64};
pub use m68k::M68k;
pub use ppc32::Ppc32;
pub use ppc64v1::Ppc64V1;
pub use ppc64v2::Ppc64V2;
pub use riscv::{Riscv32, Riscv32Be, Riscv64, Riscv64Be};
pub use s390x::S390x;
pub use sh4::{Sh4, Sh4Be};
pub use sparc64::Sparc64;
pub use x86_64::X86_64;

use std::fmt;

use crate::context::Context;
use crate::elf::*;
use crate::input_sections::{InputSection, InputSectionExtra, RelocDelta};
use crate::symbol::Symbol;
use crate::thunks::Thunk;
use crate::util::endian::*;

/// Coarse target families, for the few places where generic code needs
/// target-specific behavior that doesn't warrant a trait hook.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    X86_64,
    I386,
    Arm64,
    Arm32,
    RiscV,
    Ppc32,
    Ppc64V1,
    Ppc64V2,
    S390x,
    Sparc64,
    M68k,
    Sh4,
    LoongArch,
}

/// Sizes of range extension thunks, for targets whose branch instructions
/// can't reach the whole address space.
#[derive(Clone, Copy, Debug)]
pub struct ThunkLayout {
    pub header_size: u64,
    pub entry_size: u64,
}

/// The ELF32 or ELF64 forms of the word and of the records that differ
/// between the two classes, keyed by a const parameter. A target that is
/// generic over its word size cannot choose these types by the parameter's
/// value directly, so it takes them from `Class<IS_64>`.
pub struct Class<const IS_64: bool>;

pub trait ElfClass {
    type Word<E: Target>: ElfWord;
    type Sym<E: Target>: SymbolRecord;
    type Phdr<E: Target>: PhdrRecord;
    type Chdr<E: Target>: ChdrRecord;
}

impl ElfClass for Class<false> {
    type Word<E: Target> = U32<E>;
    type Sym<E: Target> = Elf32Sym<E>;
    type Phdr<E: Target> = Elf32Phdr<E>;
    type Chdr<E: Target> = Elf32Chdr<E>;
}

impl ElfClass for Class<true> {
    type Word<E: Target> = U64<E>;
    type Sym<E: Target> = Elf64Sym<E>;
    type Phdr<E: Target> = Elf64Phdr<E>;
    type Chdr<E: Target> = Elf64Chdr<E>;
}

// Target descriptions
pub trait Target: Copy + Default + fmt::Debug + Send + Sync + 'static {
    // The file format: byte order, word size, the record forms that differ
    // between ELF32 and ELF64, and the relocation record form.
    const IS_LITTLE: bool;
    type Word: ElfWord;
    type Sym: SymbolRecord;
    type Phdr: PhdrRecord;
    type Chdr: ChdrRecord;
    type Rel: RelRecord;
    const IS_64: bool = std::mem::size_of::<Self::Word>() == 8;
    const IS_RELA: bool = <Self::Rel as RelRecord>::IS_RELA;
    const WORD_SIZE: usize = std::mem::size_of::<Self::Word>();

    /// Target-specific members embedded directly in each input section.
    type InputSectionExtra: InputSectionExtra;

    const NAME: &'static str;
    const FAMILY: Family;
    const PAGE_SIZE: u64;
    const E_MACHINE: u32;
    const PLT_HDR_SIZE: u64;
    const PLT_SIZE: u64;
    const PLTGOT_SIZE: u64;
    const THUNK: Option<ThunkLayout> = None;
    const SFRAME_ABI: Option<u8> = None;

    /// An instruction that traps, used to fill padding in executable sections.
    const TRAP: &'static [u8];

    const R_COPY: u32;
    const R_GLOB_DAT: u32;
    const R_JUMP_SLOT: u32;
    const R_ABS: u32;
    const R_RELATIVE: u32;
    const R_DTPOFF: u32;
    const R_TPOFF: u32;
    const R_DTPMOD: u32;
    const R_IRELATIVE: Option<u32> = None;
    const R_TLSDESC: Option<u32> = None;
    const R_SFRAME: Option<u32> = None;

    /// Relocation types that denote a function call.
    const R_FUNCALL: &'static [u32];

    const SUPPORTS_IFUNC: bool = Self::R_IRELATIVE.is_some();
    const SUPPORTS_TLSDESC: bool = Self::R_TLSDESC.is_some();
    const SUPPORTS_SFRAME: bool = Self::SFRAME_ABI.is_some();
    const NEEDS_THUNK: bool = Self::THUNK.is_some();

    const IS_X86: bool = matches!(Self::FAMILY, Family::X86_64 | Family::I386);
    const IS_ARM: bool = matches!(Self::FAMILY, Family::Arm64 | Family::Arm32);
    const IS_PPC64: bool = matches!(Self::FAMILY, Family::Ppc64V1 | Family::Ppc64V2);
    const IS_SPARC: bool = matches!(Self::FAMILY, Family::Sparc64);
    const IS_RISCV: bool = matches!(Self::FAMILY, Family::RiscV);
    const IS_LOONGARCH: bool = matches!(Self::FAMILY, Family::LoongArch);

    // The maximum distance of branch instructions used for function calls.
    //
    // The exact origin for computing a destination varies slightly depending
    // on the target architecture. For example, ARM32's B instruction jumps to
    // the branch's address + immediate + 4 (i.e., B with offset 0 jumps to
    // the next instruction), while RISC-V has no such implicit bias. Here, we
    // subtract 32 as a safety margin that is large enough for all targets.
    fn branch_distance() -> i64 {
        // ARM64's branch has 26 bits immediate. The immediate is padded with
        // implicit two-bit zeros because all instructions are 4 bytes aligned
        // and therefore the least two bits are always zero. So the branch
        // operand is effectively 28 bits long. That means the branch range is
        // [-2^27, 2^27) or PC ± 128 MiB.
        //
        // ARM32's Thumb branch has 24 bits immediate, and the instructions are
        // aligned to 2, so it's effectively 25 bits. It's [-2^24, 2^24) or PC ±
        // 16 MiB.
        //
        // ARM32's non-Thumb branches have twice longer range than its Thumb
        // counterparts, but we conservatively use the Thumb's limitation.
        //
        // PPC's branch has 24 bits immediate, and the instructions are aligned
        // to 4, therefore the reach is [-2^25, 2^25) or PC ± 32 MiB.
        let bits = match Self::FAMILY {
            Family::Arm64 => 27,
            Family::Arm32 => 24,
            _ => 25,
        };
        (1 << bits) - 32
    }

    /// Returns the name of a relocation type, for diagnostics.
    fn rel_to_string(r_type: u32) -> std::borrow::Cow<'static, str>;

    /// Rewrites input sections the compiler left half-baked, before
    /// garbage collection. PPC64 ELFv1 dissolves the input `.opd`
    /// sections here.
    fn rewrite_input_sections(_ctx: &mut Context<Self>) {}

    /// Marks symbols that need target-specific synthetic entries, before
    /// relocations are scanned.
    fn scan_symbols(_ctx: &mut Context<Self>) {}

    /// Machine-specific `e_flags` of the output file.
    fn eflags(_ctx: &Context<Self>) -> u32 {
        0
    }

    fn write_plt_header(ctx: &Context<Self>, buf: &mut [u8]);
    fn write_plt_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol);
    fn write_pltgot_entry(ctx: &Context<Self>, buf: &mut [u8], sym: &Symbol);

    /// Applies a relocation in a reconstructed `.eh_frame` section. `loc` is
    /// the relocated location, `p` its address and `val` the resolved value.
    fn apply_eh_reloc(
        ctx: &Context<Self>,
        isec: &InputSection<Self>,
        rel: &ElfRel<Self>,
        loc: &mut [u8],
        p: u64,
        val: u64,
    );

    /// Scans the relocations of an allocated section to find symbols that
    /// need GOT, PLT or dynamic relocation entries.
    fn scan_relocations(ctx: &Context<Self>, isec: &InputSection<Self>);

    /// Whether `rel` is a word-size absolute relocation. The output section
    /// applies those instead of `apply_reloc_alloc`, since only they can be
    /// promoted to dynamic relocations.
    fn is_absrel(rel: &ElfRel<Self>) -> bool {
        rel.r_type() == Self::R_ABS
    }

    /// Applies relocations to a copy of an allocated section's contents.
    fn apply_reloc_alloc(
        ctx: &Context<Self>,
        isec: &InputSection<Self>,
        rels: &mut [ElfRel<Self>],
        buf: &mut [u8],
    );

    /// Applies relocations to a copy of a non-allocated section's contents.
    fn apply_reloc_nonalloc(ctx: &Context<Self>, isec: &InputSection<Self>, buf: &mut [u8]);

    /// Whether a call must go through a thunk however close its target
    /// is: a processor mode switch on ARM32, or TOC setup on PowerPC.
    fn always_needs_thunk(_ctx: &Context<Self>, _sym: &Symbol, _rel: &ElfRel<Self>) -> bool {
        false
    }

    /// Lays out the entries of a thunk placed at `addr` once addresses are
    /// known: the offset of each entry, followed by the thunk's size.
    /// Entries have a fixed size unless the target says otherwise.
    fn thunk_offsets(_ctx: &Context<Self>, thunk: &Thunk, _addr: u64) -> Vec<u64> {
        thunk.fixed_offsets::<Self>()
    }

    /// Writes the code of a thunk placed at `addr`.
    fn write_thunk(_ctx: &Context<Self>, _thunk: &Thunk, _addr: u64, _buf: &mut [u8]) {
        unreachable!("{} has no range extension thunks", Self::NAME)
    }

    /// Finds the bytes linker relaxation can remove from an executable
    /// section, for targets whose branches are shortened by the linker
    /// (RISC-V, LoongArch). The result lists the cumulative number of
    /// bytes removed up to each relocation that shrinks.
    fn shrink_section(_ctx: &Context<Self>, _isec: &InputSection<Self>) -> Vec<RelocDelta> {
        Vec::new()
    }

    /// Rewrites the output image once every section is copied. Big-endian
    /// ARM32 converts code to BE8 form here.
    fn finish_output(_ctx: &Context<Self>, _buf: &mut [u8]) {}

    /// Writes an addend into a relocated location, for REL-type targets
    /// producing relocatable output.
    fn write_addend(_loc: &mut [u8], _val: i64, _rel: &ElfRel<Self>) {}

    /// The addend of a relocation. REL-type targets store it in the
    /// relocated location, given as `loc`.
    fn get_addend(_loc: &[u8], rel: &ElfRel<Self>) -> i64 {
        rel.r_addend()
    }

    // Integers in the target's byte order, for section contents and other
    // data that is not a record.
    fn read_u16(bytes: &[u8]) -> u16 {
        if Self::IS_LITTLE { read_ul16(bytes) } else { read_ub16(bytes) }
    }

    fn read_u32(bytes: &[u8]) -> u32 {
        if Self::IS_LITTLE { read_ul32(bytes) } else { read_ub32(bytes) }
    }

    fn read_u64(bytes: &[u8]) -> u64 {
        if Self::IS_LITTLE { read_ul64(bytes) } else { read_ub64(bytes) }
    }

    fn read_i32(bytes: &[u8]) -> i32 {
        if Self::IS_LITTLE { read_il32(bytes) } else { read_ib32(bytes) }
    }

    fn read_i64(bytes: &[u8]) -> i64 {
        if Self::IS_LITTLE { read_il64(bytes) } else { read_ib64(bytes) }
    }

    fn write_u16(bytes: &mut [u8], value: u16) {
        if Self::IS_LITTLE { write_ul16(bytes, value) } else { write_ub16(bytes, value) }
    }

    fn write_u32(bytes: &mut [u8], value: u32) {
        if Self::IS_LITTLE { write_ul32(bytes, value) } else { write_ub32(bytes, value) }
    }

    fn write_u64(bytes: &mut [u8], value: u64) {
        if Self::IS_LITTLE { write_ul64(bytes, value) } else { write_ub64(bytes, value) }
    }

    fn write_i32(bytes: &mut [u8], value: i32) {
        if Self::IS_LITTLE { write_il32(bytes, value) } else { write_ib32(bytes, value) }
    }

    fn write_i64(bytes: &mut [u8], value: i64) {
        if Self::IS_LITTLE { write_il64(bytes, value) } else { write_ib64(bytes, value) }
    }
}

/// Maps an `-m` emulation name to a target name.
pub fn emulation_to_target(emulation: &str) -> Option<&'static str> {
    Some(match emulation {
        "elf_x86_64" => "x86_64",
        "elf_i386" => "i386",
        "aarch64elf" | "aarch64linux" => "arm64",
        "aarch64elfb" | "aarch64linuxb" => "arm64be",
        "armelf_linux_eabi" => "arm32",
        "armelfb_linux_eabi" => "arm32be",
        "elf64lriscv" => "riscv64",
        "elf64briscv" => "riscv64be",
        "elf32lriscv" => "riscv32",
        "elf32briscv" => "riscv32be",
        "elf32ppc" | "elf32ppclinux" => "ppc32",
        "elf64ppc" => "ppc64v1",
        "elf64lppc" => "ppc64v2",
        "elf64_s390" => "s390x",
        "elf64_sparc" => "sparc64",
        "m68kelf" => "m68k",
        "shlelf" | "shlelf_linux" => "sh4",
        "shelf" | "shelf_linux" => "sh4be",
        "elf64loongarch" => "loongarch64",
        "elf32loongarch" => "loongarch32",
        _ => return None,
    })
}
