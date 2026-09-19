//! ELF file format definitions.
//!
//! mold is always a cross linker, so file integers must use the target byte
//! order rather than the host byte order. ELF records in archive members may
//! also be unaligned because archives align members to only two bytes. Creating
//! ordinary integer references into such data would be invalid.
//!
//! Integer fields are the byte-backed [`U32`], [`U64`] and related types,
//! which take the target [`Layout`] and read and write in its byte order
//! at any alignment.
//!
//! Records whose ELF32 and ELF64 forms have the same field order are generic
//! over the target word type. Symbols, program headers and compression
//! headers have genuinely different layouts, so they exist in two forms
//! behind a common trait, and [`Layout`] names the form a target uses. Thus
//! the big tables — section headers, symbols and relocations — stay in the
//! input files rather than being copied. [`Arch`](crate::arch::Arch) selects
//! the layout at compile time.

use std::fmt;
use std::marker::PhantomData;

pub use crate::elf_consts::*;

use crate::arch::{Arch, I386, Sparc64, X86_64};
use crate::util::endian::*;

// ELF types
/// The on-disk layout of an ELF file: word size, byte order and
/// relocation record format. Targets implement this through [`Arch`].
pub trait Layout: Copy + Default + fmt::Debug + Send + Sync + 'static {
    const IS_LITTLE: bool;
    type Word: ElfWord;
    type Sym: SymbolRecord;
    type Phdr: PhdrRecord;
    type Chdr: ChdrRecord;
    type Rel: RelRecord;
    const IS_64: bool = std::mem::size_of::<Self::Word>() == 8;
    const IS_RELA: bool = <Self::Rel as RelRecord>::IS_RELA;
    const WORD_SIZE: usize = if Self::IS_64 { 8 } else { 4 };

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

/// An integer stored in the target's byte order. The type carries the
/// layout so that a record field needs no accessor of its own.
macro_rules! endian_integer {
    ($name:ident, $int:ty, $size:expr, $read:ident, $write:ident) => {
        #[repr(transparent)]
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub struct $name<E: Layout> {
            bytes: [u8; $size],
            layout: PhantomData<E>,
        }

        impl<E: Layout> $name<E> {
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
endian_integer!(I32, i32, 4, read_i32, write_i32);
endian_integer!(I64, i64, 8, read_i64, write_i64);

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct U24<E: Layout> {
    bytes: [u8; 3],
    layout: PhantomData<E>,
}

impl<E: Layout> U24<E> {
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
        let bytes = if E::IS_LITTLE { value.to_le_bytes() } else { value.to_be_bytes() };
        if E::IS_LITTLE {
            self.bytes.copy_from_slice(&bytes[..3]);
        } else {
            self.bytes.copy_from_slice(&bytes[1..]);
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

/// Mutably views records directly in their file representation.
pub(crate) fn records_from_bytes_mut<R: FileRecord>(data: &mut [u8]) -> &mut [R] {
    let size = R::size();
    assert_ne!(size, 0);
    assert!(data.len().is_multiple_of(size));
    debug_assert_eq!(std::mem::align_of::<R>(), 1);
    // SAFETY: FileRecord requires alignment one and every bit pattern to be
    // valid. `data` is exclusively borrowed for the returned slice.
    unsafe { std::slice::from_raw_parts_mut(data.as_mut_ptr().cast(), data.len() / size) }
}

/// The ELF file header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ElfEhdr<E: Layout> {
    pub e_ident: [u8; 16],
    pub e_type: U16<E>,
    pub e_machine: U16<E>,
    pub e_version: U32<E>,
    pub e_entry: E::Word,
    pub e_phoff: E::Word,
    pub e_shoff: E::Word,
    pub e_flags: U32<E>,
    pub e_ehsize: U16<E>,
    pub e_phentsize: U16<E>,
    pub e_phnum: U16<E>,
    pub e_shentsize: U16<E>,
    pub e_shnum: U16<E>,
    pub e_shstrndx: U16<E>,
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
    pub sh_name: U32<E>,
    pub sh_type: U32<E>,
    pub sh_flags: E::Word,
    pub sh_addr: E::Word,
    pub sh_offset: E::Word,
    pub sh_size: E::Word,
    pub sh_link: U32<E>,
    pub sh_info: U32<E>,
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
pub struct Elf64Phdr<E: Layout> {
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
pub struct Elf32Phdr<E: Layout> {
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
unsafe impl<E: Layout> FileRecord for Elf64Phdr<E> {}
// SAFETY: see the Elf64 implementation.
unsafe impl<E: Layout> FileRecord for Elf32Phdr<E> {}

const _: () = assert!(std::mem::size_of::<Elf32Phdr<I386>>() == 32);
const _: () = assert!(std::mem::size_of::<Elf64Phdr<X86_64>>() == 56);
const _: () = assert!(std::mem::align_of::<Elf32Phdr<I386>>() == 1);
const _: () = assert!(std::mem::align_of::<Elf64Phdr<X86_64>>() == 1);

/// The common interface of the two physical program-header layouts.
/// Accessors take and return host integers; the record itself stays in
/// its file representation.
pub trait PhdrRecord: FileRecord + fmt::Debug {
    fn p_type(&self) -> u32;
    fn set_p_type(&mut self, value: u32);
    fn p_flags(&self) -> u32;
    fn set_p_flags(&mut self, value: u32);
    fn p_offset(&self) -> u64;
    fn set_p_offset(&mut self, value: u64);
    fn p_vaddr(&self) -> u64;
    fn set_p_vaddr(&mut self, value: u64);
    fn p_paddr(&self) -> u64;
    fn set_p_paddr(&mut self, value: u64);
    fn p_filesz(&self) -> u64;
    fn set_p_filesz(&mut self, value: u64);
    fn p_memsz(&self) -> u64;
    fn set_p_memsz(&mut self, value: u64);
    fn p_align(&self) -> u64;
    fn set_p_align(&mut self, value: u64);
}

macro_rules! impl_phdr_record {
    ($record:ident) => {
#[rustfmt::skip]
        impl<E: Layout> PhdrRecord for $record<E> {

            fn p_type(&self) -> u32 { self.p_type.get() }
            fn set_p_type(&mut self, value: u32) { self.p_type.set(value) }
            fn p_flags(&self) -> u32 { self.p_flags.get() }
            fn set_p_flags(&mut self, value: u32) { self.p_flags.set(value) }
            fn p_offset(&self) -> u64 { ElfWord::get(&self.p_offset) }
            fn set_p_offset(&mut self, value: u64) { ElfWord::set(&mut self.p_offset, value) }
            fn p_vaddr(&self) -> u64 { ElfWord::get(&self.p_vaddr) }
            fn set_p_vaddr(&mut self, value: u64) { ElfWord::set(&mut self.p_vaddr, value) }
            fn p_paddr(&self) -> u64 { ElfWord::get(&self.p_paddr) }
            fn set_p_paddr(&mut self, value: u64) { ElfWord::set(&mut self.p_paddr, value) }
            fn p_filesz(&self) -> u64 { ElfWord::get(&self.p_filesz) }
            fn set_p_filesz(&mut self, value: u64) { ElfWord::set(&mut self.p_filesz, value) }
            fn p_memsz(&self) -> u64 { ElfWord::get(&self.p_memsz) }
            fn set_p_memsz(&mut self, value: u64) { ElfWord::set(&mut self.p_memsz, value) }
            fn p_align(&self) -> u64 { ElfWord::get(&self.p_align) }
            fn set_p_align(&mut self, value: u64) { ElfWord::set(&mut self.p_align, value) }
        }
    };
}

impl_phdr_record!(Elf64Phdr);
impl_phdr_record!(Elf32Phdr);

pub type ElfPhdr<E> = <E as Layout>::Phdr;

/// A symbol table entry.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Elf64Sym<E: Layout> {
    pub st_name: U32<E>,
    st_info: u8,
    st_other: u8,
    pub st_shndx: U16<E>,
    pub st_value: U64<E>,
    pub st_size: U64<E>,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Elf32Sym<E: Layout> {
    pub st_name: U32<E>,
    pub st_value: U32<E>,
    pub st_size: U32<E>,
    st_info: u8,
    st_other: u8,
    pub st_shndx: U16<E>,
}

// SAFETY: all fields are byte-backed integers or bytes, and `repr(C)` does
// not insert padding between fields with alignment one.
unsafe impl<E: Layout> FileRecord for Elf64Sym<E> {}
// SAFETY: see the Elf64 implementation.
unsafe impl<E: Layout> FileRecord for Elf32Sym<E> {}

const _: () = assert!(std::mem::size_of::<Elf32Sym<I386>>() == 16);
const _: () = assert!(std::mem::size_of::<Elf64Sym<X86_64>>() == 24);
const _: () = assert!(std::mem::align_of::<Elf32Sym<I386>>() == 1);
const _: () = assert!(std::mem::align_of::<Elf64Sym<X86_64>>() == 1);

/// The common interface of the two physical symbol layouts. Accessors
/// take and return host integers; the record itself stays in its file
/// representation.
pub trait SymbolRecord: FileRecord + fmt::Debug {
    fn st_name(&self) -> u32;
    fn set_st_name(&mut self, value: u32);
    fn st_value(&self) -> u64;
    fn set_st_value(&mut self, value: u64);
    fn st_size(&self) -> u64;
    fn set_st_size(&mut self, value: u64);
    /// The 16-bit section index, widened so that it compares directly
    /// with the SHN_* constants.
    fn st_shndx(&self) -> u32;
    fn set_st_shndx(&mut self, value: u32);

    // st_info packs the symbol type and binding into one byte.
    fn st_type(&self) -> u32;
    fn set_type(&mut self, ty: u32);
    fn st_bind(&self) -> u32;
    fn set_bind(&mut self, bind: u32);

    // st_other holds the visibility in its low two bits; targets use
    // the remaining bits for their own flags.
    fn st_visibility(&self) -> u32;
    fn set_visibility(&mut self, visibility: u32);
    fn arm64_variant_pcs(&self) -> bool;
    fn set_arm64_variant_pcs(&mut self, value: bool);
    fn riscv_variant_cc(&self) -> bool;
    fn set_riscv_variant_cc(&mut self, value: bool);
    fn ppc64_local_entry(&self) -> u8;
    fn set_ppc64_local_entry(&mut self, value: u8);

    #[inline]
    fn is_undef(&self) -> bool {
        self.st_shndx() == SHN_UNDEF
    }

    #[inline]
    fn is_abs(&self) -> bool {
        self.st_shndx() == SHN_ABS
    }

    #[inline]
    fn is_common(&self) -> bool {
        self.st_shndx() == SHN_COMMON
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
    fn ppc64_preserves_r2(&self) -> bool {
        self.ppc64_local_entry() != 1
    }

    #[inline]
    fn ppc64_uses_toc(&self) -> bool {
        self.ppc64_local_entry() > 1
    }
}

macro_rules! impl_symbol_record {
    ($record:ident) => {
#[rustfmt::skip]
        impl<E: Layout> SymbolRecord for $record<E> {

            fn st_name(&self) -> u32 { self.st_name.get() }
            fn set_st_name(&mut self, value: u32) { self.st_name.set(value) }
            fn st_value(&self) -> u64 { ElfWord::get(&self.st_value) }
            fn set_st_value(&mut self, value: u64) { ElfWord::set(&mut self.st_value, value) }
            fn st_size(&self) -> u64 { ElfWord::get(&self.st_size) }
            fn set_st_size(&mut self, value: u64) { ElfWord::set(&mut self.st_size, value) }
            fn st_shndx(&self) -> u32 { u32::from(self.st_shndx.get()) }
            fn set_st_shndx(&mut self, value: u32) { self.st_shndx.set(value as u16) }

            fn st_type(&self) -> u32 { u32::from(self.st_info & 0xf) }
            fn st_bind(&self) -> u32 { u32::from(self.st_info >> 4) }
            fn st_visibility(&self) -> u32 { u32::from(self.st_other & 3) }
            fn arm64_variant_pcs(&self) -> bool { self.st_other & 0x80 != 0 }
            fn riscv_variant_cc(&self) -> bool { self.st_other & 0x80 != 0 }
            fn ppc64_local_entry(&self) -> u8 { self.st_other >> 5 }

            fn set_type(&mut self, ty: u32) {
                self.st_info = (self.st_info & 0xf0) | (ty as u8 & 0xf);
            }

            fn set_bind(&mut self, bind: u32) {
                self.st_info = (self.st_info & 0x0f) | ((bind as u8) << 4);
            }

            fn set_visibility(&mut self, v: u32) {
                self.st_other = (self.st_other & !3) | (v as u8 & 3);
            }

            fn set_arm64_variant_pcs(&mut self, v: bool) {
                self.st_other = (self.st_other & !0x80) | (u8::from(v) << 7);
            }

            fn set_riscv_variant_cc(&mut self, v: bool) {
                self.st_other = (self.st_other & !0x80) | (u8::from(v) << 7);
            }

            fn set_ppc64_local_entry(&mut self, v: u8) {
                self.st_other = (self.st_other & 0x1f) | ((v & 7) << 5);
            }
        }
    };
}

impl_symbol_record!(Elf64Sym);
impl_symbol_record!(Elf32Sym);

pub type ElfSym<E> = <E as Layout>::Sym;

/// A word-sized unsigned integer in an ELF file.
pub trait ElfWord: FileRecord + fmt::Debug {
    fn new(value: u64) -> Self;
    fn get(&self) -> u64;
    fn set(&mut self, value: u64);
    /// The word as a two's complement integer of its own width.
    fn get_signed(&self) -> i64;
}

// SAFETY: U32 is a transparent wrapper around a byte array.
unsafe impl<E: Layout> FileRecord for U32<E> {}

impl<E: Layout> ElfWord for U32<E> {
    #[inline(always)]
    fn new(value: u64) -> Self {
        Self::new(value as u32)
    }

    #[inline(always)]
    fn get(&self) -> u64 {
        u64::from(Self::get(self))
    }

    #[inline(always)]
    fn set(&mut self, value: u64) {
        Self::set(self, value as u32);
    }

    #[inline(always)]
    fn get_signed(&self) -> i64 {
        i64::from(Self::get(self) as i32)
    }
}

// SAFETY: U64 is a transparent wrapper around a byte array.
unsafe impl<E: Layout> FileRecord for U64<E> {}

impl<E: Layout> ElfWord for U64<E> {
    #[inline(always)]
    fn new(value: u64) -> Self {
        Self::new(value)
    }

    #[inline(always)]
    fn get(&self) -> u64 {
        Self::get(self)
    }

    #[inline(always)]
    fn set(&mut self, value: u64) {
        Self::set(self, value);
    }

    #[inline(always)]
    fn get_signed(&self) -> i64 {
        Self::get(self) as i64
    }
}

/// An ELF relocation record in its target-dependent file representation.
pub trait RelRecord: FileRecord + fmt::Debug {
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
    fn type_name<E: Arch>(&self) -> std::borrow::Cow<'static, str> {
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
//
// r_info packs the symbol index and the relocation type into one word. The
// two halves would swap places in memory with the byte order if they were
// separate fields, so the record keeps the word and the accessors split it.

/// A RELA relocation record.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ElfRela<E: Layout> {
    r_offset: E::Word,
    r_info: E::Word,
    r_addend: E::Word,
}

/// A REL relocation record.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ElfRelNoAddend<E: Layout> {
    r_offset: E::Word,
    r_info: E::Word,
}

// SAFETY: all fields are byte-backed integers, and `repr(C)` does not insert
// padding between fields with alignment one.
unsafe impl<E: Layout> FileRecord for ElfRela<E> {}
// SAFETY: see ElfRela.
unsafe impl<E: Layout> FileRecord for ElfRelNoAddend<E> {}

const _: () = assert!(std::mem::size_of::<ElfRela<I386>>() == 12);
const _: () = assert!(std::mem::size_of::<ElfRela<X86_64>>() == 24);
const _: () = assert!(std::mem::size_of::<ElfRelNoAddend<I386>>() == 8);
const _: () = assert!(std::mem::size_of::<ElfRelNoAddend<X86_64>>() == 16);
const _: () = assert!(std::mem::align_of::<ElfRela<I386>>() == 1);
const _: () = assert!(std::mem::align_of::<ElfRela<X86_64>>() == 1);
const _: () = assert!(std::mem::align_of::<ElfRelNoAddend<I386>>() == 1);
const _: () = assert!(std::mem::align_of::<ElfRelNoAddend<X86_64>>() == 1);

// ELF32 keeps the relocation type in the low 8 bits of r_info and the symbol
// index above them; ELF64 gives each 32 bits.
#[inline(always)]
fn r_info<E: Layout>(r_sym: u32, r_type: u32) -> u64 {
    if E::IS_64 {
        u64::from(r_sym) << 32 | u64::from(r_type)
    } else {
        u64::from(r_sym) << 8 | u64::from(r_type & 0xff)
    }
}

#[inline(always)]
fn r_info_sym<E: Layout>(r_info: u64) -> u32 {
    if E::IS_64 { (r_info >> 32) as u32 } else { (r_info >> 8) as u32 }
}

#[inline(always)]
fn r_info_type<E: Layout>(r_info: u64) -> u32 {
    if E::IS_64 { r_info as u32 } else { (r_info & 0xff) as u32 }
}

#[rustfmt::skip]
impl<E: Layout> RelRecord for ElfRela<E> {
    const IS_RELA: bool = true;

    fn new(r_offset: u64, r_type: u32, r_sym: u32, r_addend: i64) -> Self {
        Self {
            r_offset: E::Word::new(r_offset),
            r_info: E::Word::new(r_info::<E>(r_sym, r_type)),
            r_addend: E::Word::new(r_addend as u64),
        }
    }

    fn r_offset(&self) -> u64 { self.r_offset.get() }
    fn set_r_offset(&mut self, value: u64) { self.r_offset.set(value) }
    fn r_type(&self) -> u32 { r_info_type::<E>(self.r_info.get()) }
    fn set_r_type(&mut self, value: u32) { self.r_info.set(r_info::<E>(self.r_sym(), value)) }
    fn r_sym(&self) -> u32 { r_info_sym::<E>(self.r_info.get()) }
    fn set_r_sym(&mut self, value: u32) { self.r_info.set(r_info::<E>(value, self.r_type())) }
    fn r_addend(&self) -> i64 { self.r_addend.get_signed() }
    fn set_r_addend(&mut self, value: i64) { self.r_addend.set(value as u64) }
}

#[rustfmt::skip]
impl<E: Layout> RelRecord for ElfRelNoAddend<E> {
    const IS_RELA: bool = false;

    fn new(r_offset: u64, r_type: u32, r_sym: u32, _r_addend: i64) -> Self {
        Self {
            r_offset: E::Word::new(r_offset),
            r_info: E::Word::new(r_info::<E>(r_sym, r_type)),
        }
    }

    fn r_offset(&self) -> u64 { self.r_offset.get() }
    fn set_r_offset(&mut self, value: u64) { self.r_offset.set(value) }
    fn r_type(&self) -> u32 { r_info_type::<E>(self.r_info.get()) }
    fn set_r_type(&mut self, value: u32) { self.r_info.set(r_info::<E>(self.r_sym(), value)) }
    fn r_sym(&self) -> u32 { r_info_sym::<E>(self.r_info.get()) }
    fn set_r_sym(&mut self, value: u32) { self.r_info.set(r_info::<E>(value, self.r_type())) }
    fn r_addend(&self) -> i64 { 0 }
    fn set_r_addend(&mut self, _value: i64) {}
}

//
// Target-specific ELF data types
//

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Sparc64Rela {
    r_offset: U64<Sparc64>,
    r_sym: U32<Sparc64>,
    // SPARC keeps a second addend in the upper bits of the type field;
    // its backend separates the two.
    pub(crate) r_type_data: U24<Sparc64>, // SPARC-specific: used for R_SPARC_OLO10
    /// The relocation type proper, without the second addend.
    r_type: u8,
    r_addend: I64<Sparc64>,
}

// SAFETY: all fields are byte-backed integers or bytes, and `repr(C)` does
// not insert padding between fields with alignment one.
unsafe impl FileRecord for Sparc64Rela {}

const _: () = assert!(std::mem::size_of::<Sparc64Rela>() == 24);
const _: () = assert!(std::mem::align_of::<Sparc64Rela>() == 1);

#[rustfmt::skip]
impl RelRecord for Sparc64Rela {
    const IS_RELA: bool = true;

    fn new(r_offset: u64, r_type: u32, r_sym: u32, r_addend: i64) -> Self {
        let mut rel = Self::default();
        rel.set_r_offset(r_offset);
        rel.set_r_type(r_type);
        rel.set_r_sym(r_sym);
        rel.set_r_addend(r_addend);
        rel
    }

    fn r_offset(&self) -> u64 { self.r_offset.get() }
    fn set_r_offset(&mut self, value: u64) { self.r_offset.set(value) }
    fn r_type(&self) -> u32 { u32::from(self.r_type) }
    fn set_r_type(&mut self, value: u32) { self.r_type = value as u8 }
    fn r_sym(&self) -> u32 { self.r_sym.get() }
    fn set_r_sym(&mut self, value: u32) { self.r_sym.set(value) }
    fn r_addend(&self) -> i64 { self.r_addend.get() }
    fn set_r_addend(&mut self, value: i64) { self.r_addend.set(value) }
}

pub type ElfRel<E> = <E as Layout>::Rel;

/// Relocation records as they are laid out in a file, read as they are
/// used rather than copied out. Input files hold tens of millions of
/// relocations, which most passes go through once.
pub(crate) fn rels_from_bytes<E: Layout>(data: &[u8]) -> &[ElfRel<E>] {
    records_from_bytes(data)
}

/// Mutably views relocation records in their target-dependent file representation.
pub(crate) fn rels_from_bytes_mut<E: Layout>(data: &mut [u8]) -> &mut [ElfRel<E>] {
    records_from_bytes_mut(data)
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
pub struct Elf64Chdr<E: Layout> {
    pub ch_type: U32<E>,
    pub ch_reserved: U32<E>,
    pub ch_size: U64<E>,
    pub ch_addralign: U64<E>,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Elf32Chdr<E: Layout> {
    pub ch_type: U32<E>,
    pub ch_size: U32<E>,
    pub ch_addralign: U32<E>,
}

// SAFETY: all fields are byte-backed integers, and `repr(C)` does not insert
// padding between fields with alignment one.
unsafe impl<E: Layout> FileRecord for Elf64Chdr<E> {}
// SAFETY: see the Elf64 implementation.
unsafe impl<E: Layout> FileRecord for Elf32Chdr<E> {}

const _: () = assert!(std::mem::size_of::<Elf32Chdr<I386>>() == 12);
const _: () = assert!(std::mem::size_of::<Elf64Chdr<X86_64>>() == 24);
const _: () = assert!(std::mem::align_of::<Elf32Chdr<I386>>() == 1);
const _: () = assert!(std::mem::align_of::<Elf64Chdr<X86_64>>() == 1);

/// The common interface of the two physical compression-header layouts.
/// Accessors take and return host integers; the record itself stays in
/// its file representation.
pub trait ChdrRecord: FileRecord + fmt::Debug {
    fn ch_type(&self) -> u32;
    fn set_ch_type(&mut self, value: u32);
    fn ch_size(&self) -> u64;
    fn set_ch_size(&mut self, value: u64);
    fn ch_addralign(&self) -> u64;
    fn set_ch_addralign(&mut self, value: u64);
}

macro_rules! impl_chdr_record {
    ($record:ident) => {
#[rustfmt::skip]
        impl<E: Layout> ChdrRecord for $record<E> {

            fn ch_type(&self) -> u32 { self.ch_type.get() }
            fn set_ch_type(&mut self, value: u32) { self.ch_type.set(value) }
            fn ch_size(&self) -> u64 { ElfWord::get(&self.ch_size) }
            fn set_ch_size(&mut self, value: u64) { ElfWord::set(&mut self.ch_size, value) }
            fn ch_addralign(&self) -> u64 { ElfWord::get(&self.ch_addralign) }
            fn set_ch_addralign(&mut self, value: u64) {
                ElfWord::set(&mut self.ch_addralign, value)
            }
        }
    };
}

impl_chdr_record!(Elf64Chdr);
impl_chdr_record!(Elf32Chdr);

pub type ElfChdr<E> = <E as Layout>::Chdr;

/// A note header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ElfNhdr<E: Layout> {
    pub n_namesz: U32<E>,
    pub n_descsz: U32<E>,
    pub n_type: U32<E>,
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
    pub vn_version: U16<E>,
    pub vn_cnt: U16<E>,
    pub vn_file: U32<E>,
    pub vn_aux: U32<E>,
    pub vn_next: U32<E>,
}

// SAFETY: all fields are byte-backed integers, and `repr(C)` does not insert
// padding between fields with alignment one.
unsafe impl<E: Layout> FileRecord for ElfVerneed<E> {}

const _: () = assert!(std::mem::size_of::<ElfVerneed<I386>>() == 16);

/// A `.gnu.version_r` version entry.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ElfVernaux<E: Layout> {
    pub vna_hash: U32<E>,
    pub vna_flags: U16<E>,
    pub vna_other: U16<E>,
    pub vna_name: U32<E>,
    pub vna_next: U32<E>,
}

// SAFETY: all fields are byte-backed integers, and `repr(C)` does not insert
// padding between fields with alignment one.
unsafe impl<E: Layout> FileRecord for ElfVernaux<E> {}

const _: () = assert!(std::mem::size_of::<ElfVernaux<I386>>() == 16);

/// A `.gnu.version_d` definition entry.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ElfVerdef<E: Layout> {
    pub vd_version: U16<E>,
    pub vd_flags: U16<E>,
    pub vd_ndx: U16<E>,
    pub vd_cnt: U16<E>,
    pub vd_hash: U32<E>,
    pub vd_aux: U32<E>,
    pub vd_next: U32<E>,
}

// SAFETY: all fields are byte-backed integers, and `repr(C)` does not insert
// padding between fields with alignment one.
unsafe impl<E: Layout> FileRecord for ElfVerdef<E> {}

const _: () = assert!(std::mem::size_of::<ElfVerdef<I386>>() == 20);

/// A `.gnu.version_d` name entry.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ElfVerdaux<E: Layout> {
    pub vda_name: U32<E>,
    pub vda_next: U32<E>,
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
    pub magic: U16<E>,
    pub version: u8,
    pub flags: u8,
    pub abi_arch: u8,
    pub cfa_fixed_fp_offset: i8,
    pub cfa_fixed_ra_offset: i8,
    pub auxhdr_len: u8,
    pub num_fdes: U32<E>,
    pub num_fres: U32<E>,
    pub fre_len: U32<E>,
    pub fdeoff: U32<E>,
    pub freoff: U32<E>,
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
    pub func_start_offset: I64<E>,
    pub func_size: U32<E>,
    pub func_start_fre_off: U32<E>,
}

// SAFETY: all fields are byte-backed integers, and `repr(C)` does not insert
// padding between fields with alignment one.
unsafe impl<E: Layout> FileRecord for SFrameFdeIdx<E> {}

const _: () = assert!(std::mem::size_of::<SFrameFdeIdx<I386>>() == 16);
const _: () = assert!(std::mem::align_of::<SFrameFdeIdx<I386>>() == 1);
