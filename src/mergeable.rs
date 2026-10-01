//! Mergeable dylibs: the atoms ld-prime records in a dylib it links
//! with -make_mergeable, which a later link's -merge_framework,
//! -merge_library or -merge-l takes in place of a load command.
//!
//! LC_ATOM_INFO points at a blob in __LINKEDIT, in a format of
//! ld-prime's own (its AtomFile_1, magic "nldprecr", file version 3):
//! a header, then tables of 40-byte atoms, 16-byte fixups, large
//! addends, custom sections, symbol names, the dylib's own identity
//! and the dylibs it links, debug notes, and pools of strings and
//! contents. An atom's content is mostly the dylib's own linked bytes,
//! which a negative offset from the content pool reaches back to, so
//! its fixups have to be applied again: a pointer holds a chained fixup
//! there, an instruction its final immediate, a GOT load ld-prime
//! relaxed an add (a leaq on x86-64), while the fixup records what the
//! object had. Compact unwind records, which have no place in the
//! image, are in the content pool as the objects had them.
//!
//! To merge, mold turns the atoms back into the object file they stand
//! for - as an `ld -r` of the dylib's objects would write it, debug
//! notes included - and links that: each atom gets its section (one of
//! ld-prime's standard ones by its content type, or one of the custom
//! table's), its symbol or a private label, and its fixups the
//! relocations the objects had; an import stays undefined, and the
//! dylibs the mergeable one links stand by their install names (see
//! passes::add_merged_dependencies). What ld-prime keeps of the objects
//! is all there is: no data-in-code entries and no optimization hints
//! survive, in its merged images either. make_mergeable writes the
//! record in a dylib linked with -make_mergeable.

use std::path::Path;

use crate::fatal;
use crate::macho::*;
use crate::mapped_file::MappedFile;
use crate::target::Target;

/// What ld-prime's -make_mergeable writes: an atom file whose atoms
/// point into the image around it ("nldpatom" and "nldprdcr" are the
/// plain and the image-backed kinds it reads besides).
pub(crate) const MAGIC: &[u8; 8] = b"nldprecr";

/// The atom file header's size in file version 3; the atoms follow it
/// directly.
pub(crate) const HEADER_SIZE: usize = 0xf8;
pub(crate) const ATOM_SIZE: usize = 40;
pub(crate) const FIXUP_SIZE: usize = 16;
pub(crate) const SECTION_SIZE: usize = 44;
pub(crate) const DYLIB_INFO_SIZE: usize = 0x88;
pub(crate) const DEBUG_INFO_SIZE: usize = 0x98;

/// An atom's kind (bits 3-7 of its flags word).
pub mod kind {
    pub const REGULAR: u8 = 0;
    pub const WEAK_DEF: u8 = 1;
    pub const RESOLVER: u8 = 2;
    pub const ANON: u8 = 3;
    pub const ANON_COAL_BY_CONTENT: u8 = 4;
    pub const TENTATIVE_DEF: u8 = 5;
    pub const ABSOLUTE: u8 = 6;
    pub const DYLIB_EXPORT: u8 = 7;
    pub const DYLIB_EXPORT_WEAK_DEF: u8 = 8;
    pub const DYLIB_EXPORT_FORCE_LOAD: u8 = 9;
    pub const UNDEFINE: u8 = 10;
    pub const UNDEFINE_WEAK_IMPORT: u8 = 11;
    pub const ALIAS: u8 = 12;
    pub const ANON_PLACEHOLDER: u8 = 18;
    pub const WEAK_DEF_ALIAS: u8 = 19;
}

/// An atom's scope (bits 0-2 of its flags word).
pub mod scope {
    pub const HIDDEN: u8 = 1;
    pub const AUTO_HIDE: u8 = 2;
    pub const GLOBAL: u8 = 3;
    pub const NEVER_STRIP: u8 = 4;
}

/// The content types whose atoms need more than their section.
pub(crate) mod ctype {
    pub const CFI: u8 = 31;
    pub const OBJC_METHOD_LIST: u8 = 15;
    pub const OBJC_IMAGE_INFO: u8 = 43;
    pub const INIT_OFFSET: u8 = 55;
}

/// The fixup kinds (ld-prime's Fixup::Kind), generic and per target.
pub(crate) mod fk {
    pub const KEEP_ALIVE: u16 = 1;
    pub const PTR64: u16 = 2;
    pub const PTR32: u16 = 3;
    pub const DIFF32: u16 = 4;
    pub const DIFF64: u16 = 5;
    pub const IMAGE_OFFSET32: u16 = 6;
    pub const PCREL32_TO_GOT: u16 = 7;
    pub const PCREL_DELTA32: u16 = 8;
    pub const SWIFT_REL32_TO_GOT: u16 = 9;
    pub const ALIAS_OF: u16 = 11;
    pub const TLV_OFFSET: u16 = 12;
    pub const PTR64_TO_GOT: u16 = 14;
    pub const ARM64_B26: u16 = 0x80;
    pub const ARM64_B26_ADDEND: u16 = 0x81;
    pub const ARM64_ADRP: u16 = 0x82;
    pub const ARM64_ADRP_ADDEND: u16 = 0x83;
    pub const ARM64_LO12: u16 = 0x84;
    pub const ARM64_LO12_ADDEND: u16 = 0x85;
    pub const ARM64_ADRP_GOT: u16 = 0x86;
    pub const ARM64_LD12_GOT: u16 = 0x87;
    pub const ARM64_ADRP_LO12: u16 = 0x88;
    pub const ARM64_ADRP_LO12_ADDEND: u16 = 0x89;
    pub const ARM64_ADRP_LDR_GOT: u16 = 0x8a;
    pub const ARM64_ADRP_TLV: u16 = 0x8b;
    pub const ARM64_LD12_TLV: u16 = 0x8c;
    pub const ARM64_AUTH_PTR: u16 = 0x8d;
    pub const ARM64_ADD_GOT: u16 = 0x8e;
    pub const ARM64_ADRP_ADD_GOT: u16 = 0x8f;
    pub const ARM64_ADRP_GOT_NO_OPT: u16 = 0x90;
    pub const ARM64_LD12_GOT_NO_OPT: u16 = 0x91;
    pub const ARM64_ADRP_LDR_GOT_NO_OPT: u16 = 0x92;
    pub const X86_64_CALL: u16 = 0x100;
    pub const X86_64_RIP: u16 = 0x101;
    pub const X86_64_RIP_GOT: u16 = 0x102;
    pub const X86_64_RIP1: u16 = 0x103;
    pub const X86_64_RIP2: u16 = 0x104;
    pub const X86_64_RIP4: u16 = 0x105;
    pub const X86_64_RIP_GOT_LOAD: u16 = 0x106;
    pub const X86_64_RIP_TLV_LOAD: u16 = 0x107;
    pub const X86_64_BRANCH8: u16 = 0x108;
    pub const X86_64_RIP1_GOT: u16 = 0x109;
}

/// How a fixup kind uses its last word and where its addend is
/// (ld-prime's Fixup::KindInfo extrasUsage).
pub(crate) fn extras_usage(kind: u16) -> u8 {
    use fk::*;
    match kind {
        0
        | ARM64_B26
        | ARM64_ADRP
        | ARM64_ADRP_GOT
        | ARM64_ADRP_TLV
        | ARM64_ADRP_GOT_NO_OPT
        | X86_64_RIP_GOT
        | X86_64_RIP_GOT_LOAD
        | X86_64_RIP_TLV_LOAD
        | X86_64_RIP1_GOT => 0,
        DIFF32 | DIFF64 => 1,
        ARM64_AUTH_PTR => 3,
        ARM64_B26_ADDEND | ARM64_ADRP_ADDEND => 4,
        ARM64_LO12
        | ARM64_LO12_ADDEND
        | ARM64_LD12_GOT
        | ARM64_LD12_TLV
        | ARM64_ADD_GOT
        | ARM64_LD12_GOT_NO_OPT => 5,
        ARM64_ADRP_LO12
        | ARM64_ADRP_LO12_ADDEND
        | ARM64_ADRP_LDR_GOT
        | ARM64_ADRP_ADD_GOT
        | ARM64_ADRP_LDR_GOT_NO_OPT => 7,
        _ => 2,
    }
}

/// One of the mergeable dylib's atoms.
#[derive(Clone, Debug)]
pub struct Atom {
    pub name: Option<&'static [u8]>,
    pub scope: u8,
    pub kind: u8,
    pub content_type: u8,
    pub cold: bool,
    pub no_dead_strip: bool,
    /// An import's strength: 1 a weak import, 2 a strong one, 0 none
    /// given.
    pub import: u8,
    pub custom_section: Option<usize>,
    pub size: u32,
    /// The bytes, or none for zero fill (or what the linker made).
    pub content: Option<&'static [u8]>,
    /// An import's library, by index into AtomFile::dylibs.
    pub dylib: Option<usize>,
    pub p2align: u8,
    pub modulus: u16,
    /// 1-based index into AtomFile::debug_infos, 0 for none.
    pub debug: u16,
    pub fixups: std::ops::Range<usize>,
}

/// A fixup of an atom: its place, target and addend, and what its kind
/// carries besides - the atom subtracted (a difference), or the second
/// instruction's distance in instructions and the load size (the fused
/// arm64 pairs).
#[derive(Clone, Copy, Debug)]
pub struct Fixup {
    pub offset: u32,
    pub target: u32,
    pub kind: u16,
    pub addend: i64,
    pub from: u32,
    pub other: u8,
}

/// A section that is none of ld-prime's standard ones.
#[derive(Clone, Debug)]
pub struct CustomSection {
    pub segname: [u8; 16],
    pub sectname: [u8; 16],
    pub flags: u32,
}

/// A dylib's identity, as -make_mergeable records the mergeable dylib
/// and each dylib it links.
#[derive(Clone, Debug, Default)]
pub struct DylibInfo {
    pub install_name: Vec<u8>,
    pub current_version: u32,
    pub compatibility_version: u32,
}

/// What a debug note (N_SO, N_OSO) says of the object an atom came from.
#[derive(Clone, Debug)]
pub struct DebugInfo {
    pub mtime: u32,
    pub source_dir: Vec<u8>,
    pub source_name: Vec<u8>,
    pub object_path: Vec<u8>,
}

/// A mergeable dylib's atoms, as LC_ATOM_INFO records them.
pub struct AtomFile {
    pub cputype: u32,
    pub cpusubtype: u32,
    pub platform: u32,
    pub minos: u32,
    pub sdk: u32,
    /// ld-prime's AtomFileFlags: the Swift versions of the objects'
    /// __objc_imageinfo (bits 0-23), MH_SUBSECTIONS_VIA_SYMBOLS (24),
    /// whether there was an image info (26), signed class ro data (28),
    /// category class properties (29) and classes (31).
    pub flags: u64,
    pub atoms: Vec<Atom>,
    pub fixups: Vec<Fixup>,
    pub sections: Vec<CustomSection>,
    /// The mergeable dylib's own identity.
    pub own: DylibInfo,
    pub dylibs: Vec<DylibInfo>,
    pub debug_infos: Vec<DebugInfo>,
}

/// A dylib a mergeable dylib links, which the merging image loads in
/// its place: by the install name and versions recorded, exporting
/// what the atoms import from it.
pub struct Dependency {
    pub path: std::path::PathBuf,
    pub info: DylibInfo,
    pub exports: Vec<&'static str>,
    pub weak_exports: Vec<&'static str>,
}

/// AtomFileFlags bit 31: the dylib defines Objective-C or Swift
/// classes, for which ld-prime adds its hook to a merging image.
pub const FLAG_HAS_CLASSES: u64 = 1 << 31;
pub(crate) const FLAG_HAS_OBJC_INFO: u64 = 1 << 26;
pub(crate) const FLAG_SIGNED_CLASS_RO: u64 = 1 << 28;
pub(crate) const FLAG_CATEGORY_CLASS_PROPERTIES: u64 = 1 << 29;
pub(crate) const FLAG_HAS_SWIFT_OR_OBJC: u64 = 1 << 30;

/// A symbol name as the linker keeps one.
fn symbol_str(name: &'static [u8]) -> &'static str {
    match std::str::from_utf8(name) {
        Ok(s) => s,
        Err(_) => String::from_utf8_lossy(name).into_owned().leak(),
    }
}

fn read16(data: &[u8], off: usize) -> u16 {
    u16::from_le_bytes(data[off..off + 2].try_into().unwrap())
}

fn read32(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(data[off..off + 4].try_into().unwrap())
}

fn read64(data: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(data[off..off + 8].try_into().unwrap())
}

fn sign_extend(val: u64, bits: u32) -> i64 {
    ((val << (64 - bits)) as i64) >> (64 - bits)
}

/// Where a dylib's LC_ATOM_INFO data is in its file.
fn atom_info_range(data: &[u8]) -> Option<(usize, usize)> {
    let hdr = MachHeader::read_from(data);
    let mut off = size_of::<MachHeader>();
    for _ in 0..hdr.ncmds {
        let lc = LoadCommand::read_from(data.get(off..)?);
        if lc.cmd == LC_ATOM_INFO {
            let cmd = LinkEditDataCommand::read_from(&data[off..]);
            return Some((cmd.dataoff as usize, cmd.datasize as usize));
        }
        off += lc.cmdsize as usize;
    }
    None
}

impl AtomFile {
    /// Reads a dylib's LC_ATOM_INFO; an error says what is wrong with
    /// it, in ld-prime's words where it has some.
    pub fn read(mf: &'static MappedFile) -> Result<Self, String> {
        let file = mf.data();
        let (base, size) = atom_info_range(file).ok_or("not built with -make_mergeable")?;
        let blob = file.get(base..base + size).ok_or("atom payload goes beyond end of file")?;
        if blob.len() < HEADER_SIZE {
            return Err("file too small".into());
        }
        if &blob[..8] != MAGIC {
            return Err("file magic wrong".into());
        }
        let version = read16(blob, 8);
        if version != 3 {
            return Err(format!(
                "atom file version {version} is too new.  Max supported version of 3"
            ));
        }
        if blob[10] != 2 {
            return Err(format!(
                "atom version {} is too new.  Max supported version of 2",
                blob[10]
            ));
        }
        if blob[11] != 2 {
            return Err(format!(
                "fixup version {} is too new.  Max supported version of 2",
                blob[11]
            ));
        }
        if read32(blob, 0x60) as usize != HEADER_SIZE {
            return Err("atoms array must be located directly after the atom file structure".into());
        }
        let r = Reader { file, base, blob };
        let symbols = r.symbol_names()?;
        let large_addends: Vec<i64> =
            r.array(0x70, 8)?.chunks(8).map(|c| read64(c, 0) as i64).collect();
        let fixups = r.fixups(&large_addends)?;
        let sections: Vec<CustomSection> = r
            .array(0x78, SECTION_SIZE)?
            .chunks(SECTION_SIZE)
            .map(|c| CustomSection {
                segname: c[8..24].try_into().unwrap(),
                sectname: c[26..42].try_into().unwrap(),
                flags: read32(c, 4),
            })
            .collect();
        let (own, _) = r.dylib_info(read32(blob, 0xa8) as usize)?;
        let dylibs = r.dylib_infos(read32(blob, 0xb0) as usize, read32(blob, 0xb4) as usize)?;
        let debug_infos = r.debug_infos()?;
        let atoms = r.atoms(&symbols, fixups.len(), sections.len(), dylibs.len())?;
        Ok(Self {
            cputype: read32(blob, 0x14),
            cpusubtype: read32(blob, 0x18),
            platform: read32(blob, 0x1c),
            minos: read32(blob, 0x20),
            sdk: read32(blob, 0x24),
            flags: read64(blob, 0x28),
            atoms,
            fixups,
            sections,
            own,
            dylibs,
            debug_infos,
        })
    }

    /// The dylibs the mergeable one links, each with the symbols its
    /// atoms import from it.
    pub fn dependencies(&self, path: &Path) -> Vec<Dependency> {
        let mut deps: Vec<Dependency> = self
            .dylibs
            .iter()
            .map(|info| Dependency {
                path: path.to_path_buf(),
                info: info.clone(),
                exports: Vec::new(),
                weak_exports: Vec::new(),
            })
            .collect();
        for atom in &self.atoms {
            let (Some(dylib), Some(name)) = (atom.dylib, atom.name) else { continue };
            let name = symbol_str(name);
            deps[dylib].exports.push(name);
            if atom.kind == kind::DYLIB_EXPORT_WEAK_DEF {
                deps[dylib].weak_exports.push(name);
            }
        }
        deps
    }

    /// Whether the dylib's objects define Objective-C (or Swift)
    /// classes, as ld-prime records it.
    pub fn defines_classes(&self) -> bool {
        self.flags & FLAG_HAS_CLASSES != 0
    }
}

/// Bounds-checked access to an atom file's tables.
struct Reader<'a> {
    file: &'static [u8],
    base: usize,
    blob: &'a [u8],
}

impl Reader<'_> {
    /// A table the header gives by offset and count at `field`.
    fn array(&self, field: usize, elem: usize) -> Result<&[u8], String> {
        let off = read32(self.blob, field) as usize;
        let count = read32(self.blob, field + 4) as usize;
        self.blob
            .get(off..off + count * elem)
            .ok_or_else(|| "not enough space for array in parent buffer".to_string())
    }

    /// A NUL-terminated string `rel` bytes from blob offset `at`, which
    /// may reach back into the dylib around the blob.
    fn string_at(&self, at: usize, rel: i64, len: usize) -> Result<&'static [u8], String> {
        let start = (self.base + at) as i64 + rel;
        usize::try_from(start)
            .ok()
            .and_then(|s| self.file.get(s..s + len))
            .ok_or_else(|| "string beyond end of file".to_string())
    }

    /// A CStringRO_1: an offset from the record, then a length.
    fn cstring(&self, at: usize) -> Result<Vec<u8>, String> {
        let len = read64(self.blob, at + 8) as usize;
        if len == 0 {
            return Ok(Vec::new());
        }
        Ok(self.string_at(at, read64(self.blob, at) as i64, len)?.to_vec())
    }

    fn symbol_names(&self) -> Result<Vec<&'static [u8]>, String> {
        let table = self.array(0x88, 16)?;
        let first = read32(self.blob, 0x88) as usize;
        (0..table.len() / 16)
            .map(|i| {
                let at = first + i * 16;
                let len = (read64(self.blob, at + 8) >> 44) as usize;
                self.string_at(at, read64(self.blob, at) as i64, len)
            })
            .collect()
    }

    fn fixups(&self, large_addends: &[i64]) -> Result<Vec<Fixup>, String> {
        let table = self.array(0x68, FIXUP_SIZE)?;
        table
            .chunks(FIXUP_SIZE)
            .map(|c| {
                let w2 = read32(c, 8);
                let w3 = read32(c, 12);
                let kind = (w2 & 0x3ff) as u16;
                let usage = extras_usage(kind);
                let addend = if w2 & 0x400 != 0 {
                    *large_addends.get((w2 >> 11) as usize).ok_or("bad large addend index")?
                } else {
                    match usage {
                        2 => w3 as i32 as i64,
                        3 => sign_extend(((w2 >> 11) | ((w3 >> 25) << 21)) as u64, 28),
                        4..=8 => sign_extend(((w2 >> 11) | ((w3 >> 16) << 21)) as u64, 32),
                        _ => sign_extend((w2 >> 11) as u64, 21),
                    }
                };
                Ok(Fixup {
                    offset: read32(c, 0),
                    target: read32(c, 4),
                    kind,
                    addend,
                    from: if usage == 1 { w3 } else { 0 },
                    other: if (4..=8).contains(&usage) { w3 as u8 } else { 0 },
                })
            })
            .collect()
    }

    /// A DylibFileInfoRO_2 at blob offset `at`, and its size.
    fn dylib_info(&self, at: usize) -> Result<(DylibInfo, usize), String> {
        if at + DYLIB_INFO_SIZE > self.blob.len() {
            return Err("buffer is not large enough for dylib file info".into());
        }
        let info = DylibInfo {
            install_name: self.cstring(at + 8)?,
            current_version: read32(self.blob, at),
            compatibility_version: read32(self.blob, at + 4),
        };
        let nplatforms = read32(self.blob, at + 0x2c) as usize;
        let lists: usize =
            [0x34, 0x3c, 0x44].iter().map(|&f| read32(self.blob, at + f) as usize).sum();
        Ok((info, DYLIB_INFO_SIZE + (4 * nplatforms).next_multiple_of(8) + 16 * lists))
    }

    /// `count` DylibFileInfoRO_2 records one after another from `at`.
    fn dylib_infos(&self, at: usize, count: usize) -> Result<Vec<DylibInfo>, String> {
        let mut out = Vec::with_capacity(count);
        let mut pos = at;
        for _ in 0..count {
            let (info, size) = self.dylib_info(pos)?;
            out.push(info);
            pos += size;
        }
        Ok(out)
    }

    fn debug_infos(&self) -> Result<Vec<DebugInfo>, String> {
        let first = read32(self.blob, 0xbc) as usize;
        let count = read32(self.blob, 0xc0) as usize;
        if first + count * DEBUG_INFO_SIZE > self.blob.len() {
            return Err("not enough space for array in parent buffer".into());
        }
        (0..count)
            .map(|i| {
                let at = first + i * DEBUG_INFO_SIZE;
                Ok(DebugInfo {
                    mtime: read32(self.blob, at),
                    source_dir: self.cstring(at + 8)?,
                    source_name: self.cstring(at + 0x18)?,
                    object_path: self.cstring(at + 0x28)?,
                })
            })
            .collect()
    }

    fn atoms(
        &self,
        symbols: &[&'static [u8]],
        nfixups: usize,
        nsections: usize,
        ndylibs: usize,
    ) -> Result<Vec<Atom>, String> {
        let table = self.array(0x60, ATOM_SIZE)?;
        let pool = read32(self.blob, 0x98) as i64;
        table
            .chunks(ATOM_SIZE)
            .enumerate()
            .map(|(i, c)| {
                if read32(c, 0) as usize != i {
                    return Err(format!("atom {i} has ordinal {}", read32(c, 0)));
                }
                let nfix = read32(c, 4) as usize;
                let first = read32(c, 8) as usize;
                if first + nfix > nfixups {
                    return Err("fixups out of range".into());
                }
                let name = match read32(c, 12) {
                    0xff_ffff => None,
                    n => Some(*symbols.get(n as usize).ok_or("symbol index out of range")?),
                };
                let flags = read32(c, 16);
                let size = read32(c, 20);
                let content = match read32(c, 24) as i32 {
                    -1 => None,
                    off => Some(self.string_at(pool as usize, off as i64, size as usize)?),
                };
                let custom = (flags >> 21) as u8;
                let custom_section = (custom != 0xff).then_some(custom as usize);
                if custom_section.is_some_and(|s| s >= nsections) {
                    return Err("custom section index overflow".into());
                }
                let dylib = (c[0x1c] != 0xff).then_some(c[0x1c] as usize);
                if dylib.is_some_and(|d| d >= ndylibs) {
                    return Err("dylib index out of range".into());
                }
                Ok(Atom {
                    name,
                    scope: (flags & 7) as u8,
                    kind: ((flags >> 3) & 0x1f) as u8,
                    content_type: ((flags >> 8) & 0x7f) as u8,
                    cold: flags & (1 << 15) != 0,
                    no_dead_strip: flags & (1 << 17) != 0,
                    import: ((flags >> 19) & 3) as u8,
                    custom_section,
                    size,
                    content,
                    dylib,
                    p2align: c[0x1d],
                    modulus: read16(c, 0x1e),
                    debug: read16(c, 0x20),
                    fixups: first..first + nfix,
                })
            })
            .collect()
    }
}

/// The section ld-prime gives an atom of a content type: its segment
/// and section names and Mach-O flags, as an object would have them
/// (its StandardSection::fromContentType).
pub(crate) fn standard_section(ct: u8) -> Option<(&'static str, &'static str, u32)> {
    const TEXT: u32 = S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
    Some(match ct {
        2 => ("__TEXT", "__text", TEXT),
        9 => ("__TEXT", "__const", 0),
        10 => ("__TEXT", "__cstring", S_CSTRING_LITERALS),
        11 => ("__TEXT", "__objc_classname", S_CSTRING_LITERALS),
        12 => ("__TEXT", "__objc_methname", S_CSTRING_LITERALS),
        13 => ("__TEXT", "__objc_methtype", S_CSTRING_LITERALS),
        14 => ("__TEXT", "__oslogstring", S_CSTRING_LITERALS),
        15 => ("__TEXT", "__objc_methlist", 0),
        16 => ("__TEXT", "__ustring", 0),
        17 => ("__TEXT", "__literal4", S_4BYTE_LITERALS),
        18 => ("__TEXT", "__literal8", S_8BYTE_LITERALS),
        19 => ("__TEXT", "__literal16", S_16BYTE_LITERALS),
        // A slot of an object's GOT, which an object has as a regular
        // section (one of non-lazy pointers is refused).
        22 => ("__DATA", "__got", 0),
        26 => ("__DATA", "__const", 0),
        27 => ("__DATA", "__data", 0),
        28 => ("__DATA", "__cfstring", 0),
        29 => ("__DATA", "__const_cfobj2", 0),
        30 => ("__TEXT", "__gcc_except_tab", 0),
        31 => (
            "__TEXT",
            "__eh_frame",
            S_COALESCED | S_ATTR_NO_TOC | S_ATTR_STRIP_STATIC_SYMS | S_ATTR_LIVE_SUPPORT,
        ),
        32 => ("__LD", "__compact_unwind", S_ATTR_DEBUG),
        33 => ("__DATA", "__objc_classrefs", S_ATTR_NO_DEAD_STRIP),
        34 => ("__DATA", "__objc_superrefs", S_ATTR_NO_DEAD_STRIP),
        35 => ("__DATA", "__objc_selrefs", S_LITERAL_POINTERS | S_ATTR_NO_DEAD_STRIP),
        36 => ("__DATA", "__objc_protorefs", S_COALESCED | S_ATTR_NO_DEAD_STRIP),
        37 => ("__DATA", "__objc_ivar", 0),
        38 => ("__DATA", "__objc_data", 0),
        39 => ("__DATA", "__objc_const", 0),
        40 => ("__DATA", "__objc_classlist", S_ATTR_NO_DEAD_STRIP),
        41 => ("__DATA", "__objc_catlist", S_ATTR_NO_DEAD_STRIP),
        42 => ("__DATA", "__objc_protolist", S_COALESCED),
        43 => ("__DATA", "__objc_imageinfo", 0),
        44 => ("__DATA", "__objc_nlclslist", S_ATTR_NO_DEAD_STRIP),
        45 => ("__DATA", "__objc_nlcatlist", S_ATTR_NO_DEAD_STRIP),
        46 => ("__DATA", "__objc_intobj", 0),
        47 => ("__DATA", "__objc_floatobj", 0),
        48 => ("__DATA", "__objc_doubleobj", 0),
        49 => ("__DATA", "__objc_dateobj", 0),
        50 => ("__DATA", "__objc_dictobj", 0),
        51 => ("__DATA", "__objc_arrayobj", 0),
        52 => ("__DATA", "__objc_arraydata", 0),
        // An initializer offset the dylib's link made of a pointer
        // becomes one again (see Synth::place).
        53 | 55 => ("__DATA", "__mod_init_func", S_MOD_INIT_FUNC_POINTERS),
        54 => ("__DATA", "__mod_term_func", S_MOD_TERM_FUNC_POINTERS),
        56 => ("__TEXT", "__StaticInit", TEXT),
        57 => ("__DATA", "__thread_vars", S_THREAD_LOCAL_VARIABLES),
        58 => ("__DATA", "__thread_ptrs", S_THREAD_LOCAL_VARIABLE_POINTERS),
        64 => ("__DATA", "__thread_data", S_THREAD_LOCAL_REGULAR),
        65 => ("__DATA", "__thread_bss", S_THREAD_LOCAL_ZEROFILL),
        66 => ("__DATA", "__common", S_ZEROFILL),
        67 => ("__DATA", "__bss", S_ZEROFILL),
        70 => ("__DATA", "__objc_catlist2", S_ATTR_NO_DEAD_STRIP),
        72 => ("__DATA", "__objc_clsrolist", S_ATTR_NO_DEAD_STRIP),
        73 => ("__LD", "__func_variants", 0),
        _ => return None,
    })
}

/// Whether a section's atoms are fixed-size records or literals the
/// linker splits by itself, all of one alignment. (A C string keeps
/// its alignment and modulus as any atom.)
fn is_record_section(flags: u32, sectname: &[u8; 16]) -> bool {
    matches!(
        flags & SECTION_TYPE,
        S_4BYTE_LITERALS
            | S_8BYTE_LITERALS
            | S_16BYTE_LITERALS
            | S_LITERAL_POINTERS
            | S_MOD_INIT_FUNC_POINTERS
            | S_MOD_TERM_FUNC_POINTERS
            | S_THREAD_LOCAL_VARIABLES
    ) || [
        "__compact_unwind",
        "__eh_frame",
        "__cfstring",
        "__objc_classrefs",
        "__objc_superrefs",
        "__objc_classlist",
        "__objc_catlist",
        "__objc_catlist2",
        "__objc_nlclslist",
        "__objc_nlcatlist",
        "__objc_protolist",
        "__objc_protorefs",
        "__objc_clsrolist",
    ]
    .iter()
    .any(|n| str_to_name(n) == *sectname)
}

/// Sections beyond which an object can't number its symbols'.
const MAX_SECTIONS: usize = 255;

/// The section a final link puts a section's atoms in, by name, where
/// it merges several (see output_sections::merged_name): __StaticInit
/// joins __text, the literal pools __TEXT,__const.
fn output_group(segname: &[u8; 16], sectname: &[u8; 16]) -> ([u8; 16], [u8; 16]) {
    let text = str_to_name("__TEXT");
    if *segname == text && *sectname == str_to_name("__StaticInit") {
        return (text, str_to_name("__text"));
    }
    if *segname == text
        && ["__literal4", "__literal8", "__literal16"].iter().any(|n| str_to_name(n) == *sectname)
    {
        return (text, str_to_name("__const"));
    }
    (*segname, *sectname)
}

fn is_zerofill(flags: u32) -> bool {
    matches!(flags & SECTION_TYPE, S_ZEROFILL | S_GB_ZEROFILL | S_THREAD_LOCAL_ZEROFILL)
}

/// A section by its segment and section names and flags.
type SectionKey = ([u8; 16], [u8; 16], u32);

/// A section of the object being made.
struct Section {
    segname: [u8; 16],
    sectname: [u8; 16],
    flags: u32,
    p2align: u8,
    data: Vec<u8>,
    size: u64,
    addr: u64,
    relocs: Vec<MachRel>,
}

impl Section {
    fn new(segname: [u8; 16], sectname: [u8; 16], flags: u32, p2align: u8) -> Self {
        let (data, relocs) = (Vec::new(), Vec::new());
        Self { segname, sectname, flags, p2align, data, size: 0, addr: 0, relocs }
    }

    /// Appends an atom's bytes (zeros where it has none, nothing in zero
    /// fill) at the first offset that is its modulus past a multiple of
    /// its alignment, which is where it was in its object - a record at
    /// one of the alignment, as the linker aligns records. Returns the
    /// offset.
    fn append(&mut self, content: Option<&[u8]>, size: u64, p2align: u8, modulus: u64) -> u64 {
        self.p2align = self.p2align.max(p2align);
        let align = 1u64 << p2align;
        let modulus = if is_record_section(self.flags, &self.sectname) { 0 } else { modulus };
        let mut off = (self.size & !(align - 1)) + modulus;
        if off < self.size {
            off += align;
        }
        self.size = off + size;
        if !is_zerofill(self.flags) {
            self.data.resize(off as usize, 0);
            match content {
                Some(bytes) => self.data.extend_from_slice(bytes),
                None => self.data.resize(self.size as usize, 0),
            }
        }
        off
    }
}

/// Where a symbol of the object being made stands.
#[derive(Clone, Copy)]
enum SymPlace {
    Defined { sect: usize, offset: u64 },
    Undefined,
    Common { size: u64, p2align: u8 },
    Absolute(u64),
}

struct Symbol {
    name: Vec<u8>,
    place: SymPlace,
    /// N_EXT, N_PEXT.
    n_type: u8,
    n_desc: u16,
}

/// The object an atom file stands for, under construction.
struct Synth<'a, E: Target> {
    af: &'a AtomFile,
    sections: Vec<Section>,
    /// The section each section key is being filled into.
    open: hashbrown::HashMap<SectionKey, usize>,
    /// The section an atom of each output section went to last.
    last_in_group: hashbrown::HashMap<([u8; 16], [u8; 16]), usize>,
    /// Each atom's section and offset there, if it has a place.
    place: Vec<Option<(usize, u64)>>,
    /// The symbol that stands for each atom, if any.
    sym_of: Vec<Option<usize>>,
    symbols: Vec<Symbol>,
    undefined: hashbrown::HashMap<Vec<u8>, usize>,
    _target: std::marker::PhantomData<E>,
}

/// Makes the object file a mergeable dylib's atoms stand for.
pub fn synthesize_object<E: Target>(af: &AtomFile, path: &Path) -> Vec<u8> {
    let mut s = Synth::<E> {
        af,
        sections: Vec::new(),
        open: hashbrown::HashMap::new(),
        last_in_group: hashbrown::HashMap::new(),
        place: vec![None; af.atoms.len()],
        sym_of: vec![None; af.atoms.len()],
        symbols: Vec::new(),
        undefined: hashbrown::HashMap::new(),
        _target: std::marker::PhantomData,
    };
    for i in placement_order(af) {
        s.place(i, path);
    }
    s.add_image_info();
    s.assign_addresses();
    for i in 0..af.atoms.len() {
        s.name_atom(i);
    }
    for i in 0..af.atoms.len() {
        s.name_alias(i);
    }
    for i in 0..af.atoms.len() {
        s.apply_fixups(i, path);
    }
    s.write()
}

/// The order to lay the atoms out in: theirs, but for the method lists
/// in the relative form, which go as the dylib has them - in the order
/// the linker's conversion writes them, by class, category and
/// protocol, which ld-prime's atom list doesn't keep.
fn placement_order(af: &AtomFile) -> Vec<usize> {
    let is_list = |i: &usize| af.atoms[*i].content_type == ctype::OBJC_METHOD_LIST;
    let mut lists: Vec<usize> = (0..af.atoms.len()).filter(is_list).collect();
    lists.sort_by_key(|&i| af.atoms[i].content.map(|c| c.as_ptr() as usize));
    let mut lists = lists.into_iter();
    (0..af.atoms.len()).map(|i| if is_list(&i) { lists.next().unwrap() } else { i }).collect()
}

impl<E: Target> Synth<'_, E> {
    fn is_arm64() -> bool {
        E::CPUTYPE == CPU_TYPE_ARM64
    }

    /// Lays an atom out in its section, if it has a place in one.
    fn place(&mut self, i: usize, path: &Path) {
        use kind::*;
        let atom = &self.af.atoms[i];
        if !matches!(atom.kind, REGULAR | WEAK_DEF | RESOLVER | ANON | ANON_COAL_BY_CONTENT) {
            if matches!(atom.kind, 13..=17) {
                fatal!("{}: unsupported atom kind {} in LC_ATOM_INFO", path.display(), atom.kind);
            }
            return;
        }
        // The objects' image info is made afresh (see add_image_info).
        if atom.content_type == ctype::OBJC_IMAGE_INFO {
            return;
        }
        let key = self.section_key(atom, path);
        // An initializer offset the dylib's link made of a pointer is
        // the pointer again, which the linker makes an offset itself
        // where the target is new enough.
        let (size, p2align, modulus, content) = if atom.content_type == ctype::INIT_OFFSET {
            (8, 3, 0, None)
        } else {
            (atom.size as u64, atom.p2align, atom.modulus as u64, atom.content)
        };
        let idx = self.section_for(key, p2align);
        let off = self.sections[idx].append(content, size, p2align, modulus);
        self.place[i] = Some((idx, off));
    }

    /// The section an atom goes in: one of the custom table's, or the
    /// standard one of its content type.
    fn section_key(&self, atom: &Atom, path: &Path) -> SectionKey {
        if let Some(idx) = atom.custom_section {
            let s = &self.af.sections[idx];
            return (s.segname, s.sectname, s.flags);
        }
        match standard_section(atom.content_type) {
            Some((seg, sect, flags)) => (str_to_name(seg), str_to_name(sect), flags),
            None => fatal!(
                "{}: unsupported atom content type {} in LC_ATOM_INFO",
                path.display(),
                atom.content_type
            ),
        }
    }

    /// The section of the object to put an atom of a key and alignment
    /// in. The objects had a section of a name each, but with their own
    /// alignments, which to the linker are all their atoms': atoms of
    /// another alignment go in a section of their own. And the atoms of
    /// sections a final link merges into one (see output_group) keep
    /// their order there: a section opens again after another of the
    /// group, as the objects had theirs.
    fn section_for(&mut self, key: SectionKey, p2align: u8) -> usize {
        let (segname, sectname, flags) = key;
        let record = is_record_section(flags, &sectname);
        let group = output_group(&segname, &sectname);
        let last = self.last_in_group.get(&group).copied();
        let reusable = |idx: usize, sections: &[Section]| {
            (record || sections[idx].p2align == p2align)
                && (last.is_none_or(|last| last == idx) || sections.len() >= MAX_SECTIONS)
        };
        let idx = match self.open.get(&key) {
            Some(&idx) if reusable(idx, &self.sections) => idx,
            _ => {
                self.sections.push(Section::new(segname, sectname, flags, p2align));
                self.open.insert(key, self.sections.len() - 1);
                self.sections.len() - 1
            }
        };
        self.last_in_group.insert(group, idx);
        idx
    }

    /// Adds the __objc_imageinfo record the objects had, from what the
    /// atom file says of it.
    fn add_image_info(&mut self) {
        let flags = self.af.flags;
        if flags & FLAG_HAS_OBJC_INFO == 0 {
            return;
        }
        let mut info = (flags as u32 & 0xff) << 8 | (flags as u32 & 0xffff00) << 8;
        if flags & FLAG_CATEGORY_CLASS_PROPERTIES != 0 {
            info |= 0x40;
        }
        if flags & FLAG_SIGNED_CLASS_RO != 0 {
            info |= 0x10;
        }
        let mut sect = Section::new(str_to_name("__DATA"), str_to_name("__objc_imageinfo"), 0, 2);
        sect.append(Some(&[[0; 4], info.to_le_bytes()].concat()), 8, 2, 0);
        self.sections.push(sect);
    }

    /// Gives the sections their addresses in the object, one after
    /// another, the zero fill ones last.
    fn assign_addresses(&mut self) {
        let mut addr = 0u64;
        for zerofill in [false, true] {
            for s in self.sections.iter_mut().filter(|s| is_zerofill(s.flags) == zerofill) {
                addr = addr.next_multiple_of(1u64 << s.p2align);
                s.addr = addr;
                addr += s.size;
            }
        }
    }

    fn add_symbol(&mut self, sym: Symbol) -> usize {
        self.symbols.push(sym);
        self.symbols.len() - 1
    }

    /// The undefined symbol of a name, made once.
    fn undefined(&mut self, name: &[u8], weak: bool) -> usize {
        if let Some(&idx) = self.undefined.get(name) {
            if weak {
                self.symbols[idx].n_desc |= N_WEAK_REF;
            }
            return idx;
        }
        let n_desc = if weak { N_WEAK_REF } else { 0 };
        let idx = self.add_symbol(Symbol {
            name: name.to_vec(),
            place: SymPlace::Undefined,
            n_type: N_EXT,
            n_desc,
        });
        self.undefined.insert(name.to_vec(), idx);
        idx
    }

    /// Gives an atom its symbol: its name in its section, or a private
    /// label if it has none (the linker splits the section at it, as
    /// it was an atom of its own), or an undefined or common symbol.
    fn name_atom(&mut self, i: usize) {
        use kind::*;
        let atom = &self.af.atoms[i];
        let name = atom.name;
        match atom.kind {
            DYLIB_EXPORT
            | DYLIB_EXPORT_WEAK_DEF
            | DYLIB_EXPORT_FORCE_LOAD
            | UNDEFINE
            | UNDEFINE_WEAK_IMPORT => {
                let weak = atom.import == 1 || atom.kind == UNDEFINE_WEAK_IMPORT;
                let Some(name) = name else { return };
                self.sym_of[i] = Some(self.undefined(name, weak));
            }
            TENTATIVE_DEF => {
                let Some(name) = name else { return };
                let (n_type, n_desc) = scope_bits(atom.scope);
                self.sym_of[i] = Some(self.add_symbol(Symbol {
                    name: name.to_vec(),
                    place: SymPlace::Common { size: atom.size as u64, p2align: atom.p2align },
                    n_type: n_type | N_EXT,
                    n_desc,
                }));
            }
            // Its value is its content, eight bytes.
            ABSOLUTE => {
                let (Some(name), Some(value)) = (name, atom.content) else { return };
                let value = value.get(..8).map_or(0, |v| read64(v, 0));
                let (n_type, n_desc) = scope_bits(atom.scope);
                self.sym_of[i] = Some(self.add_symbol(Symbol {
                    name: name.to_vec(),
                    place: SymPlace::Absolute(value),
                    n_type,
                    n_desc,
                }));
            }
            _ => {
                let Some((sect, offset)) = self.place[i] else { return };
                // eh_frame records are found by their lengths and need
                // no names.
                if atom.content_type == ctype::CFI {
                    return;
                }
                let (name, n_type, mut n_desc) = match name {
                    Some(name) => {
                        let (n_type, n_desc) = scope_bits(atom.scope);
                        (name.to_vec(), n_type, n_desc)
                    }
                    None => (format!("LM{i}").into_bytes(), 0, 0),
                };
                if atom.kind == WEAK_DEF {
                    n_desc |= N_WEAK_DEF;
                }
                if atom.kind == RESOLVER {
                    n_desc |= N_SYMBOL_RESOLVER;
                }
                if atom.no_dead_strip {
                    n_desc |= N_NO_DEAD_STRIP;
                }
                if atom.cold {
                    n_desc |= N_COLD_FUNC;
                }
                self.sym_of[i] = Some(self.add_symbol(Symbol {
                    name,
                    place: SymPlace::Defined { sect, offset },
                    n_type,
                    n_desc,
                }));
            }
        }
    }

    /// Gives an alias its symbol: at its target's place plus the
    /// addend, an alternate entry there unless at its start, or
    /// undefined if the target is (__dso_handle, which ld-prime makes
    /// an alias of segment$start$__TEXT, and which the merging link
    /// defines afresh).
    fn name_alias(&mut self, i: usize) {
        use kind::*;
        let atom = &self.af.atoms[i];
        if !matches!(atom.kind, ALIAS | WEAK_DEF_ALIAS) {
            return;
        }
        let Some(name) = atom.name else { return };
        let Some(f) = self.af.fixups[atom.fixups.clone()].iter().find(|f| f.kind == fk::ALIAS_OF)
        else {
            return;
        };
        let target = f.target as usize;
        let Some((sect, offset)) = self.place.get(target).copied().flatten() else {
            self.sym_of[i] = Some(self.undefined(name, false));
            return;
        };
        let (n_type, mut n_desc) = scope_bits(atom.scope);
        if f.addend != 0 {
            n_desc |= N_ALT_ENTRY;
        }
        if atom.kind == WEAK_DEF_ALIAS {
            n_desc |= N_WEAK_DEF;
        }
        if atom.no_dead_strip {
            n_desc |= N_NO_DEAD_STRIP;
        }
        let offset = offset.wrapping_add_signed(f.addend);
        self.sym_of[i] = Some(self.add_symbol(Symbol {
            name: name.to_vec(),
            place: SymPlace::Defined { sect, offset },
            n_type,
            n_desc,
        }));
    }

    /// The object address of an atom plus `addend`.
    fn addr(&self, atom: u32, addend: i64) -> u64 {
        let (sect, off) = self.place[atom as usize].unwrap_or((0, 0));
        (self.sections[sect].addr + off).wrapping_add_signed(addend)
    }

    /// The symbol a fixup refers to its target by, and the addend
    /// relative to it.
    fn target_sym(&self, atom: u32) -> Option<usize> {
        self.sym_of.get(atom as usize).copied().flatten()
    }

    fn reloc(r_address: u32, symbolnum: usize, r_type: u8, length: u32, pcrel: bool) -> MachRel {
        MachRel {
            r_address,
            bits: symbolnum as u32 & 0xff_ffff
                | (pcrel as u32) << 24
                | length << 25
                | 1 << 27
                | (r_type as u32) << 28,
        }
    }

    /// Turns an atom's fixups into the relocations its object had, and
    /// puts back in its bytes what the object had where they apply.
    fn apply_fixups(&mut self, i: usize, path: &Path) {
        let atom = &self.af.atoms[i];
        let Some((sect, atom_off)) = self.place[i] else { return };
        if atom.content_type == ctype::CFI {
            return self.apply_cfi_fixups(i);
        }
        for f in &self.af.fixups[atom.fixups.clone()] {
            if f.kind == fk::ALIAS_OF || f.kind == fk::KEEP_ALIVE {
                continue;
            }
            let target = self.af.atoms.get(f.target as usize);
            if target.is_none_or(|t| t.kind == kind::ANON_PLACEHOLDER) {
                continue;
            }
            let Some(sym) = self.target_sym(f.target) else {
                fatal!(
                    "{}: fixup of atom {i} at 0x{:x} has a target with no symbol",
                    path.display(),
                    f.offset
                );
            };
            let off = (atom_off + f.offset as u64) as u32;
            let mut out = Vec::new();
            let ok = if Self::is_arm64() {
                self.arm64_fixup(sect, off, i, f, sym, &mut out)
            } else {
                self.x86_64_fixup(sect, off, i, f, sym, &mut out)
            };
            if !ok {
                fatal!("{}: unsupported fixup kind 0x{:x} in LC_ATOM_INFO", path.display(), f.kind);
            }
            self.sections[sect].relocs.extend(out);
        }
    }

    /// Writes `val` of `size` bytes at `off` of a section.
    fn put(&mut self, sect: usize, off: u32, size: u32, val: u64) {
        let data = &mut self.sections[sect].data;
        let off = off as usize;
        match size {
            4 => data[off..off + 4].copy_from_slice(&(val as u32).to_le_bytes()),
            8 => data[off..off + 8].copy_from_slice(&val.to_le_bytes()),
            _ => data[off] = val as u8,
        }
    }

    fn insn(&self, sect: usize, off: u32) -> u32 {
        read32(&self.sections[sect].data, off as usize)
    }

    fn set_insn(&mut self, sect: usize, off: u32, insn: u32) {
        self.put(sect, off, 4, insn as u64);
    }

    /// A SUBTRACTOR pair: `from`'s symbol taken from the target's, the
    /// field holding the addend.
    fn diff_pair(
        &mut self,
        sect: usize,
        off: u32,
        size: u32,
        from: usize,
        sym: usize,
        addend: i64,
    ) -> [MachRel; 2] {
        let (sub, unsigned) = if Self::is_arm64() {
            (ARM64_RELOC_SUBTRACTOR, ARM64_RELOC_UNSIGNED)
        } else {
            (X86_64_RELOC_SUBTRACTOR, X86_64_RELOC_UNSIGNED)
        };
        let length = if size == 8 { 3 } else { 2 };
        self.put(sect, off, size, addend as u64);
        [Self::reloc(off, from, sub, length, false), Self::reloc(off, sym, unsigned, length, false)]
    }

    /// The generic fixups, as either target encodes them.
    fn generic_fixup(
        &mut self,
        sect: usize,
        off: u32,
        atom: usize,
        f: &Fixup,
        sym: usize,
        out: &mut Vec<MachRel>,
    ) -> bool {
        let unsigned = if Self::is_arm64() { ARM64_RELOC_UNSIGNED } else { X86_64_RELOC_UNSIGNED };
        match f.kind {
            fk::PTR64 | fk::TLV_OFFSET | fk::IMAGE_OFFSET32 => {
                self.put(sect, off, 8, f.addend as u64);
                out.push(Self::reloc(off, sym, unsigned, 3, false));
            }
            fk::PTR32 => {
                self.put(sect, off, 4, f.addend as u64);
                out.push(Self::reloc(off, sym, unsigned, 2, false));
            }
            fk::DIFF32 | fk::DIFF64 => {
                let size = if f.kind == fk::DIFF64 { 8 } else { 4 };
                let Some(from) = self.target_sym(f.from) else { return false };
                out.extend(self.diff_pair(sect, off, size, from, sym, f.addend));
            }
            fk::PCREL_DELTA32 => {
                // Relative to the field: to the atom's start, less the
                // field's place in it.
                let Some(from) = self.target_sym(atom as u32) else { return false };
                let field = off as i64 - self.place[atom].unwrap().1 as i64;
                out.extend(self.diff_pair(sect, off, 4, from, sym, f.addend - field));
            }
            _ => return false,
        }
        true
    }

    fn arm64_fixup(
        &mut self,
        sect: usize,
        off: u32,
        atom: usize,
        f: &Fixup,
        sym: usize,
        out: &mut Vec<MachRel>,
    ) -> bool {
        use fk::*;
        // An ADDEND before the record carries a nonzero addend.
        let addend = |out: &mut Vec<MachRel>, at: u32| {
            if f.addend != 0 {
                let bits =
                    (f.addend as u32 & 0xff_ffff) | 2 << 25 | (ARM64_RELOC_ADDEND as u32) << 28;
                out.push(MachRel { r_address: at, bits });
            }
        };
        let second = off + 4 * f.other as u32;
        match f.kind {
            ARM64_B26 | ARM64_B26_ADDEND => {
                self.set_insn(sect, off, self.insn(sect, off) & 0xfc00_0000);
                addend(out, off);
                out.push(Self::reloc(off, sym, ARM64_RELOC_BRANCH26, 2, true));
            }
            ARM64_ADRP | ARM64_ADRP_ADDEND => {
                self.clear_adrp(sect, off);
                addend(out, off);
                out.push(Self::reloc(off, sym, ARM64_RELOC_PAGE21, 2, true));
            }
            ARM64_LO12 | ARM64_LO12_ADDEND => {
                self.clear_imm12(sect, off);
                addend(out, off);
                out.push(Self::reloc(off, sym, ARM64_RELOC_PAGEOFF12, 2, false));
            }
            ARM64_ADRP_LO12 | ARM64_ADRP_LO12_ADDEND => {
                self.clear_adrp(sect, off);
                addend(out, off);
                out.push(Self::reloc(off, sym, ARM64_RELOC_PAGE21, 2, true));
                self.clear_imm12(sect, second);
                addend(out, second);
                out.push(Self::reloc(second, sym, ARM64_RELOC_PAGEOFF12, 2, false));
            }
            ARM64_ADRP_GOT | ARM64_ADRP_GOT_NO_OPT => {
                self.clear_adrp(sect, off);
                out.push(Self::reloc(off, sym, ARM64_RELOC_GOT_LOAD_PAGE21, 2, true));
            }
            ARM64_LD12_GOT | ARM64_LD12_GOT_NO_OPT => {
                self.restore_ldr(sect, off);
                out.push(Self::reloc(off, sym, ARM64_RELOC_GOT_LOAD_PAGEOFF12, 2, false));
            }
            ARM64_ADD_GOT => {
                self.clear_imm12(sect, off);
                out.push(Self::reloc(off, sym, ARM64_RELOC_GOT_LOAD_PAGEOFF12, 2, false));
            }
            ARM64_ADRP_LDR_GOT | ARM64_ADRP_LDR_GOT_NO_OPT | ARM64_ADRP_ADD_GOT => {
                self.clear_adrp(sect, off);
                out.push(Self::reloc(off, sym, ARM64_RELOC_GOT_LOAD_PAGE21, 2, true));
                if f.kind == ARM64_ADRP_ADD_GOT {
                    self.clear_imm12(sect, second);
                } else {
                    self.restore_ldr(sect, second);
                }
                out.push(Self::reloc(second, sym, ARM64_RELOC_GOT_LOAD_PAGEOFF12, 2, false));
            }
            ARM64_ADRP_TLV => {
                self.clear_adrp(sect, off);
                out.push(Self::reloc(off, sym, ARM64_RELOC_TLVP_LOAD_PAGE21, 2, true));
            }
            ARM64_LD12_TLV => {
                self.restore_ldr(sect, off);
                out.push(Self::reloc(off, sym, ARM64_RELOC_TLVP_LOAD_PAGEOFF12, 2, false));
            }
            PCREL32_TO_GOT | SWIFT_REL32_TO_GOT => {
                let swift = (f.kind == SWIFT_REL32_TO_GOT) as i64;
                self.put(sect, off, 4, (f.addend + swift) as u64);
                out.push(Self::reloc(off, sym, ARM64_RELOC_POINTER_TO_GOT, 2, true));
            }
            PTR64_TO_GOT => {
                self.put(sect, off, 8, f.addend as u64);
                out.push(Self::reloc(off, sym, ARM64_RELOC_POINTER_TO_GOT, 3, false));
            }
            _ => return self.generic_fixup(sect, off, atom, f, sym, out),
        }
        true
    }

    fn x86_64_fixup(
        &mut self,
        sect: usize,
        off: u32,
        atom: usize,
        f: &Fixup,
        sym: usize,
        out: &mut Vec<MachRel>,
    ) -> bool {
        use fk::*;
        let (r_type, bias) = match f.kind {
            X86_64_CALL => (X86_64_RELOC_BRANCH, 0),
            X86_64_RIP => (X86_64_RELOC_SIGNED, 0),
            X86_64_RIP1 => (X86_64_RELOC_SIGNED_1, 1),
            X86_64_RIP2 => (X86_64_RELOC_SIGNED_2, 2),
            X86_64_RIP4 => (X86_64_RELOC_SIGNED_4, 4),
            X86_64_RIP_GOT | PCREL32_TO_GOT | SWIFT_REL32_TO_GOT => (X86_64_RELOC_GOT, 0),
            X86_64_RIP_GOT_LOAD => (X86_64_RELOC_GOT_LOAD, 0),
            X86_64_RIP_TLV_LOAD => (X86_64_RELOC_TLV, 0),
            X86_64_BRANCH8 => {
                self.put(sect, off, 1, f.addend as u64);
                out.push(Self::reloc(off, sym, X86_64_RELOC_BRANCH, 0, true));
                return true;
            }
            _ => return self.generic_fixup(sect, off, atom, f, sym, out),
        };
        // A movq ld-prime relaxed to a leaq loads again.
        if matches!(r_type, X86_64_RELOC_GOT_LOAD | X86_64_RELOC_TLV)
            && let Some(at) = off.checked_sub(2)
            && self.sections[sect].data[at as usize] == 0x8d
        {
            self.sections[sect].data[at as usize] = 0x8b;
        }
        let addend = match f.kind {
            // A data reference to a GOT slot is from the field's start,
            // as the instruction's are from its end; Swift's relative
            // reference to one has its low bit set besides (the indirect
            // flag of a relative pointer), which ld-prime's kind implies.
            PCREL32_TO_GOT => f.addend + 4,
            SWIFT_REL32_TO_GOT => f.addend + 5,
            _ => f.addend - bias,
        };
        self.put(sect, off, 4, addend as u64);
        out.push(Self::reloc(off, sym, r_type, 2, true));
        true
    }

    /// Clears an ADRP's page.
    fn clear_adrp(&mut self, sect: usize, off: u32) {
        let insn = self.insn(sect, off);
        if insn & 0x9f00_0000 == 0x9000_0000 {
            self.set_insn(sect, off, insn & 0x9f00_001f);
        }
    }

    /// Clears a load's, store's or add's 12-bit immediate.
    fn clear_imm12(&mut self, sect: usize, off: u32) {
        self.set_insn(sect, off, self.insn(sect, off) & 0xffc0_03ff);
    }

    /// Puts back the load of a GOT or TLV slot that ld-prime relaxed to
    /// an add of the target's page offset.
    fn restore_ldr(&mut self, sect: usize, off: u32) {
        let insn = self.insn(sect, off);
        let insn = if insn & 0xff80_0000 == 0x9100_0000 {
            0xf940_0000 | (insn & 0x3ff)
        } else {
            insn & 0xffc0_03ff
        };
        self.set_insn(sect, off, insn);
    }

    /// An eh_frame record's fixups, applied in place: the linker reads
    /// the records' pointers as values in the object (see
    /// input_files::apply_eh_frame_relocs), leaving only a CIE's
    /// personality reference for a GOT relocation.
    fn apply_cfi_fixups(&mut self, i: usize) {
        let atom = &self.af.atoms[i];
        let (sect, atom_off) = self.place[i].unwrap();
        for f in &self.af.fixups[atom.fixups.clone()] {
            let off = (atom_off + f.offset as u64) as u32;
            match f.kind {
                fk::DIFF32 | fk::DIFF64 => {
                    let val = self.addr(f.target, f.addend).wrapping_sub(self.addr(f.from, 0));
                    let size = if f.kind == fk::DIFF64 { 8 } else { 4 };
                    self.put(sect, off, size, val);
                }
                fk::PCREL_DELTA32 => {
                    let here = self.sections[sect].addr + off as u64;
                    let val = self.addr(f.target, f.addend).wrapping_sub(here);
                    self.put(sect, off, 4, val);
                }
                fk::PCREL32_TO_GOT | fk::SWIFT_REL32_TO_GOT => {
                    let Some(sym) = self.target_sym(f.target) else { continue };
                    let swift = (f.kind == fk::SWIFT_REL32_TO_GOT) as i64;
                    let (r_type, addend) = if Self::is_arm64() {
                        (ARM64_RELOC_POINTER_TO_GOT, f.addend + swift)
                    } else {
                        (X86_64_RELOC_GOT, f.addend + 4 + swift)
                    };
                    self.put(sect, off, 4, addend as u64);
                    self.sections[sect].relocs.push(Self::reloc(off, sym, r_type, 2, true));
                }
                _ => {}
            }
        }
    }

    /// The debug notes of the units the atoms came from, as an `ld -r`
    /// writes them: for each, an empty N_SO, N_SO with the source
    /// directory and name, N_OSO naming the object, then the notes of
    /// its symbols by address; and an empty N_SO closing the last.
    fn stabs(&self, strtab: &mut Strtab) -> Vec<NList> {
        let mut units: Vec<u16> = Vec::new();
        for atom in &self.af.atoms {
            if atom.debug != 0 && !units.contains(&atom.debug) {
                units.push(atom.debug);
            }
        }
        let so = |strtab: &mut Strtab, name: &[u8], n_sect: u8| NList {
            n_strx: strtab.add(name),
            n_type: N_SO,
            n_sect,
            ..Default::default()
        };
        let mut out = Vec::new();
        for &unit in &units {
            let Some(info) = self.af.debug_infos.get(unit as usize - 1) else { continue };
            out.push(so(strtab, b"", 1));
            out.push(so(strtab, &info.source_dir, 0));
            out.push(so(strtab, &info.source_name, 0));
            out.push(NList {
                n_strx: strtab.add(&info.object_path),
                n_type: N_OSO,
                n_sect: self.af.cpusubtype as u8,
                n_desc: 1,
                n_value: info.mtime as u64,
            });
            let mut notes: Vec<(u64, Vec<NList>)> = (0..self.af.atoms.len())
                .filter(|&i| self.af.atoms[i].debug == unit)
                .filter_map(|i| self.symbol_stabs(i, strtab))
                .collect();
            notes.sort_by_key(|(addr, _)| *addr);
            out.extend(notes.into_iter().flat_map(|(_, n)| n));
        }
        if !units.is_empty() {
            out.push(so(strtab, b"", 1));
        }
        out
    }

    /// The notes of an atom's symbol, as those of an object's with
    /// DWARF (see chunks::symtab's symbol_stabs), and the address to
    /// order them by: a function's N_FUN pair between N_BNSYM and
    /// N_ENSYM, an external variable's N_GSYM (a private external's
    /// too), a local one's N_STSYM, and
    /// a tentative definition's N_GSYM, last.
    fn symbol_stabs(&self, i: usize, strtab: &mut Strtab) -> Option<(u64, Vec<NList>)> {
        let sym = &self.symbols[self.sym_of[i]?];
        if crate::input_files::is_private_label(symbol_str_lossy(&sym.name)) {
            return None;
        }
        let entry =
            |n_type, n_strx, n_sect, n_value| NList { n_strx, n_type, n_sect, n_desc: 0, n_value };
        let name = strtab.add(&sym.name);
        let (sect, offset) = match sym.place {
            SymPlace::Common { .. } => return Some((u64::MAX, vec![entry(N_GSYM, name, 0, 0)])),
            SymPlace::Undefined | SymPlace::Absolute(_) => return None,
            SymPlace::Defined { sect, offset } => (sect, offset),
        };
        let section = &self.sections[sect];
        if !has_stabs(section) {
            return None;
        }
        let addr = section.addr + offset;
        let n_sect = sect as u8 + 1;
        let notes = if section.flags & S_ATTR_PURE_INSTRUCTIONS != 0 {
            let empty = strtab.add(b"");
            vec![
                entry(N_BNSYM, empty, n_sect, addr),
                entry(N_FUN, name, n_sect, addr),
                entry(N_FUN, empty, 0, self.af.atoms[i].size as u64),
                entry(N_ENSYM, empty, n_sect, addr),
            ]
        } else if sym.n_type & N_EXT != 0 {
            vec![entry(N_GSYM, name, 0, 0)]
        } else {
            vec![entry(N_STSYM, name, n_sect, addr)]
        };
        Some((addr, notes))
    }

    /// The symbol table: the locals, then the debug notes, then the
    /// defined externals, then the undefined ones and the tentative
    /// definitions, each kind in the order made.
    fn symbol_table(&self) -> SymbolTable {
        let kind = |s: &Symbol| match s.place {
            SymPlace::Undefined | SymPlace::Common { .. } => 2,
            SymPlace::Defined { .. } | SymPlace::Absolute(_) if s.n_type & N_EXT != 0 => 1,
            SymPlace::Defined { .. } | SymPlace::Absolute(_) => 0,
        };
        let mut order: Vec<usize> = (0..self.symbols.len()).collect();
        order.sort_by_key(|&i| kind(&self.symbols[i]));
        let mut table = SymbolTable {
            nlists: Vec::new(),
            strtab: Strtab::new(),
            index: vec![0; self.symbols.len()],
            nlocal: 0,
            nextdef: 0,
        };
        let nlocal = order.iter().take_while(|&&i| kind(&self.symbols[i]) == 0).count();
        for &i in &order[..nlocal] {
            self.push_symbol(&mut table, i);
        }
        let stabs = self.stabs(&mut table.strtab);
        table.nlists.extend(stabs);
        table.nlocal = table.nlists.len();
        for &i in &order[nlocal..] {
            self.push_symbol(&mut table, i);
        }
        table.nextdef = order.iter().filter(|&&i| kind(&self.symbols[i]) == 1).count();
        table
    }

    fn push_symbol(&self, table: &mut SymbolTable, i: usize) {
        let s = &self.symbols[i];
        let mut n = NList {
            n_strx: table.strtab.add(&s.name),
            n_type: s.n_type,
            n_sect: 0,
            n_desc: s.n_desc,
            n_value: 0,
        };
        match s.place {
            SymPlace::Defined { sect, offset } => {
                n.n_type |= N_SECT;
                n.n_sect = sect as u8 + 1;
                n.n_value = self.sections[sect].addr + offset;
            }
            SymPlace::Undefined => {}
            SymPlace::Common { size, p2align } => {
                n.n_value = size;
                n.n_desc |= (p2align as u16 & 0xf) << 8;
            }
            SymPlace::Absolute(value) => {
                n.n_type |= N_ABS;
                n.n_value = value;
            }
        }
        table.index[i] = table.nlists.len() as u32;
        table.nlists.push(n);
    }

    /// Writes the object out: the header and load commands (one segment
    /// of all the sections, the build version and the symbol tables),
    /// the sections' bytes, their relocations, the symbols and their
    /// names.
    fn write(self) -> Vec<u8> {
        let nsects = self.sections.len();
        let seg_size = size_of::<SegmentCommand>() + nsects * size_of::<MachSection>();
        let cmds_size = seg_size
            + size_of::<BuildVersionCommand>()
            + size_of::<SymtabCommand>()
            + size_of::<DysymtabCommand>();
        let mut out = vec![0u8; size_of::<MachHeader>() + cmds_size];

        let mut sect_offs = Vec::with_capacity(nsects);
        for s in &self.sections {
            if is_zerofill(s.flags) {
                sect_offs.push(0);
                continue;
            }
            let off = out.len().next_multiple_of(1 << s.p2align.min(12));
            out.resize(off, 0);
            out.extend_from_slice(&s.data);
            sect_offs.push(off as u32);
        }
        let fileoff = sect_offs.iter().copied().filter(|&o| o != 0).min().unwrap_or(0) as u64;
        let filesize = out.len() as u64 - fileoff;
        out.resize(out.len().next_multiple_of(8), 0);

        let table = self.symbol_table();
        let mut reloc_offs = Vec::with_capacity(nsects);
        for s in &self.sections {
            reloc_offs.push(out.len() as u32);
            for r in &s.relocs {
                let mut r = *r;
                if r.is_extern() {
                    r.bits = (r.bits & !0xff_ffff) | table.index[r.r_symbolnum() as usize];
                }
                out.extend_from_slice(r.as_bytes());
            }
        }
        let symoff = out.len().next_multiple_of(8);
        out.resize(symoff, 0);
        for n in &table.nlists {
            out.extend_from_slice(n.as_bytes());
        }
        let stroff = out.len();
        out.extend_from_slice(&table.strtab.data);
        out.resize(out.len().next_multiple_of(8), 0);

        let mut off = 0;
        let mut put = |out: &mut Vec<u8>, bytes: &[u8]| {
            out[off..off + bytes.len()].copy_from_slice(bytes);
            off += bytes.len();
        };
        let header = MachHeader {
            magic: MH_MAGIC_64,
            cputype: self.af.cputype,
            cpusubtype: self.af.cpusubtype,
            filetype: MH_OBJECT,
            ncmds: 4,
            sizeofcmds: cmds_size as u32,
            flags: MH_SUBSECTIONS_VIA_SYMBOLS,
            reserved: 0,
        };
        put(&mut out, header.as_bytes());
        let segment = SegmentCommand {
            cmd: LC_SEGMENT_64,
            cmdsize: seg_size as u32,
            vmsize: self.sections.iter().map(|s| s.addr + s.size).max().unwrap_or(0),
            fileoff,
            filesize,
            maxprot: 7,
            initprot: 7,
            nsects: nsects as u32,
            ..Default::default()
        };
        put(&mut out, segment.as_bytes());
        for (i, s) in self.sections.iter().enumerate() {
            let hdr = MachSection {
                sectname: s.sectname,
                segname: s.segname,
                addr: s.addr,
                size: s.size,
                offset: sect_offs[i],
                p2align: s.p2align as u32,
                reloff: if s.relocs.is_empty() { 0 } else { reloc_offs[i] },
                nreloc: s.relocs.len() as u32,
                flags: s.flags,
                ..Default::default()
            };
            put(&mut out, hdr.as_bytes());
        }
        let build = BuildVersionCommand {
            cmd: LC_BUILD_VERSION,
            cmdsize: size_of::<BuildVersionCommand>() as u32,
            platform: self.af.platform,
            minos: self.af.minos,
            sdk: self.af.sdk,
            ntools: 0,
        };
        put(&mut out, build.as_bytes());
        let symtab = SymtabCommand {
            cmd: LC_SYMTAB,
            cmdsize: size_of::<SymtabCommand>() as u32,
            symoff: symoff as u32,
            nsyms: table.nlists.len() as u32,
            stroff: stroff as u32,
            strsize: table.strtab.data.len() as u32,
        };
        put(&mut out, symtab.as_bytes());
        let dysymtab = DysymtabCommand {
            cmd: LC_DYSYMTAB,
            cmdsize: size_of::<DysymtabCommand>() as u32,
            ilocalsym: 0,
            nlocalsym: table.nlocal as u32,
            iextdefsym: table.nlocal as u32,
            nextdefsym: table.nextdef as u32,
            iundefsym: (table.nlocal + table.nextdef) as u32,
            nundefsym: (table.nlists.len() - table.nlocal - table.nextdef) as u32,
            ..Default::default()
        };
        put(&mut out, dysymtab.as_bytes());
        out
    }
}

/// The symbol table of the object being made.
struct SymbolTable {
    nlists: Vec<NList>,
    strtab: Strtab,
    /// Each symbol's index in `nlists`.
    index: Vec<u32>,
    /// The locals and the debug notes.
    nlocal: usize,
    nextdef: usize,
}

/// A name as the linker's labels are checked (see symbol_str).
fn symbol_str_lossy(name: &[u8]) -> &str {
    std::str::from_utf8(name).unwrap_or("")
}

/// Whether ld-prime notes the symbols of a section.
fn has_stabs(s: &Section) -> bool {
    let hdr = MachSection {
        sectname: s.sectname,
        segname: s.segname,
        flags: s.flags,
        ..Default::default()
    };
    crate::chunks::symtab::has_stabs(&hdr)
}

/// A string table, starting as ld64's do with " \0".
struct Strtab {
    data: Vec<u8>,
    index: hashbrown::HashMap<Vec<u8>, u32>,
}

impl Strtab {
    fn new() -> Self {
        Self { data: b" \0".to_vec(), index: hashbrown::HashMap::new() }
    }

    fn add(&mut self, s: &[u8]) -> u32 {
        if s.is_empty() {
            return 1;
        }
        if let Some(&i) = self.index.get(s) {
            return i;
        }
        let i = self.data.len() as u32;
        self.data.extend_from_slice(s);
        self.data.push(0);
        self.index.insert(s.to_vec(), i);
        i
    }
}

/// An atom's scope as an nlist's type and description bits.
fn scope_bits(scope: u8) -> (u8, u16) {
    match scope {
        scope::HIDDEN => (N_EXT | N_PEXT, 0),
        scope::AUTO_HIDE => (N_EXT, N_WEAK_DEF | N_WEAK_REF),
        scope::GLOBAL => (N_EXT, 0),
        scope::NEVER_STRIP => (N_EXT, REFERENCED_DYNAMICALLY),
        _ => (0, 0),
    }
}
