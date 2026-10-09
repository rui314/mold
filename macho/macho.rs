//! Mach-O file format definitions.
//!
//! All Mach-O targets we support (arm64 and x86-64) are little-endian.
//! Integer fields are the byte-backed [`U16`], [`U32`] and [`U64`], which
//! read and write little-endian at any alignment, as the ELF linker's
//! integers read and write in their target's byte order. The output thus
//! doesn't depend on the host's byte order, and records have alignment
//! one: the big tables - symbols and relocations - are viewed in the input
//! files rather than copied, even in archive members, which may be
//! misaligned.
//!
//! The exception is code signatures: their data structures are big-endian,
//! and are serialized by hand in the code-signature chunk.

pub use mold_common::record::{
    FileRecord, record_from_bytes, records_from_bytes, records_from_bytes_mut,
};

pub use crate::macho_consts::*;

/// A little-endian integer, at any alignment.
macro_rules! le_integer {
    ($name:ident, $int:ty) => {
        #[repr(transparent)]
        #[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
        pub struct $name([u8; size_of::<$int>()]);

        impl $name {
            #[inline(always)]
            pub const fn new(value: $int) -> Self {
                Self(value.to_le_bytes())
            }

            #[inline(always)]
            pub const fn get(&self) -> $int {
                <$int>::from_le_bytes(self.0)
            }

            #[inline(always)]
            pub fn set(&mut self, value: $int) {
                self.0 = value.to_le_bytes();
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                self.get().fmt(f)
            }
        }

        // SAFETY: a transparent wrapper around a byte array.
        unsafe impl FileRecord for $name {}
    };
}

le_integer!(U16, u16);
le_integer!(U32, u32);
le_integer!(U64, u64);

/// Returns the bytes of a 16-byte, NUL-padded section or segment name.
/// A name is bytes, not text: ld-prime takes any but NUL, UTF-8 or
/// not, and writes them to the output as they came.
pub fn name_to_bytes(name: &[u8; 16]) -> &[u8] {
    let len = name.iter().position(|&b| b == 0).unwrap_or(16);
    &name[..len]
}

/// Whether a 16-byte, NUL-padded section or segment name is `s`.
#[inline]
fn name_is(name: &[u8; 16], s: &[u8]) -> bool {
    name.starts_with(s) && name.get(s.len()).is_none_or(|&b| b == 0)
}

/// Converts bytes to a 16-byte, NUL-padded section or segment name.
pub fn bytes_to_name(s: &[u8]) -> [u8; 16] {
    let mut name = [0; 16];
    name[..s.len()].copy_from_slice(s);
    name
}

/// A section or segment name an option gives, cut to fit the 16 bytes
/// of a header's name field - inside a UTF-8 character too, as
/// ld-prime cuts it.
pub fn cut_name(name: &[u8]) -> &[u8] {
    &name[..name.len().min(16)]
}

/// The file header, which Apple calls `mach_header_64`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MachHeader {
    pub magic: U32,
    pub cputype: U32,
    pub cpusubtype: U32,
    pub filetype: U32,
    pub ncmds: U32,
    pub sizeofcmds: U32,
    pub flags: U32,
    pub reserved: U32,
}

// SAFETY: all fields are byte-backed integers or bytes, and `repr(C)` does
// not insert padding between fields with alignment one.
unsafe impl FileRecord for MachHeader {}

const _: () = assert!(size_of::<MachHeader>() == 32 && align_of::<MachHeader>() == 1);

/// The fields every load command starts with.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LoadCommand {
    pub cmd: U32,
    pub cmdsize: U32,
}

// SAFETY: all fields are byte-backed integers or bytes, and `repr(C)` does
// not insert padding between fields with alignment one.
unsafe impl FileRecord for LoadCommand {}

const _: () = assert!(size_of::<LoadCommand>() == 8 && align_of::<LoadCommand>() == 1);

/// LC_SEGMENT_64, which `nsects` section headers follow.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SegmentCommand {
    pub cmd: U32,
    pub cmdsize: U32,
    pub segname: [u8; 16],
    pub vmaddr: U64,
    pub vmsize: U64,
    pub fileoff: U64,
    pub filesize: U64,
    pub maxprot: U32,
    pub initprot: U32,
    pub nsects: U32,
    pub flags: U32,
}

// SAFETY: all fields are byte-backed integers or bytes, and `repr(C)` does
// not insert padding between fields with alignment one.
unsafe impl FileRecord for SegmentCommand {}

const _: () = assert!(size_of::<SegmentCommand>() == 72 && align_of::<SegmentCommand>() == 1);

/// A section header, which Apple calls `section_64`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MachSection {
    pub sectname: [u8; 16],
    pub segname: [u8; 16],
    pub addr: U64,
    pub size: U64,
    pub offset: U32,
    pub p2align: U32,
    pub reloff: U32,
    pub nreloc: U32,
    pub flags: U32,
    pub reserved1: U32,
    pub reserved2: U32,
    pub reserved3: U32,
}

// SAFETY: all fields are byte-backed integers or bytes, and `repr(C)` does
// not insert padding between fields with alignment one.
unsafe impl FileRecord for MachSection {}

const _: () = assert!(size_of::<MachSection>() == 80 && align_of::<MachSection>() == 1);

impl MachSection {
    pub fn sectname(&self) -> &[u8] {
        name_to_bytes(&self.sectname)
    }

    pub fn segname(&self) -> &[u8] {
        name_to_bytes(&self.segname)
    }

    /// Whether the section is named `s`, as sectname() == s says but
    /// without finding the name's end: for loops over millions of
    /// symbols.
    #[inline]
    pub fn sectname_is(&self, s: &[u8]) -> bool {
        name_is(&self.sectname, s)
    }

    /// Whether the segment is named `s`; see sectname_is.
    #[inline]
    pub fn segname_is(&self, s: &[u8]) -> bool {
        name_is(&self.segname, s)
    }

    pub fn section_type(&self) -> u32 {
        self.flags.get() & SECTION_TYPE
    }

    /// The section's relocation records in `data`, its file's contents.
    pub fn relocs<'a>(&self, data: &'a [u8]) -> &'a [MachRel] {
        let off = self.reloff.get() as usize;
        records_from_bytes(&data[off..][..self.nreloc.get() as usize * size_of::<MachRel>()])
    }

    /// Whether the section's contents are zeros, whatever the file
    /// holds: S_GB_ZEROFILL, zero fill a 32-bit image could place past
    /// 4GB, is zero fill as well.
    pub fn is_zerofill(&self) -> bool {
        matches!(self.section_type(), S_ZEROFILL | S_GB_ZEROFILL | S_THREAD_LOCAL_ZEROFILL)
    }
}

/// LC_SYMTAB: where the symbol table and its string table are.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SymtabCommand {
    pub cmd: U32,
    pub cmdsize: U32,
    pub symoff: U32,
    pub nsyms: U32,
    pub stroff: U32,
    pub strsize: U32,
}

// SAFETY: all fields are byte-backed integers or bytes, and `repr(C)` does
// not insert padding between fields with alignment one.
unsafe impl FileRecord for SymtabCommand {}

const _: () = assert!(size_of::<SymtabCommand>() == 24 && align_of::<SymtabCommand>() == 1);

/// LC_DYSYMTAB: how the symbol table is partitioned, and the indirect
/// symbol table and the external and local relocations.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DysymtabCommand {
    pub cmd: U32,
    pub cmdsize: U32,
    pub ilocalsym: U32,
    pub nlocalsym: U32,
    pub iextdefsym: U32,
    pub nextdefsym: U32,
    pub iundefsym: U32,
    pub nundefsym: U32,
    pub tocoff: U32,
    pub ntoc: U32,
    pub modtaboff: U32,
    pub nmodtab: U32,
    pub extrefsymoff: U32,
    pub nextrefsyms: U32,
    pub indirectsymoff: U32,
    pub nindirectsyms: U32,
    pub extreloff: U32,
    pub nextrel: U32,
    pub locreloff: U32,
    pub nlocrel: U32,
}

// SAFETY: all fields are byte-backed integers or bytes, and `repr(C)` does
// not insert padding between fields with alignment one.
unsafe impl FileRecord for DysymtabCommand {}

const _: () = assert!(size_of::<DysymtabCommand>() == 80 && align_of::<DysymtabCommand>() == 1);

/// LC_ID_DYLIB, LC_LOAD_DYLIB and their kin: a dylib, by its install
/// name at `nameoff`, and its versions.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DylibCommand {
    pub cmd: U32,
    pub cmdsize: U32,
    pub nameoff: U32,
    pub timestamp: U32,
    pub current_version: U32,
    pub compatibility_version: U32,
}

// SAFETY: all fields are byte-backed integers or bytes, and `repr(C)` does
// not insert padding between fields with alignment one.
unsafe impl FileRecord for DylibCommand {}

const _: () = assert!(size_of::<DylibCommand>() == 24 && align_of::<DylibCommand>() == 1);

/// LC_LOAD_DYLINKER and its kin: a path at `nameoff`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DylinkerCommand {
    pub cmd: U32,
    pub cmdsize: U32,
    pub nameoff: U32,
}

// SAFETY: all fields are byte-backed integers or bytes, and `repr(C)` does
// not insert padding between fields with alignment one.
unsafe impl FileRecord for DylinkerCommand {}

const _: () = assert!(size_of::<DylinkerCommand>() == 12 && align_of::<DylinkerCommand>() == 1);

/// LC_UUID.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct UuidCommand {
    pub cmd: U32,
    pub cmdsize: U32,
    pub uuid: [u8; 16],
}

// SAFETY: all fields are byte-backed integers or bytes, and `repr(C)` does
// not insert padding between fields with alignment one.
unsafe impl FileRecord for UuidCommand {}

const _: () = assert!(size_of::<UuidCommand>() == 24 && align_of::<UuidCommand>() == 1);

/// LC_BUILD_VERSION, which `ntools` tool versions follow.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct BuildVersionCommand {
    pub cmd: U32,
    pub cmdsize: U32,
    pub platform: U32,
    pub minos: U32,
    pub sdk: U32,
    pub ntools: U32,
}

// SAFETY: all fields are byte-backed integers or bytes, and `repr(C)` does
// not insert padding between fields with alignment one.
unsafe impl FileRecord for BuildVersionCommand {}

const _: () =
    assert!(size_of::<BuildVersionCommand>() == 24 && align_of::<BuildVersionCommand>() == 1);

/// LC_VERSION_MIN_MACOSX and its kin, which LC_BUILD_VERSION replaced.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VersionMinCommand {
    pub cmd: U32,
    pub cmdsize: U32,
    pub version: U32,
    pub sdk: U32,
}

// SAFETY: all fields are byte-backed integers or bytes, and `repr(C)` does
// not insert padding between fields with alignment one.
unsafe impl FileRecord for VersionMinCommand {}

const _: () = assert!(size_of::<VersionMinCommand>() == 16 && align_of::<VersionMinCommand>() == 1);

/// LC_SOURCE_VERSION.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SourceVersionCommand {
    pub cmd: U32,
    pub cmdsize: U32,
    pub version: U64,
}

// SAFETY: all fields are byte-backed integers or bytes, and `repr(C)` does
// not insert padding between fields with alignment one.
unsafe impl FileRecord for SourceVersionCommand {}

const _: () =
    assert!(size_of::<SourceVersionCommand>() == 16 && align_of::<SourceVersionCommand>() == 1);

/// LC_MAIN: the entry point, as an offset in the file.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct EntryPointCommand {
    pub cmd: U32,
    pub cmdsize: U32,
    pub entryoff: U64,
    pub stacksize: U64,
}

// SAFETY: all fields are byte-backed integers or bytes, and `repr(C)` does
// not insert padding between fields with alignment one.
unsafe impl FileRecord for EntryPointCommand {}

const _: () = assert!(size_of::<EntryPointCommand>() == 24 && align_of::<EntryPointCommand>() == 1);

/// LC_ROUTINES_64: the image's -init function, by its unslid address;
/// the fields after it were for the long-gone multi-module dylibs.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RoutinesCommand64 {
    pub cmd: U32,
    pub cmdsize: U32,
    pub init_address: U64,
    pub init_module: U64,
    pub reserved: [U64; 6],
}

// SAFETY: all fields are byte-backed integers or bytes, and `repr(C)` does
// not insert padding between fields with alignment one.
unsafe impl FileRecord for RoutinesCommand64 {}

const _: () = assert!(size_of::<RoutinesCommand64>() == 72 && align_of::<RoutinesCommand64>() == 1);

/// LC_CODE_SIGNATURE, LC_FUNCTION_STARTS and the other load commands
/// that point at a blob of LINKEDIT data.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LinkEditDataCommand {
    pub cmd: U32,
    pub cmdsize: U32,
    pub dataoff: U32,
    pub datasize: U32,
}

// SAFETY: all fields are byte-backed integers or bytes, and `repr(C)` does
// not insert padding between fields with alignment one.
unsafe impl FileRecord for LinkEditDataCommand {}

const _: () =
    assert!(size_of::<LinkEditDataCommand>() == 16 && align_of::<LinkEditDataCommand>() == 1);

/// LC_DYLD_INFO and LC_DYLD_INFO_ONLY: where the rebase, bind and export
/// information is.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DyldInfoCommand {
    pub cmd: U32,
    pub cmdsize: U32,
    pub rebase_off: U32,
    pub rebase_size: U32,
    pub bind_off: U32,
    pub bind_size: U32,
    pub weak_bind_off: U32,
    pub weak_bind_size: U32,
    pub lazy_bind_off: U32,
    pub lazy_bind_size: U32,
    pub export_off: U32,
    pub export_size: U32,
}

// SAFETY: all fields are byte-backed integers or bytes, and `repr(C)` does
// not insert padding between fields with alignment one.
unsafe impl FileRecord for DyldInfoCommand {}

const _: () = assert!(size_of::<DyldInfoCommand>() == 48 && align_of::<DyldInfoCommand>() == 1);

/// A symbol table entry, which Apple calls `nlist_64`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MachSym {
    pub stroff: U32,
    pub n_type: u8,
    pub sect: u8,
    pub desc: U16,
    pub value: U64,
}

// SAFETY: all fields are byte-backed integers or bytes, and `repr(C)` does
// not insert padding between fields with alignment one.
unsafe impl FileRecord for MachSym {}

const _: () = assert!(size_of::<MachSym>() == 16 && align_of::<MachSym>() == 1);

impl MachSym {
    pub fn is_stab(&self) -> bool {
        self.n_type & N_STAB != 0
    }

    pub fn is_extern(&self) -> bool {
        self.n_type & N_EXT != 0
    }

    pub fn ty(&self) -> u8 {
        self.n_type & N_TYPE
    }

    pub fn is_common(&self) -> bool {
        !self.is_stab() && self.ty() == N_UNDF && self.is_extern() && self.value.get() != 0
    }

    /// Whether an external symbol is undefined: a reference, or a
    /// tentative definition (see is_common), which is N_UNDF too.
    pub fn is_undef(&self) -> bool {
        !self.is_stab() && self.is_extern() && self.ty() == N_UNDF
    }

    /// Whether a MachSym is an external weak definition in a section.
    pub fn is_weak_def(&self) -> bool {
        !self.is_stab()
            && self.is_extern()
            && self.ty() == N_SECT
            && self.desc.get() & N_WEAK_DEF != 0
    }

    /// The log2 of a tentative definition's alignment, which desc
    /// carries in bits 8 to 11 (Apple's GET_COMM_ALIGN).
    pub fn common_p2align(&self) -> u8 {
        ((self.desc.get() >> 8) & 0xf) as u8
    }
}

/// A relocation record, which Apple calls `relocation_info`. `offset`
/// is followed by a bitfield laid out, from the least significant bit,
/// as idx:24, pcrel:1, p2size:2, extern:1, type:4.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MachRel {
    pub offset: U32,
    pub bits: U32,
}

// SAFETY: all fields are byte-backed integers or bytes, and `repr(C)` does
// not insert padding between fields with alignment one.
unsafe impl FileRecord for MachRel {}

const _: () = assert!(size_of::<MachRel>() == 8 && align_of::<MachRel>() == 1);

impl MachRel {
    pub fn idx(&self) -> u32 {
        self.bits.get() & 0xff_ffff
    }

    /// The 1-based ordinal of the section a non-extern record refers
    /// to: idx's low byte, as a MachSym's sect is one byte.
    /// ld-prime ignores the rest of the field.
    pub fn sect(&self) -> u32 {
        self.bits.get() & 0xff
    }

    pub fn is_pcrel(&self) -> bool {
        self.bits.get() & (1 << 24) != 0
    }

    /// log2 of the size of the relocated field: 0, 1, 2 or 3.
    pub fn p2size(&self) -> u32 {
        (self.bits.get() >> 25) & 3
    }

    pub fn is_extern(&self) -> bool {
        self.bits.get() & (1 << 27) != 0
    }

    pub fn ty(&self) -> u8 {
        (self.bits.get() >> 28) as u8
    }
}

/// Encodes an X.Y.Z version number for a load command.
pub const fn encode_version(major: u32, minor: u32, patch: u32) -> u32 {
    (major << 16) | (minor << 8) | patch
}

/// Formats a load command's version, omitting a zero patch level.
pub fn format_version(version: u32) -> String {
    let major = version >> 16;
    let minor = (version >> 8) & 0xff;
    let patch = version & 0xff;
    if patch == 0 { format!("{major}.{minor}") } else { format!("{major}.{minor}.{patch}") }
}

pub fn platform_name(platform: u32) -> String {
    match platform {
        PLATFORM_MACOS => "macOS",
        PLATFORM_IOS => "iOS",
        PLATFORM_TVOS => "tvOS",
        PLATFORM_WATCHOS => "watchOS",
        PLATFORM_BRIDGEOS => "bridgeOS",
        PLATFORM_MACCATALYST => "macCatalyst",
        PLATFORM_IOSSIMULATOR => "iOS-simulator",
        PLATFORM_TVOSSIMULATOR => "tvOS-simulator",
        PLATFORM_WATCHOSSIMULATOR => "watchOS-simulator",
        PLATFORM_DRIVERKIT => "driverKit",
        PLATFORM_VISIONOS => "visionOS",
        PLATFORM_VISIONOSSIMULATOR => "visionOS-simulator",
        PLATFORM_FIRMWARE => "firmware",
        PLATFORM_SEPOS => "sepOS",
        _ => return format!("unknown platform ({platform})"),
    }
    .to_string()
}

/// Whether mold links for a platform: macOS, iOS, tvOS and visionOS,
/// on devices and in their simulators, and firmware. (A watchOS device
/// runs arm64_32 code, an ILP32 target mold has no port to; Mac
/// Catalyst, driverKit and bridgeOS aren't supported either.)
pub fn is_supported_platform(platform: u32) -> bool {
    matches!(
        platform,
        PLATFORM_MACOS
            | PLATFORM_IOS
            | PLATFORM_IOSSIMULATOR
            | PLATFORM_TVOS
            | PLATFORM_TVOSSIMULATOR
            | PLATFORM_VISIONOS
            | PLATFORM_VISIONOSSIMULATOR
            | PLATFORM_FIRMWARE
    )
}

/// Whether a platform is a simulator's: a mobile OS's processes running
/// on a Mac, on the Mac's own CPU, against the simulator SDK's libraries.
pub fn is_simulator(platform: u32) -> bool {
    matches!(
        platform,
        PLATFORM_IOSSIMULATOR
            | PLATFORM_TVOSSIMULATOR
            | PLATFORM_WATCHOSSIMULATOR
            | PLATFORM_VISIONOSSIMULATOR
    )
}

/// The platforms a .tbd file has targets for, for a diagnostic.
pub fn platforms_name(platforms: &[u32]) -> String {
    platforms.iter().map(|&p| platform_name(p)).collect::<Vec<_>>().join(" ")
}
