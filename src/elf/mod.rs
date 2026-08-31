//! This file defines integral types for file input/output. We need to use
//! these types instead of the plain integers (such as uint32_t or int32_t)
//! when reading from/writing to an mmap'ed file area for the following
//! reasons:
//!
//! 1. mold is always a cross linker and should not depend on what host it
//!    is running on. For example, users should be able to run mold on a
//!    little-endian x86 machine to create a big-endian s390x binary.
//!
//! 2. Even though data members in all ELF data strucutres are naturally
//!    aligned, they are not guaranteed to be aligned on memory because of
//!    archive files. Archive files (.a files) align each file only to a
//!    2 byte boundary, so anything larger than 2 bytes may be misaligned
//!    in an mmap'ed memory. Misaligned access is an undefined behavior in
//!    C/C++, so we shouldn't cast an arbitrary pointer to a uint32_t, for
//!    example, to read a 32 bit value.
//!
//! The data types defined in this file are independent of the host byte
//! order and are designed to avoid unaligned access.
//!
//! Note that in C/C++, memcpy is a portable and efficient way to access
//! unaligned data, as it is typically treated as an intrinsic. Compilers
//! can easily optimize memcpy calls in this file into a single load or
//! store instruction.
//!
//! ELF file format definitions.
//!
//! ELF records use their target-dependent file representation, with
//! byte-backed integer fields handling byte order and unaligned access.
//! Records whose ELF32 and ELF64 forms have the same field order are generic
//! over the target word type. Symbols, program headers and compression
//! headers have genuinely different layouts and convert as complete records
//! when target-independent bookkeeping needs them. Thus the big tables —
//! section headers, symbols and relocations — stay in the input files rather
//! than being copied. [`Arch`](crate::arch::Arch) selects the layout at
//! compile time, while [`RecordLayout`] is used where the target type isn't at
//! hand.

mod consts;

use std::fmt;
use std::marker::PhantomData;

pub use consts::*;

use crate::arch::Arch;

// ELF types
/// The on-disk layout of an ELF file: word size, byte order and
/// relocation record format. Targets implement this through [`Arch`], and
/// a few plain layouts exist for peeking into files before the target is
/// known.
pub trait Layout: Copy + Default + Send + Sync + 'static {
    type Endian: Endian;
    type Word: ElfWord<Endian = Self::Endian>;
    type Sym: SymbolRecord<Endian = Self::Endian>;
    type Phdr: ProgramHeaderRecord<Endian = Self::Endian>;
    type Chdr: CompressionHeaderRecord<Endian = Self::Endian>;
    type Rel: RelRecord<Endian = Self::Endian>;
    const IS_64: bool;
    const IS_RELA: bool;

    const WORD_SIZE: usize = if Self::IS_64 { 8 } else { 4 };
}

macro_rules! plain_layout {
    ($name:ident, $endian:ty, $word:ty, $sym:ty, $phdr:ty, $chdr:ty, $rel:ty, $is_64:expr) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;

        impl Layout for $name {
            type Endian = $endian;
            type Word = $word;
            type Sym = $sym;
            type Phdr = $phdr;
            type Chdr = $chdr;
            type Rel = $rel;
            const IS_64: bool = $is_64;
            const IS_RELA: bool = true;
        }
    };
}

plain_layout!(
    Elf32Le,
    LittleEndian,
    Ul32,
    Elf32Sym<LittleEndian>,
    Elf32Phdr<LittleEndian>,
    Elf32Chdr<LittleEndian>,
    Elf32RelaLe,
    false
);
plain_layout!(
    Elf64Le,
    LittleEndian,
    Ul64,
    Elf64Sym<LittleEndian>,
    Elf64Phdr<LittleEndian>,
    Elf64Chdr<LittleEndian>,
    Elf64RelaLe,
    true
);
plain_layout!(
    Elf32Be,
    BigEndian,
    Ub32,
    Elf32Sym<BigEndian>,
    Elf32Phdr<BigEndian>,
    Elf32Chdr<BigEndian>,
    Elf32RelaBe,
    false
);
plain_layout!(
    Elf64Be,
    BigEndian,
    Ub64,
    Elf64Sym<BigEndian>,
    Elf64Phdr<BigEndian>,
    Elf64Chdr<BigEndian>,
    Elf64RelaBe,
    true
);

/// Byte order of an ELF file, as a type-level marker.
pub trait Endian: Copy + Default + Eq + Send + Sync + fmt::Debug + 'static {
    const IS_LITTLE: bool;
    const IS_NATIVE: bool = Self::IS_LITTLE == cfg!(target_endian = "little");

    fn read_u16(bytes: &[u8]) -> u16 {
        let bytes = bytes[..2].try_into().unwrap();
        if Self::IS_LITTLE {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        }
    }

    fn read_u32(bytes: &[u8]) -> u32 {
        let bytes = bytes[..4].try_into().unwrap();
        if Self::IS_LITTLE {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        }
    }

    fn read_u64(bytes: &[u8]) -> u64 {
        let bytes = bytes[..8].try_into().unwrap();
        if Self::IS_LITTLE {
            u64::from_le_bytes(bytes)
        } else {
            u64::from_be_bytes(bytes)
        }
    }

    fn read_i32(bytes: &[u8]) -> i32 {
        Self::read_u32(bytes) as i32
    }

    fn read_i16(bytes: &[u8]) -> i16 {
        Self::read_u16(bytes) as i16
    }

    fn read_i64(bytes: &[u8]) -> i64 {
        Self::read_u64(bytes) as i64
    }

    fn write_u16(bytes: &mut [u8], value: u16) {
        let encoded = if Self::IS_LITTLE {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        };
        bytes[..2].copy_from_slice(&encoded);
    }

    fn write_u32(bytes: &mut [u8], value: u32) {
        let encoded = if Self::IS_LITTLE {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        };
        bytes[..4].copy_from_slice(&encoded);
    }

    fn write_u64(bytes: &mut [u8], value: u64) {
        let encoded = if Self::IS_LITTLE {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        };
        bytes[..8].copy_from_slice(&encoded);
    }

    fn write_i32(bytes: &mut [u8], value: i32) {
        Self::write_u32(bytes, value as u32);
    }

    fn write_i16(bytes: &mut [u8], value: i16) {
        Self::write_u16(bytes, value as u16);
    }

    fn write_i64(bytes: &mut [u8], value: i64) {
        Self::write_u64(bytes, value as u64);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LittleEndian;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BigEndian;

impl Endian for LittleEndian {
    const IS_LITTLE: bool = true;
}

impl Endian for BigEndian {
    const IS_LITTLE: bool = false;
}

/// A record with a target-dependent on-disk encoding.
pub trait Record: Sized {
    /// The encoded size in bytes.
    fn size<E: Layout>() -> usize;

    /// Decodes a record from the beginning of `bytes`.
    fn parse<E: Layout>(bytes: &[u8]) -> Self;

    /// Encodes the record to the beginning of `buf`.
    fn write<E: Layout>(&self, buf: &mut [u8]);

    /// Decodes a contiguous table of records.
    fn parse_all<E: Layout>(bytes: &[u8]) -> Vec<Self> {
        bytes
            .chunks_exact(Self::size::<E>())
            .map(Self::parse::<E>)
            .collect()
    }

    /// Encodes a slice of records back to back.
    fn write_all<E: Layout>(records: &[Self], buf: &mut [u8]) {
        for (record, slot) in records.iter().zip(buf.chunks_exact_mut(Self::size::<E>())) {
            record.write::<E>(slot);
        }
    }
}

/// A record stored in its target-dependent file representation.
///
/// # Safety
///
/// Implementations must have alignment one, contain no padding or references,
/// and accept every bit pattern. These requirements let records in possibly
/// unaligned archive members be read and written without host dependencies.
pub unsafe trait FileRecord: Clone + Copy + Default + Send + Sync + 'static {
    fn size() -> usize {
        std::mem::size_of::<Self>()
    }

    fn parse(bytes: &[u8]) -> Self {
        assert!(bytes.len() >= Self::size());
        debug_assert_eq!(std::mem::align_of::<Self>(), 1);
        // SAFETY: the trait guarantees that every bit pattern is valid, and
        // the length check proves that a complete record is available.
        unsafe { bytes.as_ptr().cast::<Self>().read_unaligned() }
    }

    fn parse_all(bytes: &[u8]) -> Vec<Self> {
        bytes.chunks_exact(Self::size()).map(Self::parse).collect()
    }

    fn write(&self, buf: &mut [u8]) {
        assert!(buf.len() >= Self::size());
        // SAFETY: `buf` has room for the complete record, and copying bytes
        // does not depend on its alignment.
        unsafe {
            std::ptr::copy_nonoverlapping(
                std::ptr::from_ref(self).cast::<u8>(),
                buf.as_mut_ptr(),
                Self::size(),
            );
        }
    }

    fn write_all(records: &[Self], buf: &mut [u8]) {
        for (record, slot) in records.iter().zip(buf.chunks_exact_mut(Self::size())) {
            record.write(slot);
        }
    }
}

/// The ELF file header.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ElfEhdr<E: Layout> {
    pub e_ident: [u8; 16],
    pub e_type: U16<E::Endian>,
    pub e_machine: U16<E::Endian>,
    pub e_version: U32<E::Endian>,
    pub e_entry: E::Word,
    pub e_phoff: E::Word,
    pub e_shoff: E::Word,
    pub e_flags: U32<E::Endian>,
    pub e_ehsize: U16<E::Endian>,
    pub e_phentsize: U16<E::Endian>,
    pub e_phnum: U16<E::Endian>,
    pub e_shentsize: U16<E::Endian>,
    pub e_shnum: U16<E::Endian>,
    pub e_shstrndx: U16<E::Endian>,
}

// SAFETY: all fields are byte-backed integers or bytes, and `repr(C)` does
// not insert padding between fields with alignment one.
unsafe impl<E: Layout> FileRecord for ElfEhdr<E> {}

const _: () = assert!(std::mem::size_of::<ElfEhdr<Elf32Le>>() == 52);
const _: () = assert!(std::mem::size_of::<ElfEhdr<Elf64Le>>() == 64);
const _: () = assert!(std::mem::align_of::<ElfEhdr<Elf32Le>>() == 1);
const _: () = assert!(std::mem::align_of::<ElfEhdr<Elf64Le>>() == 1);

/// A section header.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ElfShdr<E: Layout> {
    pub sh_name: U32<E::Endian>,
    pub sh_type: U32<E::Endian>,
    pub sh_flags: E::Word,
    pub sh_addr: E::Word,
    pub sh_offset: E::Word,
    pub sh_size: E::Word,
    pub sh_link: U32<E::Endian>,
    pub sh_info: U32<E::Endian>,
    pub sh_addralign: E::Word,
    pub sh_entsize: E::Word,
}

// SAFETY: all fields are byte-backed integers, and `repr(C)` does not insert
// padding between fields with alignment one.
unsafe impl<E: Layout> FileRecord for ElfShdr<E> {}

const _: () = assert!(std::mem::size_of::<ElfShdr<Elf32Le>>() == 40);
const _: () = assert!(std::mem::size_of::<ElfShdr<Elf64Le>>() == 64);
const _: () = assert!(std::mem::align_of::<ElfShdr<Elf32Le>>() == 1);
const _: () = assert!(std::mem::align_of::<ElfShdr<Elf64Le>>() == 1);

/// A section header decoded for target-independent bookkeeping.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SectionHeader {
    pub sh_name: u32,
    pub sh_type: u32,
    pub sh_flags: u64,
    pub sh_addr: u64,
    pub sh_offset: u64,
    pub sh_size: u64,
    pub sh_link: u32,
    pub sh_info: u32,
    pub sh_addralign: u64,
    pub sh_entsize: u64,
}

impl Record for SectionHeader {
    fn size<E: Layout>() -> usize {
        ElfShdr::<E>::size()
    }

    fn parse<E: Layout>(bytes: &[u8]) -> Self {
        let shdr = ElfShdr::<E>::parse(bytes);
        SectionHeader {
            sh_name: shdr.sh_name.get(),
            sh_type: shdr.sh_type.get(),
            sh_flags: shdr.sh_flags.get(),
            sh_addr: shdr.sh_addr.get(),
            sh_offset: shdr.sh_offset.get(),
            sh_size: shdr.sh_size.get(),
            sh_link: shdr.sh_link.get(),
            sh_info: shdr.sh_info.get(),
            sh_addralign: shdr.sh_addralign.get(),
            sh_entsize: shdr.sh_entsize.get(),
        }
    }

    fn write<E: Layout>(&self, buf: &mut [u8]) {
        ElfShdr::<E> {
            sh_name: U32::new(self.sh_name),
            sh_type: U32::new(self.sh_type),
            sh_flags: E::Word::new(self.sh_flags),
            sh_addr: E::Word::new(self.sh_addr),
            sh_offset: E::Word::new(self.sh_offset),
            sh_size: E::Word::new(self.sh_size),
            sh_link: U32::new(self.sh_link),
            sh_info: U32::new(self.sh_info),
            sh_addralign: E::Word::new(self.sh_addralign),
            sh_entsize: E::Word::new(self.sh_entsize),
        }
        .write(buf);
    }
}

const _: () = assert!(std::mem::size_of::<SectionHeader>() == 64);

/// A program header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Elf64Phdr<E: Endian> {
    pub p_type: U32<E>,
    pub p_flags: U32<E>,
    pub p_offset: U64<E>,
    pub p_vaddr: U64<E>,
    pub p_paddr: U64<E>,
    pub p_filesz: U64<E>,
    pub p_memsz: U64<E>,
    pub p_align: U64<E>,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Elf32Phdr<E: Endian> {
    pub p_type: U32<E>,
    pub p_offset: U32<E>,
    pub p_vaddr: U32<E>,
    pub p_paddr: U32<E>,
    pub p_filesz: U32<E>,
    pub p_memsz: U32<E>,
    pub p_flags: U32<E>,
    pub p_align: U32<E>,
}

// SAFETY: all fields are byte-backed integers, and `repr(C)` does not insert
// padding between fields with alignment one.
unsafe impl<E: Endian> FileRecord for Elf64Phdr<E> {}
// SAFETY: see the Elf64 implementation.
unsafe impl<E: Endian> FileRecord for Elf32Phdr<E> {}

const _: () = assert!(std::mem::size_of::<Elf32Phdr<LittleEndian>>() == 32);
const _: () = assert!(std::mem::size_of::<Elf64Phdr<LittleEndian>>() == 56);
const _: () = assert!(std::mem::align_of::<Elf32Phdr<LittleEndian>>() == 1);
const _: () = assert!(std::mem::align_of::<Elf64Phdr<LittleEndian>>() == 1);

/// The two physical program-header layouts convert as complete records.
pub trait ProgramHeaderRecord: FileRecord {
    type Endian: Endian;

    fn decode(&self) -> ProgramHeader;
    fn encode(phdr: &ProgramHeader) -> Self;
}

impl<E: Endian> ProgramHeaderRecord for Elf64Phdr<E> {
    type Endian = E;

    fn decode(&self) -> ProgramHeader {
        ProgramHeader {
            p_type: self.p_type.get(),
            p_flags: self.p_flags.get(),
            p_offset: self.p_offset.get(),
            p_vaddr: self.p_vaddr.get(),
            p_paddr: self.p_paddr.get(),
            p_filesz: self.p_filesz.get(),
            p_memsz: self.p_memsz.get(),
            p_align: self.p_align.get(),
        }
    }

    fn encode(phdr: &ProgramHeader) -> Self {
        Elf64Phdr {
            p_type: U32::new(phdr.p_type),
            p_flags: U32::new(phdr.p_flags),
            p_offset: U64::new(phdr.p_offset),
            p_vaddr: U64::new(phdr.p_vaddr),
            p_paddr: U64::new(phdr.p_paddr),
            p_filesz: U64::new(phdr.p_filesz),
            p_memsz: U64::new(phdr.p_memsz),
            p_align: U64::new(phdr.p_align),
        }
    }
}

impl<E: Endian> ProgramHeaderRecord for Elf32Phdr<E> {
    type Endian = E;

    fn decode(&self) -> ProgramHeader {
        ProgramHeader {
            p_type: self.p_type.get(),
            p_offset: u64::from(self.p_offset.get()),
            p_vaddr: u64::from(self.p_vaddr.get()),
            p_paddr: u64::from(self.p_paddr.get()),
            p_filesz: u64::from(self.p_filesz.get()),
            p_memsz: u64::from(self.p_memsz.get()),
            p_flags: self.p_flags.get(),
            p_align: u64::from(self.p_align.get()),
        }
    }

    fn encode(phdr: &ProgramHeader) -> Self {
        Elf32Phdr {
            p_type: U32::new(phdr.p_type),
            p_offset: U32::new(phdr.p_offset as u32),
            p_vaddr: U32::new(phdr.p_vaddr as u32),
            p_paddr: U32::new(phdr.p_paddr as u32),
            p_filesz: U32::new(phdr.p_filesz as u32),
            p_memsz: U32::new(phdr.p_memsz as u32),
            p_flags: U32::new(phdr.p_flags),
            p_align: U32::new(phdr.p_align as u32),
        }
    }
}

pub type ElfPhdr<E> = <E as Layout>::Phdr;

/// A program header decoded for address calculations.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProgramHeader {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

impl Record for ProgramHeader {
    fn size<E: Layout>() -> usize {
        E::Phdr::size()
    }

    fn parse<E: Layout>(bytes: &[u8]) -> Self {
        E::Phdr::parse(bytes).decode()
    }

    fn write<E: Layout>(&self, buf: &mut [u8]) {
        E::Phdr::encode(self).write(buf);
    }
}

const _: () = assert!(std::mem::size_of::<ProgramHeader>() == 56);

/// A symbol table entry.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Elf64Sym<E: Endian> {
    pub st_name: U32<E>,
    type_and_bind: u8,
    other: u8,
    pub st_shndx: U16<E>,
    pub st_value: U64<E>,
    pub st_size: U64<E>,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Elf32Sym<E: Endian> {
    pub st_name: U32<E>,
    pub st_value: U32<E>,
    pub st_size: U32<E>,
    type_and_bind: u8,
    other: u8,
    pub st_shndx: U16<E>,
}

// SAFETY: all fields are byte-backed integers or bytes, and `repr(C)` does
// not insert padding between fields with alignment one.
unsafe impl<E: Endian> FileRecord for Elf64Sym<E> {}
// SAFETY: see the Elf64 implementation.
unsafe impl<E: Endian> FileRecord for Elf32Sym<E> {}

const _: () = assert!(std::mem::size_of::<Elf32Sym<LittleEndian>>() == 16);
const _: () = assert!(std::mem::size_of::<Elf64Sym<LittleEndian>>() == 24);
const _: () = assert!(std::mem::align_of::<Elf32Sym<LittleEndian>>() == 1);
const _: () = assert!(std::mem::align_of::<Elf64Sym<LittleEndian>>() == 1);

/// The two physical symbol layouts convert as complete records.
pub trait SymbolRecord: FileRecord {
    type Endian: Endian;

    fn decode(&self) -> SymbolEntry;
    fn encode(sym: &SymbolEntry) -> Self;
}

impl<E: Endian> SymbolRecord for Elf64Sym<E> {
    type Endian = E;

    fn decode(&self) -> SymbolEntry {
        SymbolEntry {
            st_name: self.st_name.get(),
            type_and_bind: self.type_and_bind,
            other: self.other,
            st_shndx: self.st_shndx.get(),
            st_value: self.st_value.get(),
            st_size: self.st_size.get(),
        }
    }

    fn encode(sym: &SymbolEntry) -> Self {
        Elf64Sym {
            st_name: U32::new(sym.st_name),
            type_and_bind: sym.type_and_bind,
            other: sym.other,
            st_shndx: U16::new(sym.st_shndx),
            st_value: U64::new(sym.st_value),
            st_size: U64::new(sym.st_size),
        }
    }
}

impl<E: Endian> SymbolRecord for Elf32Sym<E> {
    type Endian = E;

    fn decode(&self) -> SymbolEntry {
        SymbolEntry {
            st_name: self.st_name.get(),
            st_value: u64::from(self.st_value.get()),
            st_size: u64::from(self.st_size.get()),
            type_and_bind: self.type_and_bind,
            other: self.other,
            st_shndx: self.st_shndx.get(),
        }
    }

    fn encode(sym: &SymbolEntry) -> Self {
        Elf32Sym {
            st_name: U32::new(sym.st_name),
            st_value: U32::new(sym.st_value as u32),
            st_size: U32::new(sym.st_size as u32),
            type_and_bind: sym.type_and_bind,
            other: sym.other,
            st_shndx: U16::new(sym.st_shndx),
        }
    }
}

pub type ElfSym<E> = <E as Layout>::Sym;

/// A symbol table entry decoded for target-independent bookkeeping.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SymbolEntry {
    pub st_name: u32,
    type_and_bind: u8,
    other: u8,
    pub st_shndx: u16,
    pub st_value: u64,
    pub st_size: u64,
}

impl SymbolEntry {
    pub fn st_type(&self) -> u32 {
        (self.type_and_bind & 0xf) as u32
    }

    pub fn st_bind(&self) -> u32 {
        (self.type_and_bind >> 4) as u32
    }

    pub fn st_visibility(&self) -> u32 {
        (self.other & 3) as u32
    }

    pub fn set_type(&mut self, ty: u32) {
        self.type_and_bind = (self.type_and_bind & 0xf0) | (ty as u8 & 0xf);
    }

    pub fn set_bind(&mut self, bind: u32) {
        self.type_and_bind = (self.type_and_bind & 0x0f) | ((bind as u8) << 4);
    }

    pub fn set_visibility(&mut self, visibility: u32) {
        self.other = (self.other & !3) | (visibility as u8 & 3);
    }

    pub fn is_undef(&self) -> bool {
        self.st_shndx == SHN_UNDEF as u16
    }

    pub fn is_abs(&self) -> bool {
        self.st_shndx == SHN_ABS as u16
    }

    pub fn is_common(&self) -> bool {
        self.st_shndx == SHN_COMMON as u16
    }

    pub fn is_weak(&self) -> bool {
        self.st_bind() == STB_WEAK
    }

    pub fn is_undef_weak(&self) -> bool {
        self.is_undef() && self.is_weak()
    }

    /// The `st_other` bit that AArch64 uses to mark functions with a
    /// non-standard calling convention.
    pub fn arm64_variant_pcs(&self) -> bool {
        self.other & 0x80 != 0
    }

    pub fn set_arm64_variant_pcs(&mut self, value: bool) {
        self.other = (self.other & !0x80) | if value { 0x80 } else { 0 };
    }

    /// The RISC-V counterpart of [`Self::arm64_variant_pcs`].
    pub fn riscv_variant_cc(&self) -> bool {
        self.other & 0x80 != 0
    }

    pub fn set_riscv_variant_cc(&mut self, value: bool) {
        self.other = (self.other & !0x80) | if value { 0x80 } else { 0 };
    }

    /// The distance between a PPC64 ELFv2 function's global and local entry
    /// points, encoded in the top three bits of `st_other`.
    pub fn ppc64_local_entry(&self) -> u8 {
        self.other >> 5
    }

    pub fn set_ppc64_local_entry(&mut self, value: u8) {
        self.other = (self.other & 0x1f) | ((value & 7) << 5);
    }

    pub fn ppc64_preserves_r2(&self) -> bool {
        self.ppc64_local_entry() != 1
    }

    pub fn ppc64_uses_toc(&self) -> bool {
        self.ppc64_local_entry() > 1
    }
}

impl Record for SymbolEntry {
    fn size<E: Layout>() -> usize {
        E::Sym::size()
    }

    fn parse<E: Layout>(bytes: &[u8]) -> Self {
        E::Sym::parse(bytes).decode()
    }

    fn write<E: Layout>(&self, buf: &mut [u8]) {
        E::Sym::encode(self).write(buf);
    }
}

const _: () = assert!(std::mem::size_of::<SymbolEntry>() == 24);

macro_rules! endian_integer {
    ($name:ident, $int:ty, $size:expr, $read:ident, $write:ident) => {
        #[repr(transparent)]
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub struct $name<E: Endian> {
            bytes: [u8; $size],
            endian: PhantomData<E>,
        }

        impl<E: Endian> $name<E> {
            #[inline(always)]
            pub fn new(value: $int) -> Self {
                let mut result = Self::default();
                result.set(value);
                result
            }

            #[inline(always)]
            pub fn get(&self) -> $int {
                E::$read(&self.bytes)
            }

            #[inline(always)]
            pub fn set(&mut self, value: $int) {
                E::$write(&mut self.bytes, value);
            }
        }
    };
}

endian_integer!(U16, u16, 2, read_u16, write_u16);
endian_integer!(U32, u32, 4, read_u32, write_u32);
endian_integer!(U64, u64, 8, read_u64, write_u64);
endian_integer!(I16, i16, 2, read_i16, write_i16);
endian_integer!(I32, i32, 4, read_i32, write_i32);
endian_integer!(I64, i64, 8, read_i64, write_i64);

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct U24<E: Endian> {
    bytes: [u8; 3],
    endian: PhantomData<E>,
}

impl<E: Endian> U24<E> {
    #[inline(always)]
    pub fn new(value: u32) -> Self {
        let mut result = Self::default();
        result.set(value);
        result
    }

    #[inline(always)]
    pub fn get(&self) -> u32 {
        if E::IS_LITTLE {
            u32::from_le_bytes([self.bytes[0], self.bytes[1], self.bytes[2], 0])
        } else {
            u32::from_be_bytes([0, self.bytes[0], self.bytes[1], self.bytes[2]])
        }
    }

    #[inline(always)]
    pub fn set(&mut self, value: u32) {
        let bytes = if E::IS_LITTLE {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        };
        if E::IS_LITTLE {
            self.bytes.copy_from_slice(&bytes[..3]);
        } else {
            self.bytes.copy_from_slice(&bytes[1..]);
        }
    }
}

/// A word-sized unsigned integer in an ELF file.
///
/// # Safety
///
/// Implementations must have alignment one, contain no padding, and accept
/// every bit pattern.
pub unsafe trait ElfWord: Clone + Copy + Default + Send + Sync + 'static {
    type Endian: Endian;

    fn new(value: u64) -> Self;
    fn get(&self) -> u64;
    fn set(&mut self, value: u64);
}

// SAFETY: U32 is a transparent wrapper around a byte array.
unsafe impl<E: Endian> ElfWord for U32<E> {
    type Endian = E;

    #[inline(always)]
    fn new(value: u64) -> Self {
        U32::new(value as u32)
    }

    #[inline(always)]
    fn get(&self) -> u64 {
        u64::from(U32::get(self))
    }

    #[inline(always)]
    fn set(&mut self, value: u64) {
        U32::set(self, value as u32);
    }
}

// SAFETY: U64 is a transparent wrapper around a byte array.
unsafe impl<E: Endian> ElfWord for U64<E> {
    type Endian = E;

    #[inline(always)]
    fn new(value: u64) -> Self {
        U64::new(value)
    }

    #[inline(always)]
    fn get(&self) -> u64 {
        U64::get(self)
    }

    #[inline(always)]
    fn set(&mut self, value: u64) {
        U64::set(self, value);
    }
}

/// An ELF relocation record in its target-dependent file representation.
///
/// # Safety
///
/// Implementations must have alignment one, contain no padding or references,
/// and accept every bit pattern. These requirements let relocation sections in
/// possibly unaligned archive members be viewed as slices of records.
pub unsafe trait RelRecord:
    Clone + Copy + fmt::Debug + Default + Eq + Send + Sync + 'static
{
    type Endian: Endian;

    fn new(r_offset: u64, r_type: u32, r_sym: u32, r_addend: i64) -> Self;

    fn r_offset(&self) -> u64;
    fn set_r_offset(&mut self, value: u64);
    fn r_type(&self) -> u32;
    fn set_r_type(&mut self, value: u32);
    /// Returns the symbol index without decoding the other relocation fields.
    fn r_sym(&self) -> u32;
    fn set_r_sym(&mut self, value: u32);
    fn r_addend(&self) -> i64;
    fn set_r_addend(&mut self, value: i64);

    /// Returns true if a given relocation is of type used for direct
    /// function call.
    #[inline(always)]
    fn is_func_call<E: Arch>(&self) -> bool {
        E::R_FUNCALL.contains(&self.r_type())
    }

    /// Formats the relocation type name for the given target.
    fn type_name<E: Arch>(&self) -> String {
        E::rel_to_string(self.r_type())
    }
}

// Depending on the target, ElfRel may or may not contain r_addend member.
// The relocation record containing r_addend is called RELA, and that
// without r_addend is called REL.
//
// If REL, relocation addends are stored as parts of section contents.
// That means we add a computed value to an existing value when writing a
// relocated value if REL. If RELA, we just overwrite an existing value
// with a newly computed value.
//
// We don't want to have too many `if (REL)`s and `if (RELA)`s in our
// codebase, so ElfRel always takes r_addend as a constructor argument.
// If it's REL, the argument will simply be ignored.

pub(crate) type Ul24 = U24<LittleEndian>;
pub(crate) type Ul32 = U32<LittleEndian>;
pub(crate) type Ul64 = U64<LittleEndian>;
pub(crate) type Il32 = I32<LittleEndian>;
pub(crate) type Il64 = I64<LittleEndian>;

pub(crate) type Ub24 = U24<BigEndian>;
pub(crate) type Ub32 = U32<BigEndian>;
pub(crate) type Ub64 = U64<BigEndian>;
pub(crate) type Ib32 = I32<BigEndian>;
pub(crate) type Ib64 = I64<BigEndian>;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Elf64RelaLe {
    r_offset: Ul64,
    r_type: Ul32,
    r_sym: Ul32,
    r_addend: Il64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Elf64RelaBe {
    r_offset: Ub64,
    r_sym: Ub32,
    r_type: Ub32,
    r_addend: Ib64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Elf32RelaLe {
    r_offset: Ul32,
    r_type: u8,
    r_sym: Ul24,
    r_addend: Il32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Elf32RelaBe {
    r_offset: Ub32,
    r_sym: Ub24,
    r_type: u8,
    r_addend: Ib32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Elf64RelLe {
    r_offset: Ul64,
    r_type: Ul32,
    r_sym: Ul32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Elf64RelBe {
    r_offset: Ub64,
    r_sym: Ub32,
    r_type: Ub32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Elf32RelLe {
    r_offset: Ul32,
    r_type: u8,
    r_sym: Ul24,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Elf32RelBe {
    r_offset: Ub32,
    r_sym: Ub24,
    r_type: u8,
}

//
// Target-specific ELF data types
//

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sparc64Rela {
    r_offset: Ub64,
    r_sym: Ub32,
    // SPARC keeps a second addend in the upper bits of the type field;
    // its backend separates the two.
    pub(crate) r_type_data: Ub24, // SPARC-specific: used for R_SPARC_OLO10
    /// The relocation type proper, without the second addend.
    r_type: u8,
    r_addend: Ib64,
}

trait UnsignedField {
    fn get_u64(&self) -> u64;
    fn set_u64(&mut self, value: u64);
}

impl UnsignedField for u8 {
    #[inline(always)]
    fn get_u64(&self) -> u64 {
        u64::from(*self)
    }

    #[inline(always)]
    fn set_u64(&mut self, value: u64) {
        *self = value as u8;
    }
}

macro_rules! impl_unsigned_field {
    ($name:ident) => {
        impl<E: Endian> UnsignedField for $name<E> {
            #[inline(always)]
            fn get_u64(&self) -> u64 {
                u64::from(self.get())
            }

            #[inline(always)]
            fn set_u64(&mut self, value: u64) {
                self.set(value as _);
            }
        }
    };
}

impl_unsigned_field!(U24);
impl_unsigned_field!(U32);
impl_unsigned_field!(U64);

trait SignedField {
    fn get_i64(&self) -> i64;
    fn set_i64(&mut self, value: i64);
}

macro_rules! impl_signed_field {
    ($name:ident) => {
        impl<E: Endian> SignedField for $name<E> {
            #[inline(always)]
            fn get_i64(&self) -> i64 {
                i64::from(self.get())
            }

            #[inline(always)]
            fn set_i64(&mut self, value: i64) {
                self.set(value as _);
            }
        }
    };
}

impl_signed_field!(I32);
impl_signed_field!(I64);

macro_rules! impl_rela_record {
    ($name:ty, $endian:ty) => {
        unsafe impl RelRecord for $name {
            type Endian = $endian;

            #[inline(always)]
            fn new(r_offset: u64, r_type: u32, r_sym: u32, r_addend: i64) -> Self {
                let mut rel = Self::default();
                rel.set_r_offset(r_offset);
                rel.set_r_type(r_type);
                rel.set_r_sym(r_sym);
                rel.set_r_addend(r_addend);
                rel
            }

            #[inline(always)]
            fn r_offset(&self) -> u64 {
                self.r_offset.get_u64()
            }

            #[inline(always)]
            fn set_r_offset(&mut self, value: u64) {
                self.r_offset.set_u64(value);
            }

            #[inline(always)]
            fn r_type(&self) -> u32 {
                self.r_type.get_u64() as u32
            }

            #[inline(always)]
            fn set_r_type(&mut self, value: u32) {
                self.r_type.set_u64(u64::from(value));
            }

            #[inline(always)]
            fn r_sym(&self) -> u32 {
                self.r_sym.get_u64() as u32
            }

            #[inline(always)]
            fn set_r_sym(&mut self, value: u32) {
                self.r_sym.set_u64(u64::from(value));
            }

            #[inline(always)]
            fn r_addend(&self) -> i64 {
                self.r_addend.get_i64()
            }

            #[inline(always)]
            fn set_r_addend(&mut self, value: i64) {
                self.r_addend.set_i64(value);
            }
        }
    };
}

macro_rules! impl_rel_record {
    ($name:ty, $endian:ty) => {
        unsafe impl RelRecord for $name {
            type Endian = $endian;

            #[inline(always)]
            fn new(r_offset: u64, r_type: u32, r_sym: u32, _r_addend: i64) -> Self {
                let mut rel = Self::default();
                rel.set_r_offset(r_offset);
                rel.set_r_type(r_type);
                rel.set_r_sym(r_sym);
                rel
            }

            #[inline(always)]
            fn r_offset(&self) -> u64 {
                self.r_offset.get_u64()
            }

            #[inline(always)]
            fn set_r_offset(&mut self, value: u64) {
                self.r_offset.set_u64(value);
            }

            #[inline(always)]
            fn r_type(&self) -> u32 {
                self.r_type.get_u64() as u32
            }

            #[inline(always)]
            fn set_r_type(&mut self, value: u32) {
                self.r_type.set_u64(u64::from(value));
            }

            #[inline(always)]
            fn r_sym(&self) -> u32 {
                self.r_sym.get_u64() as u32
            }

            #[inline(always)]
            fn set_r_sym(&mut self, value: u32) {
                self.r_sym.set_u64(u64::from(value));
            }

            #[inline(always)]
            fn r_addend(&self) -> i64 {
                0
            }

            #[inline(always)]
            fn set_r_addend(&mut self, _value: i64) {}
        }
    };
}

impl_rela_record!(Elf64RelaLe, LittleEndian);
impl_rela_record!(Elf64RelaBe, BigEndian);
impl_rela_record!(Elf32RelaLe, LittleEndian);
impl_rela_record!(Elf32RelaBe, BigEndian);
impl_rel_record!(Elf64RelLe, LittleEndian);
impl_rel_record!(Elf64RelBe, BigEndian);
impl_rel_record!(Elf32RelLe, LittleEndian);
impl_rel_record!(Elf32RelBe, BigEndian);
impl_rela_record!(Sparc64Rela, BigEndian);

pub type ElfRel<E> = <E as Layout>::Rel;

const _: () = assert!(std::mem::size_of::<Elf64RelaLe>() == 24);
const _: () = assert!(std::mem::size_of::<Elf64RelaBe>() == 24);
const _: () = assert!(std::mem::size_of::<Elf32RelaLe>() == 12);
const _: () = assert!(std::mem::size_of::<Elf32RelaBe>() == 12);
const _: () = assert!(std::mem::size_of::<Elf64RelLe>() == 16);
const _: () = assert!(std::mem::size_of::<Elf64RelBe>() == 16);
const _: () = assert!(std::mem::size_of::<Elf32RelLe>() == 8);
const _: () = assert!(std::mem::size_of::<Elf32RelBe>() == 8);
const _: () = assert!(std::mem::size_of::<Sparc64Rela>() == 24);
const _: () = assert!(std::mem::align_of::<Elf64RelaLe>() == 1);
const _: () = assert!(std::mem::align_of::<Elf64RelaBe>() == 1);
const _: () = assert!(std::mem::align_of::<Elf32RelaLe>() == 1);
const _: () = assert!(std::mem::align_of::<Elf32RelaBe>() == 1);
const _: () = assert!(std::mem::align_of::<Elf64RelLe>() == 1);
const _: () = assert!(std::mem::align_of::<Elf64RelBe>() == 1);
const _: () = assert!(std::mem::align_of::<Elf32RelLe>() == 1);
const _: () = assert!(std::mem::align_of::<Elf32RelBe>() == 1);
const _: () = assert!(std::mem::align_of::<Sparc64Rela>() == 1);

/// Relocation records as they are laid out in a file, read as they are
/// used rather than copied out. Input files hold tens of millions of
/// relocations, which most passes go through once.
pub(crate) fn rels_from_bytes<E: Layout>(data: &[u8]) -> &[E::Rel] {
    let size = std::mem::size_of::<E::Rel>();
    assert_eq!(std::mem::align_of::<E::Rel>(), 1);
    assert!(data.len().is_multiple_of(size));
    // SAFETY: RelRecord requires alignment one and every bit pattern to be
    // valid. The resulting slice covers exactly `data`.
    unsafe { std::slice::from_raw_parts(data.as_ptr().cast(), data.len() / size) }
}

/// Mutably views relocation records in their target-dependent file representation.
pub(crate) fn rels_from_bytes_mut<E: Layout>(data: &mut [u8]) -> &mut [E::Rel] {
    let size = std::mem::size_of::<E::Rel>();
    assert_eq!(std::mem::align_of::<E::Rel>(), 1);
    assert!(data.len().is_multiple_of(size));
    // SAFETY: RelRecord requires alignment one and every bit pattern to be
    // valid. `data` is exclusively borrowed for the returned slice.
    unsafe { std::slice::from_raw_parts_mut(data.as_mut_ptr().cast(), data.len() / size) }
}

/// The layout of a file's records — word size and byte order — as a
/// value, for the tables that are read in place by code that isn't
/// specialized for a target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecordLayout {
    pub is_64: bool,
    pub is_little_endian: bool,
}

impl RecordLayout {
    #[inline]
    pub fn of<E: Layout>() -> RecordLayout {
        RecordLayout {
            is_64: E::IS_64,
            is_little_endian: E::Endian::IS_LITTLE,
        }
    }

    #[inline]
    fn u16(self, bytes: &[u8], offset: usize) -> u16 {
        let bytes = bytes[offset..offset + 2].try_into().unwrap();
        if self.is_little_endian {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        }
    }

    #[inline]
    fn u32(self, bytes: &[u8], offset: usize) -> u32 {
        let bytes = bytes[offset..offset + 4].try_into().unwrap();
        if self.is_little_endian {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        }
    }

    #[inline]
    fn u64(self, bytes: &[u8], offset: usize) -> u64 {
        let bytes = bytes[offset..offset + 8].try_into().unwrap();
        if self.is_little_endian {
            u64::from_le_bytes(bytes)
        } else {
            u64::from_be_bytes(bytes)
        }
    }

    #[inline]
    fn word(self, bytes: &[u8], offset: usize) -> u64 {
        if self.is_64 {
            self.u64(bytes, offset)
        } else {
            self.u32(bytes, offset) as u64
        }
    }

    #[inline]
    fn put_u16(self, buf: &mut [u8], offset: usize, value: u16) {
        buf[offset..offset + 2].copy_from_slice(&if self.is_little_endian {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        });
    }

    #[inline]
    fn put_u32(self, buf: &mut [u8], offset: usize, value: u32) {
        buf[offset..offset + 4].copy_from_slice(&if self.is_little_endian {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        });
    }

    #[inline]
    fn put_u64(self, buf: &mut [u8], offset: usize, value: u64) {
        buf[offset..offset + 8].copy_from_slice(&if self.is_little_endian {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        });
    }

    #[inline]
    fn put_word(self, buf: &mut [u8], offset: usize, value: u64) {
        if self.is_64 {
            self.put_u64(buf, offset, value);
        } else {
            self.put_u32(buf, offset, value as u32);
        }
    }

    #[inline]
    pub fn sym_size(self) -> usize {
        if self.is_64 {
            24
        } else {
            16
        }
    }

    #[inline]
    pub fn read_sym(self, bytes: &[u8]) -> SymbolEntry {
        if self.is_64 {
            SymbolEntry {
                st_name: self.u32(bytes, 0),
                type_and_bind: bytes[4],
                other: bytes[5],
                st_shndx: self.u16(bytes, 6),
                st_value: self.u64(bytes, 8),
                st_size: self.u64(bytes, 16),
            }
        } else {
            SymbolEntry {
                st_name: self.u32(bytes, 0),
                st_value: self.u32(bytes, 4) as u64,
                st_size: self.u32(bytes, 8) as u64,
                type_and_bind: bytes[12],
                other: bytes[13],
                st_shndx: self.u16(bytes, 14),
            }
        }
    }

    #[inline]
    pub fn write_sym(self, sym: &SymbolEntry, buf: &mut [u8]) {
        if self.is_64 {
            self.put_u32(buf, 0, sym.st_name);
            buf[4] = sym.type_and_bind;
            buf[5] = sym.other;
            self.put_u16(buf, 6, sym.st_shndx);
            self.put_u64(buf, 8, sym.st_value);
            self.put_u64(buf, 16, sym.st_size);
        } else {
            self.put_u32(buf, 0, sym.st_name);
            self.put_u32(buf, 4, sym.st_value as u32);
            self.put_u32(buf, 8, sym.st_size as u32);
            buf[12] = sym.type_and_bind;
            buf[13] = sym.other;
            self.put_u16(buf, 14, sym.st_shndx);
        }
    }

    #[inline]
    pub fn shdr_size(self) -> usize {
        if self.is_64 {
            64
        } else {
            40
        }
    }

    #[inline]
    pub fn read_shdr(self, bytes: &[u8]) -> SectionHeader {
        let w = if self.is_64 { 8 } else { 4 };
        SectionHeader {
            sh_name: self.u32(bytes, 0),
            sh_type: self.u32(bytes, 4),
            sh_flags: self.word(bytes, 8),
            sh_addr: self.word(bytes, 8 + w),
            sh_offset: self.word(bytes, 8 + 2 * w),
            sh_size: self.word(bytes, 8 + 3 * w),
            sh_link: self.u32(bytes, 8 + 4 * w),
            sh_info: self.u32(bytes, 12 + 4 * w),
            sh_addralign: self.word(bytes, 16 + 4 * w),
            sh_entsize: self.word(bytes, 16 + 5 * w),
        }
    }

    #[inline]
    pub fn write_shdr(self, shdr: &SectionHeader, buf: &mut [u8]) {
        let w = if self.is_64 { 8 } else { 4 };
        self.put_u32(buf, 0, shdr.sh_name);
        self.put_u32(buf, 4, shdr.sh_type);
        self.put_word(buf, 8, shdr.sh_flags);
        self.put_word(buf, 8 + w, shdr.sh_addr);
        self.put_word(buf, 8 + 2 * w, shdr.sh_offset);
        self.put_word(buf, 8 + 3 * w, shdr.sh_size);
        self.put_u32(buf, 8 + 4 * w, shdr.sh_link);
        self.put_u32(buf, 12 + 4 * w, shdr.sh_info);
        self.put_word(buf, 16 + 4 * w, shdr.sh_addralign);
        self.put_word(buf, 16 + 5 * w, shdr.sh_entsize);
    }
}

/// A file's symbol table, read in place from the file; the linker's own
/// files build theirs. Symbols are decoded as they are used.
#[derive(Clone, Debug, Default)]
pub struct SymTable {
    data: std::borrow::Cow<'static, [u8]>,
    layout: RecordLayout,
    len: usize,
}

impl SymTable {
    pub fn in_file(data: &'static [u8], layout: RecordLayout) -> SymTable {
        debug_assert!(data.len().is_multiple_of(layout.sym_size()));
        SymTable {
            data: std::borrow::Cow::Borrowed(data),
            layout,
            len: data.len() / layout.sym_size(),
        }
    }

    /// An empty table the linker fills, in the target's layout.
    pub fn new(layout: RecordLayout) -> SymTable {
        SymTable {
            data: std::borrow::Cow::Owned(Vec::new()),
            layout,
            len: 0,
        }
    }

    pub fn from_records(layout: RecordLayout, syms: &[SymbolEntry]) -> SymTable {
        let mut table = SymTable::new(layout);
        for sym in syms {
            table.push(*sym);
        }
        table
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn at(&self, i: usize) -> SymbolEntry {
        let size = self.layout.sym_size();
        self.layout.read_sym(&self.data[i * size..(i + 1) * size])
    }

    #[inline]
    pub fn get(&self, i: usize) -> Option<SymbolEntry> {
        (i < self.len()).then(|| self.at(i))
    }

    /// Like [`Self::at`] for code specialized for the target, which
    /// knows the layout at compile time; relocation loops read a symbol
    /// per relocation.
    #[inline]
    pub fn at_in<E: Layout>(&self, i: usize) -> SymbolEntry {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        let size = SymbolEntry::size::<E>();
        SymbolEntry::parse::<E>(&self.data[i * size..(i + 1) * size])
    }

    #[inline]
    pub fn get_in<E: Layout>(&self, i: usize) -> Option<SymbolEntry> {
        (i < self.len()).then(|| self.at_in::<E>(i))
    }

    /// Replaces a symbol in a table the linker builds.
    pub fn set_in<E: Layout>(&mut self, i: usize, sym: SymbolEntry) {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        let size = SymbolEntry::size::<E>();
        sym.write::<E>(&mut self.data.to_mut()[i * size..(i + 1) * size]);
    }

    /// The type of symbol `i` alone, for relocation loops that look for
    /// section symbols.
    #[inline]
    pub fn st_type(&self, i: usize) -> u32 {
        let offset = if self.layout.is_64 { 4 } else { 12 };
        u32::from(self.data[i * self.layout.sym_size() + offset] & 0xf)
    }

    /// Like [`Self::st_type`] for code specialized for the target.
    #[inline]
    pub fn st_type_in<E: Layout>(&self, i: usize) -> u32 {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        let offset = if E::IS_64 { 4 } else { 12 };
        u32::from(self.data[i * SymbolEntry::size::<E>() + offset] & 0xf)
    }

    /// Reads the type after checking the symbol index once.
    #[inline(always)]
    pub fn st_type_in_checked<E: Layout>(&self, i: usize) -> Option<u32> {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        if i >= self.len {
            return None;
        }
        let offset = i * SymbolEntry::size::<E>() + if E::IS_64 { 4 } else { 12 };
        // SAFETY: `i < self.len` and every symbol record has this field.
        Some(u32::from(unsafe { *self.data.as_ptr().add(offset) } & 0xf))
    }

    /// The binding of symbol `i` alone.
    #[inline]
    pub fn st_bind_in<E: Layout>(&self, i: usize) -> u32 {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        let offset = if E::IS_64 { 4 } else { 12 };
        u32::from(self.data[i * SymbolEntry::size::<E>() + offset] >> 4)
    }

    /// The section index of symbol `i` alone.
    #[inline]
    pub fn st_shndx(&self, i: usize) -> u16 {
        let offset = if self.layout.is_64 { 6 } else { 14 };
        self.layout
            .u16(&self.data, i * self.layout.sym_size() + offset)
    }

    /// Like [`Self::st_shndx`] for code specialized for the target.
    #[inline]
    pub fn st_shndx_in<E: Layout>(&self, i: usize) -> u16 {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        let offset = if E::IS_64 { 6 } else { 14 };
        E::Endian::read_u16(&self.data[i * SymbolEntry::size::<E>() + offset..])
    }

    /// Reads a section index after the caller has checked `i < self.len()`.
    #[inline(always)]
    pub(crate) unsafe fn st_shndx_in_unchecked<E: Layout>(&self, i: usize) -> u16 {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        debug_assert!(i < self.len);
        let offset = i * SymbolEntry::size::<E>() + if E::IS_64 { 6 } else { 14 };
        // SAFETY: guaranteed by the caller and the fixed symbol layout.
        let raw = unsafe { std::ptr::read_unaligned(self.data.as_ptr().add(offset).cast::<u16>()) };
        if E::Endian::IS_LITTLE {
            u16::from_le(raw)
        } else {
            u16::from_be(raw)
        }
    }

    /// The name offset of symbol `i` alone.
    #[inline]
    pub fn st_name(&self, i: usize) -> u32 {
        self.layout.u32(&self.data, i * self.layout.sym_size())
    }

    /// Like [`Self::st_name`] for code specialized for the target.
    #[inline]
    pub fn st_name_in<E: Layout>(&self, i: usize) -> u32 {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        E::Endian::read_u32(&self.data[i * SymbolEntry::size::<E>()..])
    }

    /// Walks the name offsets without decoding the rest of each symbol.
    #[inline]
    pub fn name_offsets_in<E: Layout>(
        &self,
    ) -> impl DoubleEndedIterator<Item = u32> + ExactSizeIterator + '_ {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        self.data
            .chunks_exact(SymbolEntry::size::<E>())
            .map(E::Endian::read_u32)
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = SymbolEntry> + ExactSizeIterator + '_ {
        let layout = self.layout;
        self.data
            .chunks_exact(layout.sym_size())
            .map(move |bytes| layout.read_sym(bytes))
    }

    /// Appends a symbol to a table the linker builds.
    pub fn push(&mut self, sym: SymbolEntry) {
        let size = self.layout.sym_size();
        let data = self.data.to_mut();
        let start = data.len();
        data.resize(start + size, 0);
        self.layout.write_sym(&sym, &mut data[start..]);
        self.len += 1;
    }
}

/// A file's section headers, read in place, followed by those the linker
/// adds for the sections it makes up (for common symbols).
#[derive(Clone, Debug, Default)]
pub struct ShdrTable<'a> {
    data: &'a [u8],
    layout: RecordLayout,
    /// The number of headers in `data`.
    num_in_file: usize,
    extra: Vec<SectionHeader>,
}

impl<'a> ShdrTable<'a> {
    pub fn in_file(data: &'a [u8], layout: RecordLayout) -> ShdrTable<'a> {
        debug_assert!(data.len().is_multiple_of(layout.shdr_size()));
        ShdrTable {
            data,
            layout,
            num_in_file: data.len() / layout.shdr_size(),
            extra: Vec::new(),
        }
    }

    /// The number of headers in the file.
    #[inline]
    pub fn num_in_file(&self) -> usize {
        self.num_in_file
    }

    /// The name offset of header `i` alone.
    #[inline]
    pub fn sh_name(&self, i: usize) -> u32 {
        if i < self.num_in_file {
            self.layout.u32(self.data, i * self.layout.shdr_size())
        } else {
            self.extra[i - self.num_in_file].sh_name
        }
    }

    /// Like [`Self::sh_name`] for code specialized for the target.
    #[inline]
    pub fn sh_name_in<E: Layout>(&self, i: usize) -> u32 {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        if i < self.num_in_file {
            E::Endian::read_u32(&self.data[i * SectionHeader::size::<E>()..])
        } else {
            self.extra[i - self.num_in_file].sh_name
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.num_in_file() + self.extra.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn at(&self, i: usize) -> SectionHeader {
        let n = self.num_in_file();
        if i < n {
            let size = self.layout.shdr_size();
            self.layout.read_shdr(&self.data[i * size..(i + 1) * size])
        } else {
            self.extra[i - n]
        }
    }

    #[inline]
    pub fn get(&self, i: usize) -> Option<SectionHeader> {
        (i < self.len()).then(|| self.at(i))
    }

    /// Like [`Self::at`] for code specialized for the target, which knows
    /// the layout at compile time.
    #[inline]
    pub fn at_in<E: Layout>(&self, i: usize) -> SectionHeader {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        if i < self.num_in_file {
            let size = SectionHeader::size::<E>();
            SectionHeader::parse::<E>(&self.data[i * size..(i + 1) * size])
        } else {
            self.extra[i - self.num_in_file]
        }
    }

    /// Like [`Self::at_in`], for a caller already iterating within the
    /// table's length. C++ mold walks the same validated section-header span
    /// directly, without checking the index again for every field.
    ///
    /// # Safety
    ///
    /// `i` must be less than [`Self::len`].
    #[inline(always)]
    pub unsafe fn at_in_unchecked<E: Layout>(&self, i: usize) -> SectionHeader {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        debug_assert!(i < self.len());
        if i < self.num_in_file {
            let size = SectionHeader::size::<E>();
            // SAFETY: the caller proved `i`, and `in_file` accepted only a
            // whole number of section-header records.
            let bytes =
                unsafe { std::slice::from_raw_parts(self.data.as_ptr().add(i * size), size) };
            SectionHeader::parse::<E>(bytes)
        } else {
            // SAFETY: the total-length precondition proves this extra index.
            unsafe { *self.extra.get_unchecked(i - self.num_in_file) }
        }
    }

    /// Reads a header from the validated file-backed part of the table.
    ///
    /// # Safety
    ///
    /// `i` must be less than [`Self::num_in_file`].
    #[inline(always)]
    pub unsafe fn file_at_in_unchecked<E: Layout>(&self, i: usize) -> SectionHeader {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        debug_assert!(i < self.num_in_file);
        let size = SectionHeader::size::<E>();
        // SAFETY: the caller proved `i`, and `in_file` accepted only a whole
        // number of section-header records.
        let bytes = unsafe { std::slice::from_raw_parts(self.data.as_ptr().add(i * size), size) };
        SectionHeader::parse::<E>(bytes)
    }

    /// Reads the two fields needed to classify a section without decoding
    /// the rest of its header.
    ///
    /// # Safety
    ///
    /// `i` must be less than [`Self::len`].
    #[inline(always)]
    pub unsafe fn sh_type_and_flags_in_unchecked<E: Layout>(&self, i: usize) -> (u32, u64) {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        debug_assert!(i < self.len());
        if i < self.num_in_file {
            let ptr = unsafe { self.data.as_ptr().add(i * SectionHeader::size::<E>()) };
            // SAFETY: a complete header has a u32 at offset 4 and a target
            // word at offset 8. Unaligned reads match ELF's file layout.
            let sh_type = unsafe { std::ptr::read_unaligned(ptr.add(4).cast::<u32>()) };
            let sh_type = if E::Endian::IS_LITTLE {
                u32::from_le(sh_type)
            } else {
                u32::from_be(sh_type)
            };
            let sh_flags = if E::IS_64 {
                let value = unsafe { std::ptr::read_unaligned(ptr.add(8).cast::<u64>()) };
                if E::Endian::IS_LITTLE {
                    u64::from_le(value)
                } else {
                    u64::from_be(value)
                }
            } else {
                let value = unsafe { std::ptr::read_unaligned(ptr.add(8).cast::<u32>()) };
                if E::Endian::IS_LITTLE {
                    u32::from_le(value) as u64
                } else {
                    u32::from_be(value) as u64
                }
            };
            (sh_type, sh_flags)
        } else {
            // SAFETY: the total-length precondition proves this extra index.
            let shdr = unsafe { self.extra.get_unchecked(i - self.num_in_file) };
            (shdr.sh_type, shdr.sh_flags)
        }
    }

    /// Reads the type and flags from the validated file-backed span.
    ///
    /// # Safety
    ///
    /// `i` must be less than [`Self::num_in_file`].
    #[inline(always)]
    pub unsafe fn file_type_and_flags_in_unchecked<E: Layout>(&self, i: usize) -> (u32, u64) {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        debug_assert!(i < self.num_in_file);
        let ptr = unsafe { self.data.as_ptr().add(i * SectionHeader::size::<E>()) };
        // SAFETY: a complete header has a u32 at offset 4 and a target word at
        // offset 8. Unaligned reads match ELF's file layout.
        let sh_type = unsafe { std::ptr::read_unaligned(ptr.add(4).cast::<u32>()) };
        let sh_type = if E::Endian::IS_LITTLE {
            u32::from_le(sh_type)
        } else {
            u32::from_be(sh_type)
        };
        let sh_flags = if E::IS_64 {
            let value = unsafe { std::ptr::read_unaligned(ptr.add(8).cast::<u64>()) };
            if E::Endian::IS_LITTLE {
                u64::from_le(value)
            } else {
                u64::from_be(value)
            }
        } else {
            let value = unsafe { std::ptr::read_unaligned(ptr.add(8).cast::<u32>()) };
            if E::Endian::IS_LITTLE {
                u32::from_le(value) as u64
            } else {
                u32::from_be(value) as u64
            }
        };
        (sh_type, sh_flags)
    }

    /// Reads the type from the validated file-backed span.
    ///
    /// # Safety
    ///
    /// `i` must be less than [`Self::num_in_file`].
    #[inline(always)]
    pub unsafe fn file_type_in_unchecked<E: Layout>(&self, i: usize) -> u32 {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        debug_assert!(i < self.num_in_file);
        let ptr = unsafe { self.data.as_ptr().add(i * SectionHeader::size::<E>() + 4) };
        // SAFETY: a complete header has a u32 at offset 4. An unaligned read
        // matches ELF's file layout.
        let value = unsafe { std::ptr::read_unaligned(ptr.cast::<u32>()) };
        if E::Endian::IS_LITTLE {
            u32::from_le(value)
        } else {
            u32::from_be(value)
        }
    }

    /// The type of header `i` alone, for loops that decode only the
    /// headers they are after.
    #[inline]
    pub fn sh_type(&self, i: usize) -> u32 {
        let n = self.num_in_file();
        if i < n {
            self.layout.u32(self.data, i * self.layout.shdr_size() + 4)
        } else {
            self.extra[i - n].sh_type
        }
    }

    /// Like [`Self::sh_type`] for code specialized for the target.
    #[inline]
    pub fn sh_type_in<E: Layout>(&self, i: usize) -> u32 {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        if i < self.num_in_file {
            E::Endian::read_u32(&self.data[i * SectionHeader::size::<E>() + 4..])
        } else {
            self.extra[i - self.num_in_file].sh_type
        }
    }

    /// The flags of header `i` alone.
    #[inline]
    pub fn sh_flags(&self, i: usize) -> u64 {
        let n = self.num_in_file();
        if i < n {
            self.layout.word(self.data, i * self.layout.shdr_size() + 8)
        } else {
            self.extra[i - n].sh_flags
        }
    }

    /// Like [`Self::sh_flags`] for code specialized for the target.
    #[inline]
    pub fn sh_flags_in<E: Layout>(&self, i: usize) -> u64 {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        if i < self.num_in_file {
            let bytes = &self.data[i * SectionHeader::size::<E>() + 8..];
            if E::IS_64 {
                E::Endian::read_u64(bytes)
            } else {
                E::Endian::read_u32(bytes) as u64
            }
        } else {
            self.extra[i - self.num_in_file].sh_flags
        }
    }

    /// Reads section flags after checking the section index once.
    #[inline(always)]
    pub fn sh_flags_in_checked<E: Layout>(&self, i: usize) -> Option<u64> {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        if i >= self.len() {
            return None;
        }
        if i < self.num_in_file {
            let offset = i * SectionHeader::size::<E>() + 8;
            if E::IS_64 {
                // SAFETY: `i` names a complete 64-bit section header.
                let raw = unsafe {
                    std::ptr::read_unaligned(self.data.as_ptr().add(offset).cast::<u64>())
                };
                Some(if E::Endian::IS_LITTLE {
                    u64::from_le(raw)
                } else {
                    u64::from_be(raw)
                })
            } else {
                // SAFETY: `i` names a complete 32-bit section header.
                let raw = unsafe {
                    std::ptr::read_unaligned(self.data.as_ptr().add(offset).cast::<u32>())
                };
                Some(if E::Endian::IS_LITTLE {
                    u32::from_le(raw) as u64
                } else {
                    u32::from_be(raw) as u64
                })
            }
        } else {
            // SAFETY: the total-length check above proves this extra index.
            Some(unsafe { self.extra.get_unchecked(i - self.num_in_file).sh_flags })
        }
    }

    /// The file offset and size of section `i` alone.
    #[inline]
    pub fn sh_offset_and_size(&self, i: usize) -> (u64, u64) {
        let n = self.num_in_file();
        if i < n {
            let w = if self.layout.is_64 { 8 } else { 4 };
            let base = i * self.layout.shdr_size();
            (
                self.layout.word(self.data, base + 8 + 2 * w),
                self.layout.word(self.data, base + 8 + 3 * w),
            )
        } else {
            let shdr = &self.extra[i - n];
            (shdr.sh_offset, shdr.sh_size)
        }
    }

    /// Like [`Self::sh_offset_and_size`] for target-specialized code.
    #[inline]
    pub fn sh_offset_and_size_in<E: Layout>(&self, i: usize) -> (u64, u64) {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        if i < self.num_in_file {
            let base = i * SectionHeader::size::<E>();
            if E::IS_64 {
                (
                    E::Endian::read_u64(&self.data[base + 24..]),
                    E::Endian::read_u64(&self.data[base + 32..]),
                )
            } else {
                (
                    E::Endian::read_u32(&self.data[base + 16..]) as u64,
                    E::Endian::read_u32(&self.data[base + 20..]) as u64,
                )
            }
        } else {
            let shdr = &self.extra[i - self.num_in_file];
            (shdr.sh_offset, shdr.sh_size)
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = SectionHeader> + '_ {
        (0..self.len()).map(|i| self.at(i))
    }

    pub fn push(&mut self, shdr: SectionHeader) {
        self.extra.push(shdr);
    }
}

/// An entry of the `.dynamic` section.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ElfDyn<E: Layout> {
    pub d_tag: E::Word,
    pub d_val: E::Word,
}

// SAFETY: both fields are byte-backed integers, and `repr(C)` does not insert
// padding between fields with alignment one.
unsafe impl<E: Layout> FileRecord for ElfDyn<E> {}

const _: () = assert!(std::mem::size_of::<ElfDyn<Elf32Le>>() == 8);
const _: () = assert!(std::mem::size_of::<ElfDyn<Elf64Le>>() == 16);
const _: () = assert!(std::mem::align_of::<ElfDyn<Elf32Le>>() == 1);
const _: () = assert!(std::mem::align_of::<ElfDyn<Elf64Le>>() == 1);

/// The header of a compressed section.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Elf64Chdr<E: Endian> {
    pub ch_type: U32<E>,
    pub ch_reserved: U32<E>,
    pub ch_size: U64<E>,
    pub ch_addralign: U64<E>,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Elf32Chdr<E: Endian> {
    pub ch_type: U32<E>,
    pub ch_size: U32<E>,
    pub ch_addralign: U32<E>,
}

// SAFETY: all fields are byte-backed integers, and `repr(C)` does not insert
// padding between fields with alignment one.
unsafe impl<E: Endian> FileRecord for Elf64Chdr<E> {}
// SAFETY: see the Elf64 implementation.
unsafe impl<E: Endian> FileRecord for Elf32Chdr<E> {}

const _: () = assert!(std::mem::size_of::<Elf32Chdr<LittleEndian>>() == 12);
const _: () = assert!(std::mem::size_of::<Elf64Chdr<LittleEndian>>() == 24);
const _: () = assert!(std::mem::align_of::<Elf32Chdr<LittleEndian>>() == 1);
const _: () = assert!(std::mem::align_of::<Elf64Chdr<LittleEndian>>() == 1);

/// The two physical compression-header layouts convert as complete records.
pub trait CompressionHeaderRecord: FileRecord {
    type Endian: Endian;

    fn decode(&self) -> CompressionHeader;
    fn encode(chdr: &CompressionHeader) -> Self;
}

impl<E: Endian> CompressionHeaderRecord for Elf64Chdr<E> {
    type Endian = E;

    fn decode(&self) -> CompressionHeader {
        CompressionHeader {
            ch_type: self.ch_type.get(),
            ch_size: self.ch_size.get(),
            ch_addralign: self.ch_addralign.get(),
        }
    }

    fn encode(chdr: &CompressionHeader) -> Self {
        Elf64Chdr {
            ch_type: U32::new(chdr.ch_type),
            ch_reserved: U32::default(),
            ch_size: U64::new(chdr.ch_size),
            ch_addralign: U64::new(chdr.ch_addralign),
        }
    }
}

impl<E: Endian> CompressionHeaderRecord for Elf32Chdr<E> {
    type Endian = E;

    fn decode(&self) -> CompressionHeader {
        CompressionHeader {
            ch_type: self.ch_type.get(),
            ch_size: u64::from(self.ch_size.get()),
            ch_addralign: u64::from(self.ch_addralign.get()),
        }
    }

    fn encode(chdr: &CompressionHeader) -> Self {
        Elf32Chdr {
            ch_type: U32::new(chdr.ch_type),
            ch_size: U32::new(chdr.ch_size as u32),
            ch_addralign: U32::new(chdr.ch_addralign as u32),
        }
    }
}

pub type ElfChdr<E> = <E as Layout>::Chdr;

/// A compression header decoded for compression bookkeeping.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompressionHeader {
    pub ch_type: u32,
    pub ch_size: u64,
    pub ch_addralign: u64,
}

impl Record for CompressionHeader {
    fn size<E: Layout>() -> usize {
        E::Chdr::size()
    }

    fn parse<E: Layout>(bytes: &[u8]) -> Self {
        E::Chdr::parse(bytes).decode()
    }

    fn write<E: Layout>(&self, buf: &mut [u8]) {
        E::Chdr::encode(self).write(buf);
    }
}

/// A note header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ElfNhdr<E: Layout> {
    pub n_namesz: U32<E::Endian>,
    pub n_descsz: U32<E::Endian>,
    pub n_type: U32<E::Endian>,
}

// SAFETY: all fields are byte-backed integers, and `repr(C)` does not insert
// padding between fields with alignment one.
unsafe impl<E: Layout> FileRecord for ElfNhdr<E> {}

const _: () = assert!(std::mem::size_of::<ElfNhdr<Elf32Le>>() == 12);
const _: () = assert!(std::mem::align_of::<ElfNhdr<Elf32Le>>() == 1);

/// A `.gnu.version_r` file entry.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ElfVerneed<E: Layout> {
    pub vn_version: U16<E::Endian>,
    pub vn_cnt: U16<E::Endian>,
    pub vn_file: U32<E::Endian>,
    pub vn_aux: U32<E::Endian>,
    pub vn_next: U32<E::Endian>,
}

// SAFETY: all fields are byte-backed integers, and `repr(C)` does not insert
// padding between fields with alignment one.
unsafe impl<E: Layout> FileRecord for ElfVerneed<E> {}

const _: () = assert!(std::mem::size_of::<ElfVerneed<Elf32Le>>() == 16);

/// A `.gnu.version_r` version entry.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ElfVernaux<E: Layout> {
    pub vna_hash: U32<E::Endian>,
    pub vna_flags: U16<E::Endian>,
    pub vna_other: U16<E::Endian>,
    pub vna_name: U32<E::Endian>,
    pub vna_next: U32<E::Endian>,
}

// SAFETY: all fields are byte-backed integers, and `repr(C)` does not insert
// padding between fields with alignment one.
unsafe impl<E: Layout> FileRecord for ElfVernaux<E> {}

const _: () = assert!(std::mem::size_of::<ElfVernaux<Elf32Le>>() == 16);

/// A `.gnu.version_d` definition entry.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ElfVerdef<E: Layout> {
    pub vd_version: U16<E::Endian>,
    pub vd_flags: U16<E::Endian>,
    pub vd_ndx: U16<E::Endian>,
    pub vd_cnt: U16<E::Endian>,
    pub vd_hash: U32<E::Endian>,
    pub vd_aux: U32<E::Endian>,
    pub vd_next: U32<E::Endian>,
}

// SAFETY: all fields are byte-backed integers, and `repr(C)` does not insert
// padding between fields with alignment one.
unsafe impl<E: Layout> FileRecord for ElfVerdef<E> {}

const _: () = assert!(std::mem::size_of::<ElfVerdef<Elf32Le>>() == 20);

/// A `.gnu.version_d` name entry.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ElfVerdaux<E: Layout> {
    pub vda_name: U32<E::Endian>,
    pub vda_next: U32<E::Endian>,
}

// SAFETY: all fields are byte-backed integers, and `repr(C)` does not insert
// padding between fields with alignment one.
unsafe impl<E: Layout> FileRecord for ElfVerdaux<E> {}

const _: () = assert!(std::mem::size_of::<ElfVerdaux<Elf32Le>>() == 8);

/// SFrame is a simple unwind information format used as a lightweight
/// alternative to .eh_frame. A .sframe section consists of a header, an
/// array of Function Descriptor Entries (FDEs) sorted by PC, and a blob
/// of Frame Row Entries (FREs). mold understands SFrame Version 3.
///
/// https://sourceware.org/binutils/docs/sframe-spec.html
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SFrameHeader<E: Layout> {
    pub magic: U16<E::Endian>,
    pub version: u8,
    pub flags: u8,
    pub abi_arch: u8,
    pub cfa_fixed_fp_offset: i8,
    pub cfa_fixed_ra_offset: i8,
    pub auxhdr_len: u8,
    pub num_fdes: U32<E::Endian>,
    pub num_fres: U32<E::Endian>,
    pub fre_len: U32<E::Endian>,
    pub fdeoff: U32<E::Endian>,
    pub freoff: U32<E::Endian>,
}

// SAFETY: all fields are byte-backed integers or bytes, and `repr(C)` does
// not insert padding between fields with alignment one.
unsafe impl<E: Layout> FileRecord for SFrameHeader<E> {}

const _: () = assert!(std::mem::size_of::<SFrameHeader<Elf32Le>>() == 28);
const _: () = assert!(std::mem::align_of::<SFrameHeader<Elf32Le>>() == 1);

pub const SFRAME_MAGIC: u16 = 0xdee2;
pub const SFRAME_F_FDE_SORTED: u8 = 0x1;
pub const SFRAME_F_FRAME_POINTER: u8 = 0x2;
pub const SFRAME_F_FDE_FUNC_START_PCREL: u8 = 0x4;

pub const SFRAME_ABI_AARCH64_ENDIAN_BIG: u8 = 1;
pub const SFRAME_ABI_AARCH64_ENDIAN_LITTLE: u8 = 2;
pub const SFRAME_ABI_AMD64_ENDIAN_LITTLE: u8 = 3;
pub const SFRAME_ABI_S390X_ENDIAN_BIG: u8 = 4;

/// The index part of an SFrame Version 3 FDE. The func_start_offset field
/// is PC-relative (relative to its own address) when the section flag
/// SFRAME_F_FDE_FUNC_START_PCREL is set, which is how mold always emits it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SFrameFdeIdx<E: Layout> {
    pub func_start_offset: I64<E::Endian>,
    pub func_size: U32<E::Endian>,
    pub func_start_fre_off: U32<E::Endian>,
}

// SAFETY: all fields are byte-backed integers, and `repr(C)` does not insert
// padding between fields with alignment one.
unsafe impl<E: Layout> FileRecord for SFrameFdeIdx<E> {}

const _: () = assert!(std::mem::size_of::<SFrameFdeIdx<Elf32Le>>() == 16);
const _: () = assert!(std::mem::align_of::<SFrameFdeIdx<Elf32Le>>() == 1);

/// Returns a symbol type's name for diagnostics.
pub fn stt_to_string(st_type: u32) -> String {
    match st_type {
        STT_NOTYPE => "STT_NOTYPE".into(),
        STT_OBJECT => "STT_OBJECT".into(),
        STT_FUNC => "STT_FUNC".into(),
        STT_SECTION => "STT_SECTION".into(),
        STT_FILE => "STT_FILE".into(),
        STT_COMMON => "STT_COMMON".into(),
        STT_TLS => "STT_TLS".into(),
        STT_GNU_IFUNC => "STT_GNU_IFUNC".into(),
        STT_SPARC_REGISTER => "STT_SPARC_REGISTER".into(),
        _ => format!("unknown st_type ({st_type})"),
    }
}
