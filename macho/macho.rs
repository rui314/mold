//! Mach-O file format definitions.
//!
//! All Mach-O targets we support (arm64 and x86-64) are little-endian, so
//! records are defined with plain integer fields and read from and written
//! to file buffers with unaligned copies. Structures in archive members may
//! be misaligned in memory, so records are never referenced in place.
//!
//! The exception is code signatures: their data structures are big-endian,
//! and are serialized by hand in the code-signature chunk.

pub use crate::macho_consts::*;

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
        unsafe { std::ptr::read_unaligned(buf.as_ptr().cast::<Self>()) }
    }

    fn write_to(&self, buf: &mut [u8]) {
        assert!(buf.len() >= size_of::<Self>());
        // SAFETY: the buffer is large enough.
        unsafe { std::ptr::write_unaligned(buf.as_mut_ptr().cast::<Self>(), *self) }
    }

    fn as_bytes(&self) -> &[u8] {
        // SAFETY: the record is repr(C) with no padding.
        unsafe {
            std::slice::from_raw_parts(
                std::ptr::from_ref::<Self>(self).cast::<u8>(),
                size_of::<Self>(),
            )
        }
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

/// Writes records one after another starting at `off`.
pub fn write_array<T: FileRecord>(buf: &mut [u8], off: usize, records: &[T]) {
    for (i, record) in records.iter().enumerate() {
        record.write_to(&mut buf[off + i * size_of::<T>()..]);
    }
}

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
        self.flags & SECTION_TYPE
    }

    /// Whether the section's contents are zeros, whatever the file
    /// holds: S_GB_ZEROFILL, zero fill a 32-bit image could place past
    /// 4GB, is zero fill as well.
    pub fn is_zerofill(&self) -> bool {
        matches!(self.section_type(), S_ZEROFILL | S_GB_ZEROFILL | S_THREAD_LOCAL_ZEROFILL)
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

/// LC_ROUTINES_64: the image's -init function, by its unslid address;
/// the fields after it were for the long-gone multi-module dylibs.
#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct RoutinesCommand64 {
    pub cmd: u32,
    pub cmdsize: u32,
    pub init_address: u64,
    pub init_module: u64,
    pub reserved: [u64; 6],
}

unsafe impl FileRecord for RoutinesCommand64 {}

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

/// A symbol table entry, which Apple calls `nlist_64`.
#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct MachSym {
    pub stroff: u32,
    pub n_type: u8,
    pub sect: u8,
    pub desc: u16,
    pub value: u64,
}

unsafe impl FileRecord for MachSym {}

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
        !self.is_stab() && self.ty() == N_UNDF && self.is_extern() && self.value != 0
    }
}

/// A relocation record, which Apple calls `relocation_info`. `offset`
/// is followed by a bitfield laid out, from the least significant bit,
/// as idx:24, pcrel:1, p2size:2, extern:1, type:4.
#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct MachRel {
    pub offset: u32,
    pub bits: u32,
}

unsafe impl FileRecord for MachRel {}

impl MachRel {
    pub fn idx(&self) -> u32 {
        self.bits & 0xff_ffff
    }

    /// The 1-based ordinal of the section a non-extern record refers
    /// to: idx's low byte, as a MachSym's sect is one byte.
    /// ld-prime ignores the rest of the field.
    pub fn sect(&self) -> u32 {
        self.bits & 0xff
    }

    pub fn is_pcrel(&self) -> bool {
        self.bits & (1 << 24) != 0
    }

    /// log2 of the size of the relocated field: 0, 1, 2 or 3.
    pub fn p2size(&self) -> u32 {
        (self.bits >> 25) & 3
    }

    pub fn is_extern(&self) -> bool {
        self.bits & (1 << 27) != 0
    }

    pub fn ty(&self) -> u8 {
        (self.bits >> 28) as u8
    }
}

/// A season's releases of Apple's OSes, which ld64 names its version
/// sets after (ld::version2019Fall and the like): it turns a default on
/// for every deployment target at or past such a set, as dyld and the
/// runtimes of that season support what the default makes. tvOS is
/// numbered as iOS is, and a simulator as its device; visionOS, which
/// came later, has every default up to its own first release. Firmware,
/// which no OS release governs, has the defaults of the older sets.
pub struct VersionSet {
    macos: u32,
    ios: u32,
    visionos: u32,
    firmware: bool,
}

pub const VERSION_2012_FALL: VersionSet = VersionSet::new((10, 8), (6, 0), (0, 0), true);
pub const VERSION_2013_FALL: VersionSet = VersionSet::new((10, 9), (7, 0), (0, 0), true);
pub const VERSION_2018_FALL: VersionSet = VersionSet::new((10, 14), (12, 0), (0, 0), true);
pub const VERSION_2019_FALL: VersionSet = VersionSet::new((10, 15), (13, 0), (0, 0), true);
pub const VERSION_2020_FALL: VersionSet = VersionSet::new((11, 0), (14, 0), (0, 0), true);
pub const VERSION_2021_FALL: VersionSet = VersionSet::new((12, 0), (15, 0), (0, 0), true);
pub const VERSION_2024_SPRING: VersionSet = VersionSet::new((14, 4), (17, 4), (1, 1), false);
pub const VERSION_2024_FALL: VersionSet = VersionSet::new((15, 0), (18, 0), (2, 0), false);
pub const VERSION_2026_FALL: VersionSet = VersionSet::new((27, 0), (27, 0), (27, 0), false);

impl VersionSet {
    /// The set of macOS, iOS and visionOS (major, minor) versions.
    const fn new(macos: (u32, u32), ios: (u32, u32), visionos: (u32, u32), firmware: bool) -> Self {
        Self {
            macos: encode_version(macos.0, macos.1, 0),
            ios: encode_version(ios.0, ios.1, 0),
            visionos: encode_version(visionos.0, visionos.1, 0),
            firmware,
        }
    }

    /// Whether a deployment target is at or past this set: never for
    /// a -r or -preload output linked for no platform.
    pub fn reached_by(&self, platform: u32, minos: u32) -> bool {
        match platform {
            PLATFORM_MACOS => minos >= self.macos,
            PLATFORM_IOS | PLATFORM_IOSSIMULATOR | PLATFORM_TVOS | PLATFORM_TVOSSIMULATOR => {
                minos >= self.ios
            }
            PLATFORM_VISIONOS | PLATFORM_VISIONOSSIMULATOR => minos >= self.visionos,
            PLATFORM_FIRMWARE => self.firmware,
            _ => false,
        }
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
