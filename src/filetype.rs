//! Input file type detection.

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

/// Returns the target name of a Mach-O file, or `None` if it is not a
/// 64-bit Mach-O file for a CPU type we recognize.
pub fn get_macho_target(data: &[u8]) -> Option<&'static str> {
    if data.len() < size_of::<MachHeader>() {
        return None;
    }
    let hdr = MachHeader::read_from(data);
    if hdr.magic != MH_MAGIC_64 {
        return None;
    }
    crate::target::cputype_name(hdr.cputype)
}

/// Returns the file type (MH_EXECUTE, MH_BUNDLE, ...) of a 64-bit
/// Mach-O file, or `None` if it is not one.
pub fn get_macho_filetype(data: &[u8]) -> Option<u32> {
    if data.len() < size_of::<MachHeader>() {
        return None;
    }
    let hdr = MachHeader::read_from(data);
    (hdr.magic == MH_MAGIC_64).then_some(hdr.filetype)
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

    if data.len() >= size_of::<MachHeader>() {
        let hdr = MachHeader::read_from(data);
        if hdr.magic == MH_MAGIC_64 {
            return match hdr.filetype {
                MH_OBJECT => FileType::Object,
                MH_DYLIB => FileType::Dylib,
                _ => FileType::Unknown,
            };
        }
        if hdr.magic.swap_bytes() == FAT_MAGIC {
            return FileType::Fat;
        }
    }

    FileType::Unknown
}
