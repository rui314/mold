//! Input file type detection, and the architectures of thin and
//! universal (fat) files.

use crate::arch::Target;
use crate::macho::*;
use crate::mapped_file::MappedFile;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileType {
    Unknown,
    Empty,
    Object,
    Dylib,
    Archive,
    /// A TAPI text-based dylib stub (.tbd), a YAML description of a dylib
    /// that ships in SDKs in place of the binary.
    Tapi,
    Fat,
    /// An LLVM bitcode file, produced by -flto; compiled by libLTO at
    /// link time.
    LlvmBitcode,
}

/// The mach header of a 64-bit Mach-O file, or `None` if `data` is not
/// one.
fn macho_header(data: &[u8]) -> Option<MachHeader> {
    if data.len() < size_of::<MachHeader>() {
        return None;
    }
    let hdr = MachHeader::parse(data);
    (hdr.magic.get() == MH_MAGIC_64).then_some(hdr)
}

/// Returns the target name of a Mach-O file, or `None` if it is not a
/// 64-bit Mach-O file for a CPU type we recognize.
pub fn get_macho_target(data: &[u8]) -> Option<&'static str> {
    crate::arch::cputype_name(macho_header(data)?.cputype.get())
}

/// Returns the file type (MH_EXECUTE, MH_BUNDLE, ...) of a 64-bit
/// Mach-O file, or `None` if it is not one.
pub fn get_macho_filetype(data: &[u8]) -> Option<u32> {
    macho_header(data).map(|hdr| hdr.filetype.get())
}

pub fn get_file_type(mf: &MappedFile) -> FileType {
    let data = mf.data();
    if data.is_empty() {
        return FileType::Empty;
    }

    if data.len() >= 8 && &data[..8] == b"!<arch>\n" {
        return FileType::Archive;
    }

    if data.starts_with(b"--- !tapi-tbd") || data.starts_with(b"---\narchs:") {
        return FileType::Tapi;
    }
    // TBD version 5 is JSON; the version key may come last in the file
    // (Xcode's eager-linking stubs put it there), so a .tbd that
    // starts with '{' is taken as one.
    if data.starts_with(b"{")
        && (mf.name.extension().is_some_and(|ext| ext == "tbd")
            || data.windows(16).any(|w| w == b"tapi_tbd_version"))
    {
        return FileType::Tapi;
    }

    // LLVM bitcode in the wrapper Apple's compilers put it in. ld-prime
    // knows no raw bitcode ("BC\xc0\xde"), whatever its target: a file
    // of it is of an unknown type, an archive member none it loads.
    if data.starts_with(&0x0b17_c0deu32.to_le_bytes()) {
        return FileType::LlvmBitcode;
    }

    if let Some(hdr) = macho_header(data) {
        return match hdr.filetype.get() {
            MH_OBJECT => FileType::Object,
            MH_DYLIB => FileType::Dylib,
            _ => FileType::Unknown,
        };
    }
    // A universal file's header is big-endian.
    if data.len() >= size_of::<MachHeader>() && data.starts_with(&FAT_MAGIC.to_be_bytes()) {
        return FileType::Fat;
    }
    FileType::Unknown
}

/// An architecture's name, from a Mach-O CPU type and subtype: one of
/// the subtypes of the CPU types mold links for, or "unknown".
fn arch_name(cputype: u32, cpusubtype: u32) -> &'static str {
    match (cputype, cpusubtype & !CPU_SUBTYPE_MASK) {
        (CPU_TYPE_X86_64, CPU_SUBTYPE_X86_64_H) => "x86_64h",
        (CPU_TYPE_X86_64, _) => "x86_64",
        (CPU_TYPE_ARM64, CPU_SUBTYPE_ARM64E) => "arm64e",
        (CPU_TYPE_ARM64, _) => "arm64",
        _ => "unknown",
    }
}

/// Whether a Mach-O file of `filetype` built for `cputype` and
/// `cpusubtype` is one the link takes: an object must be for exactly its
/// architecture, while a dylib serves every link of its CPU type (an
/// arm64e one an arm64 link too).
fn takes_arch<E: Target>(filetype: u32, cputype: u32, cpusubtype: u32) -> bool {
    match filetype {
        MH_DYLIB => cputype == E::CPUTYPE,
        _ => arch_name(cputype, cpusubtype) == E::NAME,
    }
}

/// The architecture of a thin object or dylib the link doesn't take
/// (see takes_arch), which ld-prime ignores with a warning.
pub fn foreign_arch<E: Target>(mf: &MappedFile) -> Option<&'static str> {
    let hdr = MachHeader::parse(mf.data());
    let takes = takes_arch::<E>(hdr.filetype.get(), hdr.cputype.get(), hdr.cpusubtype.get());
    (!takes).then(|| arch_name(hdr.cputype.get(), hdr.cpusubtype.get()))
}

/// Whether a thin file the link doesn't take is of its CPU type all the
/// same, an x86_64h object in an x86_64 link: -allow_sub_type_mismatches
/// has ld-prime take it, but for arm64e, whose pointers are signed.
pub fn is_subtype_mismatch<E: Target>(mf: &MappedFile) -> bool {
    let hdr = MachHeader::parse(mf.data());
    hdr.cputype.get() == E::CPUTYPE
        && arch_name(hdr.cputype.get(), hdr.cpusubtype.get()) != "arm64e"
}

/// A fat (universal) file's slices: each one's CPU type, subtype, file
/// offset and size. Fat headers are big-endian.
fn fat_arches(mf: &MappedFile) -> impl Iterator<Item = (u32, u32, usize, usize)> + '_ {
    let data = mf.data();
    let read_be32 = |off: usize| u32::from_be_bytes(data[off..off + 4].try_into().unwrap());
    (0..read_be32(4) as usize).map(move |i| {
        let off = 8 + i * 20;
        (
            read_be32(off),
            read_be32(off + 4),
            read_be32(off + 8) as usize,
            read_be32(off + 12) as usize,
        )
    })
}

/// The architectures a fat file has slices for.
pub fn fat_arch_names(mf: &MappedFile) -> Vec<&'static str> {
    fat_arches(mf).map(|(cputype, cpusubtype, _, _)| arch_name(cputype, cpusubtype)).collect()
}

/// The slice of a fat file the link takes (see takes_arch), if any: the
/// one for exactly its architecture first, but for a dylib whose subtype
/// must match (Args::dylib_subtypes_must_match). With
/// -allow_sub_type_mismatches, one of another subtype of its CPU type
/// will do too, as for a thin file (see is_subtype_mismatch).
pub fn fat_slice<E: Target>(
    args: &crate::cmdline::Args,
    mf: &'static MappedFile,
) -> Option<&'static MappedFile> {
    let slices: Vec<_> = fat_arches(mf).collect();
    let (_, _, off, size) = slices
        .iter()
        .find(|&&(cputype, cpusubtype, _, _)| arch_name(cputype, cpusubtype) == E::NAME)
        .or_else(|| {
            slices.iter().find(|&&(cputype, cpusubtype, off, _)| {
                let filetype = MachHeader::parse(&mf.data()[off..]).filetype;
                !args.dylib_subtypes_must_match
                    && takes_arch::<E>(filetype.get(), cputype, cpusubtype)
            })
        })
        .or_else(|| {
            slices.iter().find(|&&(cputype, cpusubtype, _, _)| {
                args.allow_sub_type_mismatches
                    && cputype == E::CPUTYPE
                    && arch_name(cputype, cpusubtype) != "arm64e"
            })
        })
        .copied()?;
    let mut name = std::ffi::OsString::from(&mf.name);
    name.push(format!("(for architecture {})", E::NAME));
    Some(mf.slice(name.into(), off, size))
}

/// Splits the name fat_slice gives a fat file's slice into the file's
/// path and the slice's architecture.
pub fn split_fat_arch(name: &[u8]) -> (&[u8], Option<&[u8]>) {
    const TAG: &[u8] = b"(for architecture ";
    match memchr::memmem::find(name, TAG) {
        Some(i) if name.ends_with(b")") => (&name[..i], Some(&name[i + TAG.len()..name.len() - 1])),
        _ => (name, None),
    }
}

/// A file's name without the "(for architecture ...)" that fat_slice
/// gives a fat file's slice, which ld-prime never shows: it names the
/// slice, and the members of a fat archive, by the file's own path.
pub fn without_fat_arch(name: &[u8]) -> Vec<u8> {
    let mut name = name.to_vec();
    if let Some(i) = memchr::memmem::find(&name, b"(for architecture")
        && let Some(len) = name[i..].iter().position(|&c| c == b')')
    {
        name.drain(i..=i + len);
    }
    name
}
