//! ELF file format definitions.
//!
//! mold is always a cross linker, so file integers must use the target byte
//! order rather than the host byte order. ELF records in archive members may
//! also be unaligned because archives align members to only two bytes. Creating
//! ordinary integer references into such data would be invalid.
//!
//! The byte-backed integer fields below handle both target endianness and
//! unaligned access.
//!
//! Records whose ELF32 and ELF64 forms have the same field order are generic
//! over the target word type. Symbols, program headers and compression
//! headers have genuinely different layouts and convert as complete records
//! when target-independent bookkeeping needs them. Thus the big tables —
//! section headers, symbols and relocations — stay in the input files rather
//! than being copied. [`Arch`](crate::arch::Arch) selects the layout at
//! compile time.

mod consts;

use std::fmt;
use std::marker::PhantomData;

pub use consts::*;

use crate::arch::{Arch, I386, X86_64};

// ELF types
/// The on-disk layout of an ELF file: word size, byte order and
/// relocation record format. Targets implement this through [`Arch`].
pub trait Layout: Copy + Default + Send + Sync + 'static {
    type Endian: Endian;
    type Word: ElfWord<Endian = Self::Endian>;
    type Sym: SymbolRecord<Endian = Self::Endian, Word = Self::Word>;
    type Phdr: PhdrRecord<Endian = Self::Endian, Word = Self::Word>;
    type Chdr: ChdrRecord<Endian = Self::Endian, Word = Self::Word>;
    type Rel: RelRecord<Endian = Self::Endian>;
    const IS_64: bool = std::mem::size_of::<Self::Word>() == 8;
    const IS_RELA: bool = <Self::Rel as RelRecord>::IS_RELA;
    const WORD_SIZE: usize = if Self::IS_64 { 8 } else { 4 };
}

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

/// Views one record directly in its file representation.
pub(crate) fn record_from_bytes<R: FileRecord>(data: &[u8]) -> &R {
    assert!(data.len() >= R::size());
    debug_assert_eq!(std::mem::align_of::<R>(), 1);
    // SAFETY: FileRecord requires alignment one and every bit pattern to be
    // valid. The length check proves that one complete record is present.
    unsafe { &*data.as_ptr().cast() }
}

/// Views records directly in their file representation.
pub(crate) fn records_from_bytes<R: FileRecord>(data: &[u8]) -> &[R] {
    let size = R::size();
    assert_ne!(size, 0);
    assert!(data.len().is_multiple_of(size));
    debug_assert_eq!(std::mem::align_of::<R>(), 1);
    // SAFETY: FileRecord requires alignment one and every bit pattern to be
    // valid. The resulting slice covers exactly `data`.
    unsafe { std::slice::from_raw_parts(data.as_ptr().cast(), data.len() / size) }
}

/// The ELF file header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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

const _: () = assert!(std::mem::size_of::<ElfEhdr<I386>>() == 52);
const _: () = assert!(std::mem::size_of::<ElfEhdr<X86_64>>() == 64);
const _: () = assert!(std::mem::align_of::<ElfEhdr<I386>>() == 1);
const _: () = assert!(std::mem::align_of::<ElfEhdr<X86_64>>() == 1);

/// A section header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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

const _: () = assert!(std::mem::size_of::<ElfShdr<I386>>() == 40);
const _: () = assert!(std::mem::size_of::<ElfShdr<X86_64>>() == 64);
const _: () = assert!(std::mem::align_of::<ElfShdr<I386>>() == 1);
const _: () = assert!(std::mem::align_of::<ElfShdr<X86_64>>() == 1);

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

/// The common interface of the two physical program-header layouts.
pub trait PhdrRecord: FileRecord + fmt::Debug {
    type Endian: Endian;
    type Word: ElfWord<Endian = Self::Endian>;

    fn p_type(&self) -> &U32<Self::Endian>;
    fn p_type_mut(&mut self) -> &mut U32<Self::Endian>;
    fn p_flags(&self) -> &U32<Self::Endian>;
    fn p_flags_mut(&mut self) -> &mut U32<Self::Endian>;
    fn p_offset(&self) -> &Self::Word;
    fn p_offset_mut(&mut self) -> &mut Self::Word;
    fn p_vaddr(&self) -> &Self::Word;
    fn p_vaddr_mut(&mut self) -> &mut Self::Word;
    fn p_paddr(&self) -> &Self::Word;
    fn p_paddr_mut(&mut self) -> &mut Self::Word;
    fn p_filesz(&self) -> &Self::Word;
    fn p_filesz_mut(&mut self) -> &mut Self::Word;
    fn p_memsz(&self) -> &Self::Word;
    fn p_memsz_mut(&mut self) -> &mut Self::Word;
    fn p_align(&self) -> &Self::Word;
    fn p_align_mut(&mut self) -> &mut Self::Word;
}

#[rustfmt::skip]
impl<E: Endian> PhdrRecord for Elf64Phdr<E> {
    type Endian = E;
    type Word = U64<E>;

    fn p_type(&self) -> &U32<E> { &self.p_type }
    fn p_type_mut(&mut self) -> &mut U32<E> { &mut self.p_type }
    fn p_flags(&self) -> &U32<E> { &self.p_flags }
    fn p_flags_mut(&mut self) -> &mut U32<E> { &mut self.p_flags }
    fn p_offset(&self) -> &U64<E> { &self.p_offset }
    fn p_offset_mut(&mut self) -> &mut U64<E> { &mut self.p_offset }
    fn p_vaddr(&self) -> &U64<E> { &self.p_vaddr }
    fn p_vaddr_mut(&mut self) -> &mut U64<E> { &mut self.p_vaddr }
    fn p_paddr(&self) -> &U64<E> { &self.p_paddr }
    fn p_paddr_mut(&mut self) -> &mut U64<E> { &mut self.p_paddr }
    fn p_filesz(&self) -> &U64<E> { &self.p_filesz }
    fn p_filesz_mut(&mut self) -> &mut U64<E> { &mut self.p_filesz }
    fn p_memsz(&self) -> &U64<E> { &self.p_memsz }
    fn p_memsz_mut(&mut self) -> &mut U64<E> { &mut self.p_memsz }
    fn p_align(&self) -> &U64<E> { &self.p_align }
    fn p_align_mut(&mut self) -> &mut U64<E> { &mut self.p_align }
}

#[rustfmt::skip]
impl<E: Endian> PhdrRecord for Elf32Phdr<E> {
    type Endian = E;
    type Word = U32<E>;

    fn p_type(&self) -> &U32<E> { &self.p_type }
    fn p_type_mut(&mut self) -> &mut U32<E> { &mut self.p_type }
    fn p_flags(&self) -> &U32<E> { &self.p_flags }
    fn p_flags_mut(&mut self) -> &mut U32<E> { &mut self.p_flags }
    fn p_offset(&self) -> &U32<E> { &self.p_offset }
    fn p_offset_mut(&mut self) -> &mut U32<E> { &mut self.p_offset }
    fn p_vaddr(&self) -> &U32<E> { &self.p_vaddr }
    fn p_vaddr_mut(&mut self) -> &mut U32<E> { &mut self.p_vaddr }
    fn p_paddr(&self) -> &U32<E> { &self.p_paddr }
    fn p_paddr_mut(&mut self) -> &mut U32<E> { &mut self.p_paddr }
    fn p_filesz(&self) -> &U32<E> { &self.p_filesz }
    fn p_filesz_mut(&mut self) -> &mut U32<E> { &mut self.p_filesz }
    fn p_memsz(&self) -> &U32<E> { &self.p_memsz }
    fn p_memsz_mut(&mut self) -> &mut U32<E> { &mut self.p_memsz }
    fn p_align(&self) -> &U32<E> { &self.p_align }
    fn p_align_mut(&mut self) -> &mut U32<E> { &mut self.p_align }
}

pub type ElfPhdr<E> = <E as Layout>::Phdr;

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

/// The common interface of the two physical symbol layouts.
pub trait SymbolRecord: FileRecord + fmt::Debug {
    type Endian: Endian;
    type Word: ElfWord<Endian = Self::Endian>;

    fn st_name(&self) -> &U32<Self::Endian>;
    fn st_name_mut(&mut self) -> &mut U32<Self::Endian>;
    fn st_value(&self) -> &Self::Word;
    fn st_value_mut(&mut self) -> &mut Self::Word;
    fn st_size(&self) -> &Self::Word;
    fn st_size_mut(&mut self) -> &mut Self::Word;
    fn st_shndx(&self) -> &U16<Self::Endian>;
    fn st_shndx_mut(&mut self) -> &mut U16<Self::Endian>;
    fn type_and_bind(&self) -> u8;
    fn type_and_bind_mut(&mut self) -> &mut u8;
    fn other(&self) -> u8;
    fn other_mut(&mut self) -> &mut u8;

    #[inline]
    fn st_type(&self) -> u32 {
        u32::from(self.type_and_bind() & 0xf)
    }

    #[inline]
    fn st_bind(&self) -> u32 {
        u32::from(self.type_and_bind() >> 4)
    }

    #[inline]
    fn st_visibility(&self) -> u32 {
        u32::from(self.other() & 3)
    }

    #[inline]
    fn set_type(&mut self, ty: u32) {
        *self.type_and_bind_mut() = (self.type_and_bind() & 0xf0) | (ty as u8 & 0xf);
    }

    #[inline]
    fn set_bind(&mut self, bind: u32) {
        *self.type_and_bind_mut() = (self.type_and_bind() & 0x0f) | ((bind as u8) << 4);
    }

    #[inline]
    fn set_visibility(&mut self, visibility: u32) {
        *self.other_mut() = (self.other() & !3) | (visibility as u8 & 3);
    }

    #[inline]
    fn is_undef(&self) -> bool {
        self.st_shndx().get() as u32 == SHN_UNDEF
    }

    #[inline]
    fn is_abs(&self) -> bool {
        self.st_shndx().get() as u32 == SHN_ABS
    }

    #[inline]
    fn is_common(&self) -> bool {
        self.st_shndx().get() as u32 == SHN_COMMON
    }

    #[inline]
    fn is_weak(&self) -> bool {
        self.st_bind() == STB_WEAK
    }

    #[inline]
    fn is_undef_weak(&self) -> bool {
        self.is_undef() && self.is_weak()
    }

    #[inline]
    fn arm64_variant_pcs(&self) -> bool {
        self.other() & 0x80 != 0
    }

    #[inline]
    fn set_arm64_variant_pcs(&mut self, value: bool) {
        *self.other_mut() = (self.other() & !0x80) | if value { 0x80 } else { 0 };
    }

    #[inline]
    fn riscv_variant_cc(&self) -> bool {
        self.other() & 0x80 != 0
    }

    #[inline]
    fn set_riscv_variant_cc(&mut self, value: bool) {
        *self.other_mut() = (self.other() & !0x80) | if value { 0x80 } else { 0 };
    }

    #[inline]
    fn ppc64_local_entry(&self) -> u8 {
        self.other() >> 5
    }

    #[inline]
    fn set_ppc64_local_entry(&mut self, value: u8) {
        *self.other_mut() = (self.other() & 0x1f) | ((value & 7) << 5);
    }

    #[inline]
    fn ppc64_preserves_r2(&self) -> bool {
        self.ppc64_local_entry() != 1
    }

    #[inline]
    fn ppc64_uses_toc(&self) -> bool {
        self.ppc64_local_entry() > 1
    }
}

#[rustfmt::skip]
impl<E: Endian> SymbolRecord for Elf64Sym<E> {
    type Endian = E;
    type Word = U64<E>;

    fn st_name(&self) -> &U32<E> { &self.st_name }
    fn st_name_mut(&mut self) -> &mut U32<E> { &mut self.st_name }
    fn st_value(&self) -> &U64<E> { &self.st_value }
    fn st_value_mut(&mut self) -> &mut U64<E> { &mut self.st_value }
    fn st_size(&self) -> &U64<E> { &self.st_size }
    fn st_size_mut(&mut self) -> &mut U64<E> { &mut self.st_size }
    fn st_shndx(&self) -> &U16<E> { &self.st_shndx }
    fn st_shndx_mut(&mut self) -> &mut U16<E> { &mut self.st_shndx }
    fn type_and_bind(&self) -> u8 { self.type_and_bind }
    fn type_and_bind_mut(&mut self) -> &mut u8 { &mut self.type_and_bind }
    fn other(&self) -> u8 { self.other }
    fn other_mut(&mut self) -> &mut u8 { &mut self.other }
}

#[rustfmt::skip]
impl<E: Endian> SymbolRecord for Elf32Sym<E> {
    type Endian = E;
    type Word = U32<E>;

    fn st_name(&self) -> &U32<E> { &self.st_name }
    fn st_name_mut(&mut self) -> &mut U32<E> { &mut self.st_name }
    fn st_value(&self) -> &U32<E> { &self.st_value }
    fn st_value_mut(&mut self) -> &mut U32<E> { &mut self.st_value }
    fn st_size(&self) -> &U32<E> { &self.st_size }
    fn st_size_mut(&mut self) -> &mut U32<E> { &mut self.st_size }
    fn st_shndx(&self) -> &U16<E> { &self.st_shndx }
    fn st_shndx_mut(&mut self) -> &mut U16<E> { &mut self.st_shndx }
    fn type_and_bind(&self) -> u8 { self.type_and_bind }
    fn type_and_bind_mut(&mut self) -> &mut u8 { &mut self.type_and_bind }
    fn other(&self) -> u8 { self.other }
    fn other_mut(&mut self) -> &mut u8 { &mut self.other }
}

pub type ElfSym<E> = <E as Layout>::Sym;

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
pub unsafe trait ElfWord:
    Clone + Copy + fmt::Debug + Default + Send + Sync + 'static
{
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
    const IS_RELA: bool;

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

// Depending on the target, ElfRel may or may not contain an r_addend member.
// A relocation record containing r_addend is called RELA; one without it is
// called REL.
//
// For REL records, applying a relocation adds the computed value to the addend
// stored in the section contents. For RELA records, it writes the value
// computed from the explicit addend.
//
// To keep target-independent code uniform, RelRecord::new always accepts an
// addend. REL implementations ignore it.

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
            const IS_RELA: bool = true;

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
            const IS_RELA: bool = false;

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

const _: () = assert!(std::mem::size_of::<ElfDyn<I386>>() == 8);
const _: () = assert!(std::mem::size_of::<ElfDyn<X86_64>>() == 16);
const _: () = assert!(std::mem::align_of::<ElfDyn<I386>>() == 1);
const _: () = assert!(std::mem::align_of::<ElfDyn<X86_64>>() == 1);

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

/// The common interface of the two physical compression-header layouts.
pub trait ChdrRecord: FileRecord + fmt::Debug {
    type Endian: Endian;
    type Word: ElfWord<Endian = Self::Endian>;

    fn ch_type(&self) -> &U32<Self::Endian>;
    fn ch_type_mut(&mut self) -> &mut U32<Self::Endian>;
    fn ch_size(&self) -> &Self::Word;
    fn ch_size_mut(&mut self) -> &mut Self::Word;
    fn ch_addralign(&self) -> &Self::Word;
    fn ch_addralign_mut(&mut self) -> &mut Self::Word;
}

#[rustfmt::skip]
impl<E: Endian> ChdrRecord for Elf64Chdr<E> {
    type Endian = E;
    type Word = U64<E>;

    fn ch_type(&self) -> &U32<E> { &self.ch_type }
    fn ch_type_mut(&mut self) -> &mut U32<E> { &mut self.ch_type }
    fn ch_size(&self) -> &U64<E> { &self.ch_size }
    fn ch_size_mut(&mut self) -> &mut U64<E> { &mut self.ch_size }
    fn ch_addralign(&self) -> &U64<E> { &self.ch_addralign }
    fn ch_addralign_mut(&mut self) -> &mut U64<E> { &mut self.ch_addralign }
}

#[rustfmt::skip]
impl<E: Endian> ChdrRecord for Elf32Chdr<E> {
    type Endian = E;
    type Word = U32<E>;

    fn ch_type(&self) -> &U32<E> { &self.ch_type }
    fn ch_type_mut(&mut self) -> &mut U32<E> { &mut self.ch_type }
    fn ch_size(&self) -> &U32<E> { &self.ch_size }
    fn ch_size_mut(&mut self) -> &mut U32<E> { &mut self.ch_size }
    fn ch_addralign(&self) -> &U32<E> { &self.ch_addralign }
    fn ch_addralign_mut(&mut self) -> &mut U32<E> { &mut self.ch_addralign }
}

pub type ElfChdr<E> = <E as Layout>::Chdr;

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

const _: () = assert!(std::mem::size_of::<ElfNhdr<I386>>() == 12);
const _: () = assert!(std::mem::align_of::<ElfNhdr<I386>>() == 1);

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

const _: () = assert!(std::mem::size_of::<ElfVerneed<I386>>() == 16);

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

const _: () = assert!(std::mem::size_of::<ElfVernaux<I386>>() == 16);

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

const _: () = assert!(std::mem::size_of::<ElfVerdef<I386>>() == 20);

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

const _: () = assert!(std::mem::size_of::<ElfVerdaux<I386>>() == 8);

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

const _: () = assert!(std::mem::size_of::<SFrameHeader<I386>>() == 28);
const _: () = assert!(std::mem::align_of::<SFrameHeader<I386>>() == 1);

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

const _: () = assert!(std::mem::size_of::<SFrameFdeIdx<I386>>() == 16);
const _: () = assert!(std::mem::align_of::<SFrameFdeIdx<I386>>() == 1);
