//! Mach-O file format definitions.
//!
//! All Mach-O targets we support (arm64 and x86-64) are little-endian, so
//! records are defined with plain integer fields and read from and written
//! to file buffers with unaligned copies. Structures in archive members may
//! be misaligned in memory, so records are never referenced in place.
//!
//! The exception is code signatures: their data structures are big-endian,
//! and are serialized by hand in the code-signature chunk.

pub mod consts;

pub use consts::*;

/// A record that can be copied between memory and a file buffer.
///
/// # Safety
///
/// Implementors must be `#[repr(C)]` with no padding and valid for any bit
/// pattern.
pub unsafe trait FileRecord: Copy + Default {
    fn read_from(buf: &[u8]) -> Self {
        assert!(buf.len() >= size_of::<Self>());
        // SAFETY: the buffer is large enough, and any bit pattern is a
        // valid value of the record type.
        unsafe { std::ptr::read_unaligned(buf.as_ptr() as *const Self) }
    }

    fn write_to(&self, buf: &mut [u8]) {
        assert!(buf.len() >= size_of::<Self>());
        // SAFETY: the buffer is large enough.
        unsafe { std::ptr::write_unaligned(buf.as_mut_ptr() as *mut Self, *self) }
    }

    fn as_bytes(&self) -> &[u8] {
        // SAFETY: the record is repr(C) with no padding.
        unsafe { std::slice::from_raw_parts(self as *const Self as *const u8, size_of::<Self>()) }
    }
}

/// Reads an array of `n` records starting at `off`.
pub fn read_array<T: FileRecord>(buf: &[u8], off: usize, n: usize) -> Vec<T> {
    let mut vec = Vec::with_capacity(n);
    for i in 0..n {
        vec.push(T::read_from(&buf[off + i * size_of::<T>()..]));
    }
    vec
}

/// Returns a 16-byte, NUL-padded section or segment name as a string.
pub fn name_to_str(name: &[u8; 16]) -> &str {
    let len = name.iter().position(|&b| b == 0).unwrap_or(16);
    std::str::from_utf8(&name[..len]).unwrap_or("")
}

/// Converts a string to a 16-byte, NUL-padded section or segment name.
pub fn str_to_name(s: &str) -> [u8; 16] {
    let mut name = [0; 16];
    name[..s.len()].copy_from_slice(s.as_bytes());
    name
}

#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct MachHeader {
    pub magic: u32,
    pub cputype: u32,
    pub cpusubtype: u32,
    pub filetype: u32,
    pub ncmds: u32,
    pub sizeofcmds: u32,
    pub flags: u32,
    pub reserved: u32,
}

unsafe impl FileRecord for MachHeader {}

#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct LoadCommand {
    pub cmd: u32,
    pub cmdsize: u32,
}

unsafe impl FileRecord for LoadCommand {}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct SegmentCommand {
    pub cmd: u32,
    pub cmdsize: u32,
    pub segname: [u8; 16],
    pub vmaddr: u64,
    pub vmsize: u64,
    pub fileoff: u64,
    pub filesize: u64,
    pub maxprot: u32,
    pub initprot: u32,
    pub nsects: u32,
    pub flags: u32,
}

unsafe impl FileRecord for SegmentCommand {}

impl Default for SegmentCommand {
    fn default() -> Self {
        // SAFETY: all-zero bytes are a valid value for a plain record.
        unsafe { std::mem::zeroed() }
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct MachSection {
    pub sectname: [u8; 16],
    pub segname: [u8; 16],
    pub addr: u64,
    pub size: u64,
    pub offset: u32,
    pub p2align: u32,
    pub reloff: u32,
    pub nreloc: u32,
    pub flags: u32,
    pub reserved1: u32,
    pub reserved2: u32,
    pub reserved3: u32,
}

unsafe impl FileRecord for MachSection {}

impl Default for MachSection {
    fn default() -> Self {
        // SAFETY: all-zero bytes are a valid value for a plain record.
        unsafe { std::mem::zeroed() }
    }
}

impl MachSection {
    pub fn sectname(&self) -> &str {
        name_to_str(&self.sectname)
    }

    pub fn segname(&self) -> &str {
        name_to_str(&self.segname)
    }

    pub fn section_type(&self) -> u32 {
        self.flags & SECTION_TYPE
    }
}

#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct SymtabCommand {
    pub cmd: u32,
    pub cmdsize: u32,
    pub symoff: u32,
    pub nsyms: u32,
    pub stroff: u32,
    pub strsize: u32,
}

unsafe impl FileRecord for SymtabCommand {}

#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct DysymtabCommand {
    pub cmd: u32,
    pub cmdsize: u32,
    pub ilocalsym: u32,
    pub nlocalsym: u32,
    pub iextdefsym: u32,
    pub nextdefsym: u32,
    pub iundefsym: u32,
    pub nundefsym: u32,
    pub tocoff: u32,
    pub ntoc: u32,
    pub modtaboff: u32,
    pub nmodtab: u32,
    pub extrefsymoff: u32,
    pub nextrefsyms: u32,
    pub indirectsymoff: u32,
    pub nindirectsyms: u32,
    pub extreloff: u32,
    pub nextrel: u32,
    pub locreloff: u32,
    pub nlocrel: u32,
}

unsafe impl FileRecord for DysymtabCommand {}

#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct DylibCommand {
    pub cmd: u32,
    pub cmdsize: u32,
    pub nameoff: u32,
    pub timestamp: u32,
    pub current_version: u32,
    pub compatibility_version: u32,
}

unsafe impl FileRecord for DylibCommand {}

#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct DylinkerCommand {
    pub cmd: u32,
    pub cmdsize: u32,
    pub nameoff: u32,
}

unsafe impl FileRecord for DylinkerCommand {}

#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct UuidCommand {
    pub cmd: u32,
    pub cmdsize: u32,
    pub uuid: [u8; 16],
}

unsafe impl FileRecord for UuidCommand {}

#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct BuildVersionCommand {
    pub cmd: u32,
    pub cmdsize: u32,
    pub platform: u32,
    pub minos: u32,
    pub sdk: u32,
    pub ntools: u32,
}

unsafe impl FileRecord for BuildVersionCommand {}

#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct VersionMinCommand {
    pub cmd: u32,
    pub cmdsize: u32,
    pub version: u32,
    pub sdk: u32,
}

unsafe impl FileRecord for VersionMinCommand {}

#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct SourceVersionCommand {
    pub cmd: u32,
    pub cmdsize: u32,
    pub version: u64,
}

unsafe impl FileRecord for SourceVersionCommand {}

#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct EntryPointCommand {
    pub cmd: u32,
    pub cmdsize: u32,
    pub entryoff: u64,
    pub stacksize: u64,
}

unsafe impl FileRecord for EntryPointCommand {}

#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct LinkEditDataCommand {
    pub cmd: u32,
    pub cmdsize: u32,
    pub dataoff: u32,
    pub datasize: u32,
}

unsafe impl FileRecord for LinkEditDataCommand {}

#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct DyldInfoCommand {
    pub cmd: u32,
    pub cmdsize: u32,
    pub rebase_off: u32,
    pub rebase_size: u32,
    pub bind_off: u32,
    pub bind_size: u32,
    pub weak_bind_off: u32,
    pub weak_bind_size: u32,
    pub lazy_bind_off: u32,
    pub lazy_bind_size: u32,
    pub export_off: u32,
    pub export_size: u32,
}

unsafe impl FileRecord for DyldInfoCommand {}

#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct NList {
    pub n_strx: u32,
    pub n_type: u8,
    pub n_sect: u8,
    pub n_desc: u16,
    pub n_value: u64,
}

unsafe impl FileRecord for NList {}

impl NList {
    pub fn is_stab(&self) -> bool {
        self.n_type & N_STAB != 0
    }

    pub fn is_extern(&self) -> bool {
        self.n_type & N_EXT != 0
    }

    pub fn n_type(&self) -> u8 {
        self.n_type & N_TYPE
    }

    pub fn is_undef(&self) -> bool {
        !self.is_stab() && self.n_type() == N_UNDF && self.is_extern()
    }

    pub fn is_common(&self) -> bool {
        !self.is_stab() && self.n_type() == N_UNDF && self.is_extern() && self.n_value != 0
    }
}

/// A relocation record. `r_address` is followed by a bitfield laid out,
/// from the least significant bit, as symbolnum:24, pcrel:1, length:2,
/// extern:1, type:4.
#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct MachRel {
    pub r_address: u32,
    pub bits: u32,
}

unsafe impl FileRecord for MachRel {}

impl MachRel {
    pub fn r_symbolnum(&self) -> u32 {
        self.bits & 0xff_ffff
    }

    pub fn is_pcrel(&self) -> bool {
        self.bits & (1 << 24) != 0
    }

    /// log2 of the size of the relocated field: 0, 1, 2 or 3.
    pub fn r_length(&self) -> u32 {
        (self.bits >> 25) & 3
    }

    pub fn is_extern(&self) -> bool {
        self.bits & (1 << 27) != 0
    }

    pub fn r_type(&self) -> u8 {
        (self.bits >> 28) as u8
    }
}

/// Encodes an X.Y.Z version number for a load command.
pub fn encode_version(major: u32, minor: u32, patch: u32) -> u32 {
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
        PLATFORM_MACCATALYST => "Mac Catalyst",
        PLATFORM_IOSSIMULATOR => "iOS-simulator",
        PLATFORM_TVOSSIMULATOR => "tvOS-simulator",
        PLATFORM_WATCHOSSIMULATOR => "watchOS-simulator",
        PLATFORM_DRIVERKIT => "DriverKit",
        PLATFORM_VISIONOS => "visionOS",
        PLATFORM_VISIONOSSIMULATOR => "visionOS-simulator",
        _ => return format!("unknown platform ({platform})"),
    }
    .to_string()
}
