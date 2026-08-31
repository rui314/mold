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
//! Most records are decoded into the host-native structs defined here as
//! they are used. Relocations instead use their target-dependent file
//! representation directly, with byte-backed integer fields handling
//! byte order and unaligned access. Thus the big tables — section headers,
//! symbols and relocations — stay in the input files rather than being
//! copied. Encoding and decoding are driven by an
//! [`Arch`](crate::arch::Arch), whose associated [`Endian`], relocation
//! type and `IS_64` constant select the layout at compile time, or by a
//! [`RecordLayout`] value where the target type isn't at hand.

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
pub trait Layout: Copy + Send + Sync + 'static {
    type Endian: Endian;
    type Rel: RelRecord<Endian = Self::Endian>;
    const IS_64: bool;
    const IS_RELA: bool;

    const WORD_SIZE: usize = if Self::IS_64 { 8 } else { 4 };
}

macro_rules! plain_layout {
    ($name:ident, $endian:ty, $rel:ty, $is_64:expr) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;

        impl Layout for $name {
            type Endian = $endian;
            type Rel = $rel;
            const IS_64: bool = $is_64;
            const IS_RELA: bool = true;
        }
    };
}

plain_layout!(Elf32Le, LittleEndian, Elf32RelaLe, false);
plain_layout!(Elf64Le, LittleEndian, Elf64RelaLe, true);
plain_layout!(Elf32Be, BigEndian, Elf32RelaBe, false);
plain_layout!(Elf64Be, BigEndian, Elf64RelaBe, true);

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

/// A cursor for decoding fixed-layout records.
struct Reader<'a, E: Layout> {
    bytes: &'a [u8],
    _arch: std::marker::PhantomData<E>,
}

impl<'a, E: Layout> Reader<'a, E> {
    fn new(bytes: &'a [u8]) -> Self {
        Reader {
            bytes,
            _arch: std::marker::PhantomData,
        }
    }

    fn u8(&self, offset: usize) -> u8 {
        self.bytes[offset]
    }

    #[inline]
    fn u16(&self, offset: usize) -> u16 {
        E::Endian::read_u16(&self.bytes[offset..])
    }

    #[inline]
    fn u32(&self, offset: usize) -> u32 {
        E::Endian::read_u32(&self.bytes[offset..])
    }

    #[inline]
    fn u64(&self, offset: usize) -> u64 {
        E::Endian::read_u64(&self.bytes[offset..])
    }

    /// Reads a word-sized (4 or 8 bytes) unsigned integer.
    #[inline]
    fn word(&self, offset: usize) -> u64 {
        if E::IS_64 {
            self.u64(offset)
        } else {
            self.u32(offset) as u64
        }
    }
}

/// A cursor for encoding fixed-layout records.
struct Writer<'a, E: Layout> {
    bytes: &'a mut [u8],
    _arch: std::marker::PhantomData<E>,
}

impl<'a, E: Layout> Writer<'a, E> {
    fn new(bytes: &'a mut [u8]) -> Self {
        Writer {
            bytes,
            _arch: std::marker::PhantomData,
        }
    }

    fn u8(&mut self, offset: usize, value: u8) {
        self.bytes[offset] = value;
    }

    #[inline]
    fn u16(&mut self, offset: usize, value: u16) {
        E::Endian::write_u16(&mut self.bytes[offset..], value);
    }

    #[inline]
    fn u32(&mut self, offset: usize, value: u32) {
        E::Endian::write_u32(&mut self.bytes[offset..], value);
    }

    #[inline]
    fn u64(&mut self, offset: usize, value: u64) {
        E::Endian::write_u64(&mut self.bytes[offset..], value);
    }

    #[inline]
    fn word(&mut self, offset: usize, value: u64) {
        if E::IS_64 {
            self.u64(offset, value);
        } else {
            self.u32(offset, value as u32);
        }
    }
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

/// The ELF file header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ElfEhdr {
    pub e_ident: [u8; 16],
    pub e_type: u16,
    pub e_machine: u16,
    pub e_version: u32,
    pub e_entry: u64,
    pub e_phoff: u64,
    pub e_shoff: u64,
    pub e_flags: u32,
    pub e_ehsize: u16,
    pub e_phentsize: u16,
    pub e_phnum: u16,
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

impl Record for ElfEhdr {
    fn size<E: Layout>() -> usize {
        if E::IS_64 {
            64
        } else {
            52
        }
    }

    fn parse<E: Layout>(bytes: &[u8]) -> Self {
        if E::IS_64 && E::Endian::IS_NATIVE {
            let bytes = &bytes[..Self::size::<E>()];
            // SAFETY: ElfEhdr has the native ELF64 field layout, `bytes`
            // contains a complete record, and an unaligned read accepts the
            // file's alignment.
            return unsafe { bytes.as_ptr().cast::<ElfEhdr>().read_unaligned() };
        }

        let r = Reader::<E>::new(bytes);
        let w = E::WORD_SIZE;
        ElfEhdr {
            e_ident: bytes[..16].try_into().unwrap(),
            e_type: r.u16(16),
            e_machine: r.u16(18),
            e_version: r.u32(20),
            e_entry: r.word(24),
            e_phoff: r.word(24 + w),
            e_shoff: r.word(24 + 2 * w),
            e_flags: r.u32(24 + 3 * w),
            e_ehsize: r.u16(28 + 3 * w),
            e_phentsize: r.u16(30 + 3 * w),
            e_phnum: r.u16(32 + 3 * w),
            e_shentsize: r.u16(34 + 3 * w),
            e_shnum: r.u16(36 + 3 * w),
            e_shstrndx: r.u16(38 + 3 * w),
        }
    }

    fn write<E: Layout>(&self, buf: &mut [u8]) {
        let mut w = Writer::<E>::new(buf);
        let ws = E::WORD_SIZE;
        w.bytes[..16].copy_from_slice(&self.e_ident);
        w.u16(16, self.e_type);
        w.u16(18, self.e_machine);
        w.u32(20, self.e_version);
        w.word(24, self.e_entry);
        w.word(24 + ws, self.e_phoff);
        w.word(24 + 2 * ws, self.e_shoff);
        w.u32(24 + 3 * ws, self.e_flags);
        w.u16(28 + 3 * ws, self.e_ehsize);
        w.u16(30 + 3 * ws, self.e_phentsize);
        w.u16(32 + 3 * ws, self.e_phnum);
        w.u16(34 + 3 * ws, self.e_shentsize);
        w.u16(36 + 3 * ws, self.e_shnum);
        w.u16(38 + 3 * ws, self.e_shstrndx);
    }
}

const _: () = assert!(std::mem::size_of::<ElfEhdr>() == 64);

/// A section header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ElfShdr {
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

impl Record for ElfShdr {
    fn size<E: Layout>() -> usize {
        if E::IS_64 {
            64
        } else {
            40
        }
    }

    fn parse<E: Layout>(bytes: &[u8]) -> Self {
        if E::IS_64 && E::Endian::IS_NATIVE {
            let bytes = &bytes[..Self::size::<E>()];
            // SAFETY: ElfShdr has the native ELF64 field layout, `bytes`
            // contains a complete record, and an unaligned read accepts the
            // file's alignment.
            return unsafe { bytes.as_ptr().cast::<ElfShdr>().read_unaligned() };
        }

        let r = Reader::<E>::new(bytes);
        let w = E::WORD_SIZE;
        ElfShdr {
            sh_name: r.u32(0),
            sh_type: r.u32(4),
            sh_flags: r.word(8),
            sh_addr: r.word(8 + w),
            sh_offset: r.word(8 + 2 * w),
            sh_size: r.word(8 + 3 * w),
            sh_link: r.u32(8 + 4 * w),
            sh_info: r.u32(12 + 4 * w),
            sh_addralign: r.word(16 + 4 * w),
            sh_entsize: r.word(16 + 5 * w),
        }
    }

    fn write<E: Layout>(&self, buf: &mut [u8]) {
        let mut w = Writer::<E>::new(buf);
        let ws = E::WORD_SIZE;
        w.u32(0, self.sh_name);
        w.u32(4, self.sh_type);
        w.word(8, self.sh_flags);
        w.word(8 + ws, self.sh_addr);
        w.word(8 + 2 * ws, self.sh_offset);
        w.word(8 + 3 * ws, self.sh_size);
        w.u32(8 + 4 * ws, self.sh_link);
        w.u32(12 + 4 * ws, self.sh_info);
        w.word(16 + 4 * ws, self.sh_addralign);
        w.word(16 + 5 * ws, self.sh_entsize);
    }
}

const _: () = assert!(std::mem::size_of::<ElfShdr>() == 64);

/// A program header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ElfPhdr {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

impl Record for ElfPhdr {
    fn size<E: Layout>() -> usize {
        if E::IS_64 {
            56
        } else {
            32
        }
    }

    fn parse<E: Layout>(bytes: &[u8]) -> Self {
        if E::IS_64 && E::Endian::IS_NATIVE {
            let bytes = &bytes[..Self::size::<E>()];
            // SAFETY: ElfPhdr has the native ELF64 field layout, `bytes`
            // contains a complete record, and an unaligned read accepts the
            // file's alignment.
            return unsafe { bytes.as_ptr().cast::<ElfPhdr>().read_unaligned() };
        }

        let r = Reader::<E>::new(bytes);
        if E::IS_64 {
            ElfPhdr {
                p_type: r.u32(0),
                p_flags: r.u32(4),
                p_offset: r.u64(8),
                p_vaddr: r.u64(16),
                p_paddr: r.u64(24),
                p_filesz: r.u64(32),
                p_memsz: r.u64(40),
                p_align: r.u64(48),
            }
        } else {
            ElfPhdr {
                p_type: r.u32(0),
                p_offset: r.u32(4) as u64,
                p_vaddr: r.u32(8) as u64,
                p_paddr: r.u32(12) as u64,
                p_filesz: r.u32(16) as u64,
                p_memsz: r.u32(20) as u64,
                p_flags: r.u32(24),
                p_align: r.u32(28) as u64,
            }
        }
    }

    fn write<E: Layout>(&self, buf: &mut [u8]) {
        let mut w = Writer::<E>::new(buf);
        if E::IS_64 {
            w.u32(0, self.p_type);
            w.u32(4, self.p_flags);
            w.u64(8, self.p_offset);
            w.u64(16, self.p_vaddr);
            w.u64(24, self.p_paddr);
            w.u64(32, self.p_filesz);
            w.u64(40, self.p_memsz);
            w.u64(48, self.p_align);
        } else {
            w.u32(0, self.p_type);
            w.u32(4, self.p_offset as u32);
            w.u32(8, self.p_vaddr as u32);
            w.u32(12, self.p_paddr as u32);
            w.u32(16, self.p_filesz as u32);
            w.u32(20, self.p_memsz as u32);
            w.u32(24, self.p_flags);
            w.u32(28, self.p_align as u32);
        }
    }
}

const _: () = assert!(std::mem::size_of::<ElfPhdr>() == 56);

/// A symbol table entry.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ElfSym {
    pub st_name: u32,
    pub st_info: u8,
    pub st_other: u8,
    pub st_shndx: u16,
    pub st_value: u64,
    pub st_size: u64,
}

impl ElfSym {
    pub fn st_type(&self) -> u32 {
        (self.st_info & 0xf) as u32
    }

    pub fn st_bind(&self) -> u32 {
        (self.st_info >> 4) as u32
    }

    pub fn st_visibility(&self) -> u32 {
        (self.st_other & 3) as u32
    }

    pub fn set_type(&mut self, ty: u32) {
        self.st_info = (self.st_info & 0xf0) | (ty as u8 & 0xf);
    }

    pub fn set_bind(&mut self, bind: u32) {
        self.st_info = (self.st_info & 0x0f) | ((bind as u8) << 4);
    }

    pub fn set_visibility(&mut self, visibility: u32) {
        self.st_other = (self.st_other & !3) | (visibility as u8 & 3);
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
        self.st_other & 0x80 != 0
    }

    /// The RISC-V counterpart of [`Self::arm64_variant_pcs`].
    pub fn riscv_variant_cc(&self) -> bool {
        self.st_other & 0x80 != 0
    }

    /// The distance between a PPC64 ELFv2 function's global and local entry
    /// points, encoded in the top three bits of `st_other`.
    pub fn ppc64_local_entry(&self) -> u8 {
        self.st_other >> 5
    }

    pub fn ppc64_preserves_r2(&self) -> bool {
        self.ppc64_local_entry() != 1
    }

    pub fn ppc64_uses_toc(&self) -> bool {
        self.ppc64_local_entry() > 1
    }
}

impl Record for ElfSym {
    fn size<E: Layout>() -> usize {
        if E::IS_64 {
            24
        } else {
            16
        }
    }

    fn parse<E: Layout>(bytes: &[u8]) -> Self {
        if E::IS_64 && E::Endian::IS_NATIVE {
            let bytes = &bytes[..Self::size::<E>()];
            // SAFETY: ElfSym has the native ELF64 field layout, `bytes`
            // contains a complete record, and an unaligned read accepts the
            // file's alignment.
            return unsafe { bytes.as_ptr().cast::<ElfSym>().read_unaligned() };
        }

        let r = Reader::<E>::new(bytes);
        if E::IS_64 {
            ElfSym {
                st_name: r.u32(0),
                st_info: r.u8(4),
                st_other: r.u8(5),
                st_shndx: r.u16(6),
                st_value: r.u64(8),
                st_size: r.u64(16),
            }
        } else {
            ElfSym {
                st_name: r.u32(0),
                st_value: r.u32(4) as u64,
                st_size: r.u32(8) as u64,
                st_info: r.u8(12),
                st_other: r.u8(13),
                st_shndx: r.u16(14),
            }
        }
    }

    fn write<E: Layout>(&self, buf: &mut [u8]) {
        let mut w = Writer::<E>::new(buf);
        if E::IS_64 {
            w.u32(0, self.st_name);
            w.u8(4, self.st_info);
            w.u8(5, self.st_other);
            w.u16(6, self.st_shndx);
            w.u64(8, self.st_value);
            w.u64(16, self.st_size);
        } else {
            w.u32(0, self.st_name);
            w.u32(4, self.st_value as u32);
            w.u32(8, self.st_size as u32);
            w.u8(12, self.st_info);
            w.u8(13, self.st_other);
            w.u16(14, self.st_shndx);
        }
    }
}

const _: () = assert!(std::mem::size_of::<ElfSym>() == 24);

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
    pub fn read_sym(self, bytes: &[u8]) -> ElfSym {
        if self.is_64 {
            ElfSym {
                st_name: self.u32(bytes, 0),
                st_info: bytes[4],
                st_other: bytes[5],
                st_shndx: self.u16(bytes, 6),
                st_value: self.u64(bytes, 8),
                st_size: self.u64(bytes, 16),
            }
        } else {
            ElfSym {
                st_name: self.u32(bytes, 0),
                st_value: self.u32(bytes, 4) as u64,
                st_size: self.u32(bytes, 8) as u64,
                st_info: bytes[12],
                st_other: bytes[13],
                st_shndx: self.u16(bytes, 14),
            }
        }
    }

    #[inline]
    pub fn write_sym(self, sym: &ElfSym, buf: &mut [u8]) {
        if self.is_64 {
            self.put_u32(buf, 0, sym.st_name);
            buf[4] = sym.st_info;
            buf[5] = sym.st_other;
            self.put_u16(buf, 6, sym.st_shndx);
            self.put_u64(buf, 8, sym.st_value);
            self.put_u64(buf, 16, sym.st_size);
        } else {
            self.put_u32(buf, 0, sym.st_name);
            self.put_u32(buf, 4, sym.st_value as u32);
            self.put_u32(buf, 8, sym.st_size as u32);
            buf[12] = sym.st_info;
            buf[13] = sym.st_other;
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
    pub fn read_shdr(self, bytes: &[u8]) -> ElfShdr {
        let w = if self.is_64 { 8 } else { 4 };
        ElfShdr {
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
    pub fn write_shdr(self, shdr: &ElfShdr, buf: &mut [u8]) {
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

    pub fn from_records(layout: RecordLayout, syms: &[ElfSym]) -> SymTable {
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
    pub fn at(&self, i: usize) -> ElfSym {
        let size = self.layout.sym_size();
        self.layout.read_sym(&self.data[i * size..(i + 1) * size])
    }

    #[inline]
    pub fn get(&self, i: usize) -> Option<ElfSym> {
        (i < self.len()).then(|| self.at(i))
    }

    /// Like [`Self::at`] for code specialized for the target, which
    /// knows the layout at compile time; relocation loops read a symbol
    /// per relocation.
    #[inline]
    pub fn at_in<E: Layout>(&self, i: usize) -> ElfSym {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        let size = ElfSym::size::<E>();
        ElfSym::parse::<E>(&self.data[i * size..(i + 1) * size])
    }

    #[inline]
    pub fn get_in<E: Layout>(&self, i: usize) -> Option<ElfSym> {
        (i < self.len()).then(|| self.at_in::<E>(i))
    }

    /// Replaces a symbol in a table the linker builds.
    pub fn set_in<E: Layout>(&mut self, i: usize, sym: ElfSym) {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        let size = ElfSym::size::<E>();
        sym.write::<E>(&mut self.data.to_mut()[i * size..(i + 1) * size]);
    }

    /// The type and binding byte of symbol `i` alone, for relocation loops
    /// that look for section symbols.
    #[inline]
    pub fn st_info(&self, i: usize) -> u8 {
        let offset = if self.layout.is_64 { 4 } else { 12 };
        self.data[i * self.layout.sym_size() + offset]
    }

    /// Like [`Self::st_info`] for code specialized for the target.
    #[inline]
    pub fn st_info_in<E: Layout>(&self, i: usize) -> u8 {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        let offset = if E::IS_64 { 4 } else { 12 };
        self.data[i * ElfSym::size::<E>() + offset]
    }

    /// Reads the type and binding byte after checking the symbol index once.
    #[inline(always)]
    pub fn st_info_in_checked<E: Layout>(&self, i: usize) -> Option<u8> {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        if i >= self.len {
            return None;
        }
        let offset = i * ElfSym::size::<E>() + if E::IS_64 { 4 } else { 12 };
        // SAFETY: `i < self.len` and every symbol record has this field.
        Some(unsafe { *self.data.as_ptr().add(offset) })
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
        E::Endian::read_u16(&self.data[i * ElfSym::size::<E>() + offset..])
    }

    /// Reads a section index after the caller has checked `i < self.len()`.
    #[inline(always)]
    pub(crate) unsafe fn st_shndx_in_unchecked<E: Layout>(&self, i: usize) -> u16 {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        debug_assert!(i < self.len);
        let offset = i * ElfSym::size::<E>() + if E::IS_64 { 6 } else { 14 };
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
        E::Endian::read_u32(&self.data[i * ElfSym::size::<E>()..])
    }

    /// Walks the name offsets without decoding the rest of each symbol.
    #[inline]
    pub fn name_offsets_in<E: Layout>(
        &self,
    ) -> impl DoubleEndedIterator<Item = u32> + ExactSizeIterator + '_ {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        self.data
            .chunks_exact(ElfSym::size::<E>())
            .map(E::Endian::read_u32)
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = ElfSym> + ExactSizeIterator + '_ {
        let layout = self.layout;
        self.data
            .chunks_exact(layout.sym_size())
            .map(move |bytes| layout.read_sym(bytes))
    }

    /// Appends a symbol to a table the linker builds.
    pub fn push(&mut self, sym: ElfSym) {
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
    extra: Vec<ElfShdr>,
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
            E::Endian::read_u32(&self.data[i * ElfShdr::size::<E>()..])
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
    pub fn at(&self, i: usize) -> ElfShdr {
        let n = self.num_in_file();
        if i < n {
            let size = self.layout.shdr_size();
            self.layout.read_shdr(&self.data[i * size..(i + 1) * size])
        } else {
            self.extra[i - n]
        }
    }

    #[inline]
    pub fn get(&self, i: usize) -> Option<ElfShdr> {
        (i < self.len()).then(|| self.at(i))
    }

    /// Like [`Self::at`] for code specialized for the target, which knows
    /// the layout at compile time.
    #[inline]
    pub fn at_in<E: Layout>(&self, i: usize) -> ElfShdr {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        if i < self.num_in_file {
            let size = ElfShdr::size::<E>();
            ElfShdr::parse::<E>(&self.data[i * size..(i + 1) * size])
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
    pub unsafe fn at_in_unchecked<E: Layout>(&self, i: usize) -> ElfShdr {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        debug_assert!(i < self.len());
        if i < self.num_in_file {
            let size = ElfShdr::size::<E>();
            // SAFETY: the caller proved `i`, and `in_file` accepted only a
            // whole number of section-header records.
            let bytes =
                unsafe { std::slice::from_raw_parts(self.data.as_ptr().add(i * size), size) };
            ElfShdr::parse::<E>(bytes)
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
    pub unsafe fn file_at_in_unchecked<E: Layout>(&self, i: usize) -> ElfShdr {
        debug_assert_eq!(self.layout, RecordLayout::of::<E>());
        debug_assert!(i < self.num_in_file);
        let size = ElfShdr::size::<E>();
        // SAFETY: the caller proved `i`, and `in_file` accepted only a whole
        // number of section-header records.
        let bytes = unsafe { std::slice::from_raw_parts(self.data.as_ptr().add(i * size), size) };
        ElfShdr::parse::<E>(bytes)
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
            let ptr = unsafe { self.data.as_ptr().add(i * ElfShdr::size::<E>()) };
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
        let ptr = unsafe { self.data.as_ptr().add(i * ElfShdr::size::<E>()) };
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
        let ptr = unsafe { self.data.as_ptr().add(i * ElfShdr::size::<E>() + 4) };
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
            E::Endian::read_u32(&self.data[i * ElfShdr::size::<E>() + 4..])
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
            let bytes = &self.data[i * ElfShdr::size::<E>() + 8..];
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
            let offset = i * ElfShdr::size::<E>() + 8;
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
            let base = i * ElfShdr::size::<E>();
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

    pub fn iter(&self) -> impl Iterator<Item = ElfShdr> + '_ {
        (0..self.len()).map(|i| self.at(i))
    }

    pub fn push(&mut self, shdr: ElfShdr) {
        self.extra.push(shdr);
    }
}

/// An entry of the `.dynamic` section.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ElfDyn {
    pub d_tag: u64,
    pub d_val: u64,
}

impl Record for ElfDyn {
    fn size<E: Layout>() -> usize {
        2 * E::WORD_SIZE
    }

    fn parse<E: Layout>(bytes: &[u8]) -> Self {
        if E::IS_64 && E::Endian::IS_NATIVE {
            let bytes = &bytes[..Self::size::<E>()];
            // SAFETY: ElfDyn has the native ELF64 field layout, `bytes`
            // contains a complete record, and an unaligned read accepts the
            // file's alignment.
            return unsafe { bytes.as_ptr().cast::<ElfDyn>().read_unaligned() };
        }

        let r = Reader::<E>::new(bytes);
        ElfDyn {
            d_tag: r.word(0),
            d_val: r.word(E::WORD_SIZE),
        }
    }

    fn write<E: Layout>(&self, buf: &mut [u8]) {
        let mut w = Writer::<E>::new(buf);
        w.word(0, self.d_tag);
        w.word(E::WORD_SIZE, self.d_val);
    }
}

const _: () = assert!(std::mem::size_of::<ElfDyn>() == 16);

/// The header of a compressed section.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ElfChdr {
    pub ch_type: u32,
    pub ch_size: u64,
    pub ch_addralign: u64,
}

impl Record for ElfChdr {
    fn size<E: Layout>() -> usize {
        if E::IS_64 {
            24
        } else {
            12
        }
    }

    fn parse<E: Layout>(bytes: &[u8]) -> Self {
        let r = Reader::<E>::new(bytes);
        if E::IS_64 {
            ElfChdr {
                ch_type: r.u32(0),
                ch_size: r.u64(8),
                ch_addralign: r.u64(16),
            }
        } else {
            ElfChdr {
                ch_type: r.u32(0),
                ch_size: r.u32(4) as u64,
                ch_addralign: r.u32(8) as u64,
            }
        }
    }

    fn write<E: Layout>(&self, buf: &mut [u8]) {
        let mut w = Writer::<E>::new(buf);
        if E::IS_64 {
            w.u32(0, self.ch_type);
            w.u32(4, 0);
            w.u64(8, self.ch_size);
            w.u64(16, self.ch_addralign);
        } else {
            w.u32(0, self.ch_type);
            w.u32(4, self.ch_size as u32);
            w.u32(8, self.ch_addralign as u32);
        }
    }
}

/// A note header.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ElfNhdr {
    pub n_namesz: u32,
    pub n_descsz: u32,
    pub n_type: u32,
}

impl Record for ElfNhdr {
    fn size<E: Layout>() -> usize {
        12
    }

    fn parse<E: Layout>(bytes: &[u8]) -> Self {
        let r = Reader::<E>::new(bytes);
        ElfNhdr {
            n_namesz: r.u32(0),
            n_descsz: r.u32(4),
            n_type: r.u32(8),
        }
    }

    fn write<E: Layout>(&self, buf: &mut [u8]) {
        let mut w = Writer::<E>::new(buf);
        w.u32(0, self.n_namesz);
        w.u32(4, self.n_descsz);
        w.u32(8, self.n_type);
    }
}

/// A `.gnu.version_r` file entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ElfVerneed {
    pub vn_version: u16,
    pub vn_cnt: u16,
    pub vn_file: u32,
    pub vn_aux: u32,
    pub vn_next: u32,
}

impl Record for ElfVerneed {
    fn size<E: Layout>() -> usize {
        16
    }

    fn parse<E: Layout>(bytes: &[u8]) -> Self {
        let r = Reader::<E>::new(bytes);
        ElfVerneed {
            vn_version: r.u16(0),
            vn_cnt: r.u16(2),
            vn_file: r.u32(4),
            vn_aux: r.u32(8),
            vn_next: r.u32(12),
        }
    }

    fn write<E: Layout>(&self, buf: &mut [u8]) {
        let mut w = Writer::<E>::new(buf);
        w.u16(0, self.vn_version);
        w.u16(2, self.vn_cnt);
        w.u32(4, self.vn_file);
        w.u32(8, self.vn_aux);
        w.u32(12, self.vn_next);
    }
}

/// A `.gnu.version_r` version entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ElfVernaux {
    pub vna_hash: u32,
    pub vna_flags: u16,
    pub vna_other: u16,
    pub vna_name: u32,
    pub vna_next: u32,
}

impl Record for ElfVernaux {
    fn size<E: Layout>() -> usize {
        16
    }

    fn parse<E: Layout>(bytes: &[u8]) -> Self {
        let r = Reader::<E>::new(bytes);
        ElfVernaux {
            vna_hash: r.u32(0),
            vna_flags: r.u16(4),
            vna_other: r.u16(6),
            vna_name: r.u32(8),
            vna_next: r.u32(12),
        }
    }

    fn write<E: Layout>(&self, buf: &mut [u8]) {
        let mut w = Writer::<E>::new(buf);
        w.u32(0, self.vna_hash);
        w.u16(4, self.vna_flags);
        w.u16(6, self.vna_other);
        w.u32(8, self.vna_name);
        w.u32(12, self.vna_next);
    }
}

/// A `.gnu.version_d` definition entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ElfVerdef {
    pub vd_version: u16,
    pub vd_flags: u16,
    pub vd_ndx: u16,
    pub vd_cnt: u16,
    pub vd_hash: u32,
    pub vd_aux: u32,
    pub vd_next: u32,
}

impl Record for ElfVerdef {
    fn size<E: Layout>() -> usize {
        20
    }

    fn parse<E: Layout>(bytes: &[u8]) -> Self {
        let r = Reader::<E>::new(bytes);
        ElfVerdef {
            vd_version: r.u16(0),
            vd_flags: r.u16(2),
            vd_ndx: r.u16(4),
            vd_cnt: r.u16(6),
            vd_hash: r.u32(8),
            vd_aux: r.u32(12),
            vd_next: r.u32(16),
        }
    }

    fn write<E: Layout>(&self, buf: &mut [u8]) {
        let mut w = Writer::<E>::new(buf);
        w.u16(0, self.vd_version);
        w.u16(2, self.vd_flags);
        w.u16(4, self.vd_ndx);
        w.u16(6, self.vd_cnt);
        w.u32(8, self.vd_hash);
        w.u32(12, self.vd_aux);
        w.u32(16, self.vd_next);
    }
}

/// A `.gnu.version_d` name entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ElfVerdaux {
    pub vda_name: u32,
    pub vda_next: u32,
}

impl Record for ElfVerdaux {
    fn size<E: Layout>() -> usize {
        8
    }

    fn parse<E: Layout>(bytes: &[u8]) -> Self {
        let r = Reader::<E>::new(bytes);
        ElfVerdaux {
            vda_name: r.u32(0),
            vda_next: r.u32(4),
        }
    }

    fn write<E: Layout>(&self, buf: &mut [u8]) {
        let mut w = Writer::<E>::new(buf);
        w.u32(0, self.vda_name);
        w.u32(4, self.vda_next);
    }
}

/// SFrame is a simple unwind information format used as a lightweight
/// alternative to .eh_frame. A .sframe section consists of a header, an
/// array of Function Descriptor Entries (FDEs) sorted by PC, and a blob
/// of Frame Row Entries (FREs). mold understands SFrame Version 3.
///
/// https://sourceware.org/binutils/docs/sframe-spec.html
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SFrameHeader {
    pub magic: u16,
    pub version: u8,
    pub flags: u8,
    pub abi_arch: u8,
    pub cfa_fixed_fp_offset: i8,
    pub cfa_fixed_ra_offset: i8,
    pub auxhdr_len: u8,
    pub num_fdes: u32,
    pub num_fres: u32,
    pub fre_len: u32,
    pub fdeoff: u32,
    pub freoff: u32,
}

pub const SFRAME_MAGIC: u16 = 0xdee2;
pub const SFRAME_F_FDE_SORTED: u8 = 0x1;
pub const SFRAME_F_FRAME_POINTER: u8 = 0x2;
pub const SFRAME_F_FDE_FUNC_START_PCREL: u8 = 0x4;

pub const SFRAME_ABI_AARCH64_ENDIAN_BIG: u8 = 1;
pub const SFRAME_ABI_AARCH64_ENDIAN_LITTLE: u8 = 2;
pub const SFRAME_ABI_AMD64_ENDIAN_LITTLE: u8 = 3;
pub const SFRAME_ABI_S390X_ENDIAN_BIG: u8 = 4;

impl Record for SFrameHeader {
    fn size<E: Layout>() -> usize {
        28
    }

    fn parse<E: Layout>(bytes: &[u8]) -> Self {
        let r = Reader::<E>::new(bytes);
        SFrameHeader {
            magic: r.u16(0),
            version: r.u8(2),
            flags: r.u8(3),
            abi_arch: r.u8(4),
            cfa_fixed_fp_offset: r.u8(5) as i8,
            cfa_fixed_ra_offset: r.u8(6) as i8,
            auxhdr_len: r.u8(7),
            num_fdes: r.u32(8),
            num_fres: r.u32(12),
            fre_len: r.u32(16),
            fdeoff: r.u32(20),
            freoff: r.u32(24),
        }
    }

    fn write<E: Layout>(&self, buf: &mut [u8]) {
        let mut w = Writer::<E>::new(buf);
        w.u16(0, self.magic);
        w.u8(2, self.version);
        w.u8(3, self.flags);
        w.u8(4, self.abi_arch);
        w.u8(5, self.cfa_fixed_fp_offset as u8);
        w.u8(6, self.cfa_fixed_ra_offset as u8);
        w.u8(7, self.auxhdr_len);
        w.u32(8, self.num_fdes);
        w.u32(12, self.num_fres);
        w.u32(16, self.fre_len);
        w.u32(20, self.fdeoff);
        w.u32(24, self.freoff);
    }
}

/// The index part of an SFrame Version 3 FDE. The func_start_offset field
/// is PC-relative (relative to its own address) when the section flag
/// SFRAME_F_FDE_FUNC_START_PCREL is set, which is how mold always emits it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SFrameFdeIdx {
    pub func_start_offset: i64,
    pub func_size: u32,
    pub func_start_fre_off: u32,
}

impl Record for SFrameFdeIdx {
    fn size<E: Layout>() -> usize {
        16
    }

    fn parse<E: Layout>(bytes: &[u8]) -> Self {
        let r = Reader::<E>::new(bytes);
        SFrameFdeIdx {
            func_start_offset: r.u64(0) as i64,
            func_size: r.u32(8),
            func_start_fre_off: r.u32(12),
        }
    }

    fn write<E: Layout>(&self, buf: &mut [u8]) {
        let mut w = Writer::<E>::new(buf);
        w.u64(0, self.func_start_offset as u64);
        w.u32(8, self.func_size);
        w.u32(12, self.func_start_fre_off);
    }
}

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
