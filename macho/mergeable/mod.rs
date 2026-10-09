//! Mergeable dylibs: a dylib ld-prime links with -make_mergeable keeps
//! a record of the subsections and symbols of its objects, which a
//! later link's -merge_framework, -merge_library or -merge-l takes in
//! place of a load command.
//!
//! LC_ATOM_INFO points at the record, a blob in __LINKEDIT in a format
//! of ld-prime's own (magic "nldprecr", file version 3): a header, then
//! tables of 40-byte entries, 16-byte fixups, large addends, custom
//! sections, symbol names, the dylib's own identity and the dylibs it
//! links, debug notes, and pools of strings and contents. An entry
//! stands for a subsection, a symbol or an import of the objects. Its
//! content is mostly the dylib's own linked bytes, which a negative
//! offset from the content pool reaches back to, so its fixups have to
//! be applied again: a pointer holds a chained fixup there, an
//! instruction its final immediate, a GOT load ld-prime relaxed an add
//! (a leaq on x86-64), while the fixup records what the object had.
//! Compact unwind records, which have no place in the image, are in the
//! content pool as the objects had them.
//!
//! This module has the format and reads the record; to merge, mold
//! turns the entries back into the object file they stand for (see
//! object.rs) and links that. make_mergeable writes the record in a
//! dylib linked with -make_mergeable.

use std::path::Path;

use mold_common::bits::sign_extend;
use mold_common::endian::{read_ul16, read_ul32, read_ul64};

use crate::input_files::load_commands;
use crate::macho::*;
use crate::mapped_file::MappedFile;

mod object;

pub use object::synthesize_object;

/// What ld-prime's -make_mergeable writes: a record whose entries
/// point into the image around it ("nldpatom" and "nldprdcr" are the
/// plain and the image-backed kinds it reads besides).
pub(crate) const MAGIC: &[u8; 8] = b"nldprecr";

/// The record header's size in file version 3; the entries follow it
/// directly.
pub(crate) const HEADER_SIZE: usize = 0xf8;
pub(crate) const ENTRY_SIZE: usize = 40;
pub(crate) const FIXUP_SIZE: usize = 16;
pub(crate) const SECTION_SIZE: usize = 44;
pub(crate) const DYLIB_INFO_SIZE: usize = 0x88;
pub(crate) const DEBUG_INFO_SIZE: usize = 0x98;

/// Where the header has its fields: the target, the flags, the place of
/// the image around the record, and the tables, each by its offset in
/// the record and its count (or a pool's size) after it.
pub(crate) mod header {
    pub const CPUTYPE: usize = 0x14;
    pub const CPUSUBTYPE: usize = 0x18;
    pub const PLATFORM: usize = 0x1c;
    pub const MINOS: usize = 0x20;
    pub const SDK: usize = 0x24;
    pub const FLAGS: usize = 0x28;
    /// The image's offset from the record (negative), and its size.
    pub const IMAGE: usize = 0x54;
    /// The record's own size.
    pub const SIZE: usize = 0x5c;
    pub const ENTRIES: usize = 0x60;
    pub const FIXUPS: usize = 0x68;
    pub const LARGE_ADDENDS: usize = 0x70;
    pub const SECTIONS: usize = 0x78;
    pub const LINKER_OPTIONS: usize = 0x80;
    pub const NAMES: usize = 0x88;
    pub const NAME_POOL: usize = 0x90;
    pub const CONTENT_POOL: usize = 0x98;
    pub const CSTRING_POOL: usize = 0xa0;
    /// The dylib's own info: its offset and size.
    pub const OWN_DYLIB: usize = 0xa8;
    /// The infos of the dylibs it links, and their total size after the
    /// count.
    pub const DYLIBS: usize = 0xb0;
    /// The debug notes, and their total size after the count.
    pub const DEBUG_INFOS: usize = 0xbc;
}

/// An entry's kind (bits 3-7 of its flags word).
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

/// An entry's scope (bits 0-2 of its flags word).
pub mod scope {
    /// Local to its object.
    pub const LOCAL: u8 = 0;
    pub const HIDDEN: u8 = 1;
    pub const AUTO_HIDE: u8 = 2;
    pub const GLOBAL: u8 = 3;
    pub const NEVER_STRIP: u8 = 4;
}

/// The content types (bits 8-14 of an entry's flags word) the reader
/// or the writer names; STANDARD_SECTIONS has the rest.
pub(crate) mod ctype {
    /// No bytes: an alias, an import.
    pub const NONE: u8 = 1;
    pub const METHOD_NAME: u8 = 12;
    pub const METHOD_LIST: u8 = 15;
    pub const DATA: u8 = 27;
    pub const CFI: u8 = 31;
    pub const COMPACT_UNWIND: u8 = 32;
    pub const CLASS_REF: u8 = 33;
    pub const SELECTOR_REF: u8 = 35;
    /// __objc_classlist and __objc_nlclslist.
    pub const CLASS_LISTS: [u8; 2] = [40, 44];
    /// __objc_catlist, __objc_nlcatlist and __objc_catlist2.
    pub const CATEGORY_LISTS: [u8; 3] = [41, 45, 70];
    pub const OBJC_IMAGE_INFO: u8 = 43;
    pub const INIT_OFFSET: u8 = 55;
    /// A section of the custom table.
    pub const CUSTOM: u8 = 63;
    pub const COMMON: u8 = 66;
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

/// An entry of the record: a subsection, a symbol or an import of the
/// mergeable dylib's objects. `C` and `F` are where its bytes and its
/// fixups are: for the reader, the bytes (none for zero fill, or what
/// the linker made) and a range of MergeableRecord::fixups; for the
/// writer, see make_mergeable::OutEntry.
#[derive(Clone, Debug)]
pub struct Entry<C = Option<&'static [u8]>, F = std::ops::Range<usize>> {
    pub name: Option<&'static [u8]>,
    pub scope: u8,
    pub kind: u8,
    pub content_type: u8,
    pub cold: bool,
    /// Live if what refers to it is (an unwind record, an alias).
    pub dds_if_refs_live: bool,
    pub no_dead_strip: bool,
    /// An import's strength: 1 a weak import, 2 a strong one, 0 none
    /// given.
    pub import: u8,
    /// An index into the custom sections.
    pub custom_section: Option<u8>,
    pub size: u32,
    pub content: C,
    /// An import's library, by index into the dylibs the mergeable one
    /// links.
    pub dylib: Option<u8>,
    pub p2align: u8,
    pub modulus: u16,
    /// 1-based index into the debug notes, 0 for none.
    pub debug: u16,
    pub fixups: F,
}

/// An entry's name's index in its 40 bytes when it has none.
const NO_NAME: u32 = 0xff_ffff;

impl<C, F> Entry<C, F> {
    /// The flags word: the scope (bits 0-2), kind (3-7) and content
    /// type (8-14), whether it is cold (15), live if what refers to it
    /// is (16) or never dead stripped (17), an import's strength
    /// (19-20) and the custom section (21-28, 0xff for none).
    fn flags(&self) -> u32 {
        self.scope as u32
            | (self.kind as u32) << 3
            | (self.content_type as u32) << 8
            | (self.cold as u32) << 15
            | (self.dds_if_refs_live as u32) << 16
            | (self.no_dead_strip as u32) << 17
            | (self.import as u32) << 19
            | (self.custom_section.unwrap_or(0xff) as u32) << 21
    }

    /// Writes the entry's 40 bytes (see Reader::entry): its index, the
    /// count and first index of its fixups, its name's index among the
    /// named entries, its flags word, its size and its content's offset
    /// from the content pool (-1 for none), then its import's library
    /// (0xff for none), alignment, modulus and debug notes.
    pub(crate) fn write(
        &self,
        out: &mut [u8],
        index: u32,
        fixups: (u32, u32),
        name: Option<u32>,
        content: i32,
    ) {
        out[0..4].copy_from_slice(&index.to_le_bytes());
        out[4..8].copy_from_slice(&fixups.0.to_le_bytes());
        out[8..12].copy_from_slice(&fixups.1.to_le_bytes());
        out[12..16].copy_from_slice(&name.unwrap_or(NO_NAME).to_le_bytes());
        out[16..20].copy_from_slice(&self.flags().to_le_bytes());
        out[20..24].copy_from_slice(&self.size.to_le_bytes());
        out[24..28].copy_from_slice(&content.to_le_bytes());
        out[0x1c] = self.dylib.unwrap_or(0xff);
        out[0x1d] = self.p2align;
        out[0x1e..0x20].copy_from_slice(&self.modulus.to_le_bytes());
        out[0x20..0x22].copy_from_slice(&self.debug.to_le_bytes());
    }
}

/// A fixup of an entry: its place, target and addend, and what its kind
/// carries besides - the entry subtracted (a difference), or an arm64
/// page offset's access size and, for the fused pairs, the distance to
/// the second instruction. `T` refers to an entry: by its index in the
/// record, or as the writer does until it numbers the entries.
#[derive(Clone, Copy, Debug)]
pub struct Fixup<T = u32> {
    pub offset: u32,
    pub target: T,
    pub kind: u16,
    pub addend: i64,
    pub from: Option<T>,
    /// How many bytes a page offset's load or store moves (1 for an add).
    pub scale: u8,
    /// The second instruction's distance from the first, in instructions.
    pub second: u8,
}

/// Bit 10 of a fixup's third word: bits 11-31 are the index of its
/// addend among the large ones.
const LARGE_ADDEND: u32 = 0x400;

impl Fixup {
    /// Reads a fixup's 16 bytes: its offset and target, then its kind
    /// (bits 0-9) and its addend (bits 11-31, or see LARGE_ADDEND), and
    /// a last word its kind uses as extras_usage says: for the rest of
    /// the addend, for the entry subtracted, or for the second
    /// instruction's distance (byte 0) and the access size (byte 1).
    fn read(c: &[u8], large_addends: &[i64]) -> Self {
        let (w2, w3) = (read_ul32(&c[8..]), read_ul32(&c[12..]));
        let kind = (w2 & 0x3ff) as u16;
        let usage = extras_usage(kind);
        let addend = if w2 & LARGE_ADDEND != 0 {
            large_addends[(w2 >> 11) as usize]
        } else {
            match usage {
                2 => w3 as i32 as i64,
                3 => sign_extend(((w2 >> 11) | ((w3 >> 25) << 21)) as u64, 28),
                4..=8 => sign_extend(((w2 >> 11) | ((w3 >> 16) << 21)) as u64, 32),
                _ => sign_extend((w2 >> 11) as u64, 21),
            }
        };
        let arm64 = (4..=8).contains(&usage);
        Self {
            offset: read_ul32(c),
            target: read_ul32(&c[4..]),
            kind,
            addend,
            from: (usage == 1).then_some(w3),
            scale: if arm64 { (w3 >> 8) as u8 } else { 0 },
            second: if arm64 { w3 as u8 } else { 0 },
        }
    }

    /// Writes the fixup's 16 bytes (see read). An addend too large for
    /// its kind's bits goes among the `large` ones, each value once.
    pub(crate) fn write(&self, out: &mut [u8], large: &mut Vec<i64>) {
        let usage = extras_usage(self.kind);
        let kind = self.kind as u32;
        let addend = self.addend;
        let fits = |bits: u32| addend >= -(1 << (bits - 1)) && addend < 1 << (bits - 1);
        let extras = match usage {
            1 => self.from.unwrap_or(0),
            4..=8 => (self.scale as u32) << 8 | self.second as u32,
            _ => 0,
        };
        let (low, high) = (addend as u32 & 0x1f_ffff, addend as u32 >> 21);
        let (w2, w3) = match usage {
            2 if fits(32) => (kind, addend as u32),
            4..=8 if fits(32) => (kind | low << 11, extras | high << 16),
            0 | 1 | 3 if fits(21) => (kind | low << 11, extras),
            _ => {
                let i = large.iter().position(|&v| v == addend).unwrap_or_else(|| {
                    large.push(addend);
                    large.len() - 1
                });
                (kind | LARGE_ADDEND | (i as u32) << 11, extras)
            }
        };
        out[0..4].copy_from_slice(&self.offset.to_le_bytes());
        out[4..8].copy_from_slice(&self.target.to_le_bytes());
        out[8..12].copy_from_slice(&w2.to_le_bytes());
        out[12..16].copy_from_slice(&w3.to_le_bytes());
    }
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

/// What a debug note (N_SO, N_OSO) says of the object an entry came from.
#[derive(Clone, Debug)]
pub struct DebugInfo {
    pub mtime: u32,
    pub source_dir: Vec<u8>,
    pub source_name: Vec<u8>,
    pub object_path: Vec<u8>,
}

/// A mergeable dylib's record, which LC_ATOM_INFO points at.
pub struct MergeableRecord {
    pub cputype: u32,
    pub cpusubtype: u32,
    pub platform: u32,
    pub minos: u32,
    pub sdk: u32,
    /// The record's flags word: the Swift versions of the objects'
    /// __objc_imageinfo (bits 0-23), MH_SUBSECTIONS_VIA_SYMBOLS (24),
    /// whether there was an image info (26), signed class ro data (28),
    /// category class properties (29) and classes (31).
    pub flags: u64,
    pub entries: Vec<Entry>,
    pub fixups: Vec<Fixup>,
    pub sections: Vec<CustomSection>,
    /// The mergeable dylib's own identity.
    pub own: DylibInfo,
    pub dylibs: Vec<DylibInfo>,
    pub debug_infos: Vec<DebugInfo>,
}

/// A dylib a mergeable dylib links, which the merging image loads in
/// its place: by the install name and versions recorded, exporting
/// what the entries import from it.
pub struct Dependency {
    pub path: std::path::PathBuf,
    pub info: DylibInfo,
    pub exports: Vec<&'static [u8]>,
    pub weak_exports: Vec<&'static [u8]>,
}

/// A mergeable dylib merged into the image: its install name, the OS
/// version it was built for, and the object its record makes.
pub struct MergedLibrary {
    pub install_name: Vec<u8>,
    pub minos: u32,
    pub obj: &'static crate::mapped_file::MappedFile,
}

/// Bit 31 of the record's flags: the dylib defines Objective-C or
/// Swift classes, for which ld-prime adds its hook to a merging image.
pub const FLAG_HAS_CLASSES: u64 = 1 << 31;
pub(crate) const FLAG_HAS_OBJC_INFO: u64 = 1 << 26;
pub(crate) const FLAG_SIGNED_CLASS_RO: u64 = 1 << 28;
pub(crate) const FLAG_CATEGORY_CLASS_PROPERTIES: u64 = 1 << 29;

/// Where a dylib's LC_ATOM_INFO data is in its file, if it has one.
fn record_range(data: &[u8]) -> Option<(usize, usize)> {
    let (_, cmd) = load_commands(data).find(|&(cmd, _)| cmd == LC_ATOM_INFO)?;
    let cmd = LinkEditDataCommand::read_from(cmd);
    Some((cmd.dataoff as usize, cmd.datasize as usize))
}

impl MergeableRecord {
    /// Reads the record of a mergeable dylib (see
    /// input_files::is_mergeable) as -make_mergeable wrote it: file
    /// version 3, the entries right after the header. A broken one may
    /// make it panic.
    pub fn read(mf: &'static MappedFile) -> Self {
        let file = mf.data();
        let (base, size) = record_range(file).unwrap();
        let blob = &file[base..base + size];
        let r = Reader { file, base, blob };
        let symbols = r.symbol_names();
        let fixups = r.fixups();
        let sections: Vec<CustomSection> = r
            .array(header::SECTIONS, SECTION_SIZE)
            .chunks(SECTION_SIZE)
            .map(|c| CustomSection {
                segname: c[8..24].try_into().unwrap(),
                sectname: c[26..42].try_into().unwrap(),
                flags: read_ul32(&c[4..]),
            })
            .collect();
        let (own, _) = r.dylib_info(r.table(header::OWN_DYLIB).0);
        let (at, count) = r.table(header::DYLIBS);
        let dylibs = r.dylib_infos(at, count);
        let debug_infos = r.debug_infos();
        let entries = r.entries(&symbols);
        Self {
            cputype: read_ul32(&blob[header::CPUTYPE..]),
            cpusubtype: read_ul32(&blob[header::CPUSUBTYPE..]),
            platform: read_ul32(&blob[header::PLATFORM..]),
            minos: read_ul32(&blob[header::MINOS..]),
            sdk: read_ul32(&blob[header::SDK..]),
            flags: read_ul64(&blob[header::FLAGS..]),
            entries,
            fixups,
            sections,
            own,
            dylibs,
            debug_infos,
        }
    }

    /// The dylibs the mergeable one links, each with the symbols its
    /// entries import from it.
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
        for entry in &self.entries {
            let (Some(dylib), Some(name)) = (entry.dylib, entry.name) else { continue };
            let dep = &mut deps[dylib as usize];
            dep.exports.push(name);
            if entry.kind == kind::DYLIB_EXPORT_WEAK_DEF {
                dep.weak_exports.push(name);
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

/// Access to the record's tables.
struct Reader<'a> {
    file: &'static [u8],
    base: usize,
    blob: &'a [u8],
}

impl Reader<'_> {
    /// A table's offset in the record and its count (or a pool's
    /// size), which the header has at `field`.
    fn table(&self, field: usize) -> (usize, usize) {
        (read_ul32(&self.blob[field..]) as usize, read_ul32(&self.blob[field + 4..]) as usize)
    }

    /// The bytes of a table of `elem`-byte elements (see table).
    fn array(&self, field: usize, elem: usize) -> &[u8] {
        let (off, count) = self.table(field);
        &self.blob[off..off + count * elem]
    }

    /// The `len` bytes `rel` bytes from blob offset `at`, which may
    /// reach back into the dylib around the blob.
    fn string_at(&self, at: usize, rel: i64, len: usize) -> &'static [u8] {
        let start = ((self.base + at) as i64 + rel) as usize;
        &self.file[start..start + len]
    }

    /// A CStringRO_1: an offset from the record, then a length.
    fn cstring(&self, at: usize) -> Vec<u8> {
        let len = read_ul64(&self.blob[at + 8..]) as usize;
        if len == 0 {
            return Vec::new();
        }
        self.string_at(at, read_ul64(&self.blob[at..]) as i64, len).to_vec()
    }

    fn symbol_names(&self) -> Vec<&'static [u8]> {
        let (first, count) = self.table(header::NAMES);
        (0..count)
            .map(|i| {
                let at = first + i * 16;
                let len = (read_ul64(&self.blob[at + 8..]) >> 44) as usize;
                self.string_at(at, read_ul64(&self.blob[at..]) as i64, len)
            })
            .collect()
    }

    /// The fixups, and the addends too large for theirs.
    fn fixups(&self) -> Vec<Fixup> {
        let large: Vec<i64> =
            self.array(header::LARGE_ADDENDS, 8).chunks(8).map(|c| read_ul64(c) as i64).collect();
        let table = self.array(header::FIXUPS, FIXUP_SIZE);
        table.chunks(FIXUP_SIZE).map(|c| Fixup::read(c, &large)).collect()
    }

    /// A DylibFileInfoRO_2 at blob offset `at`, and its size.
    fn dylib_info(&self, at: usize) -> (DylibInfo, usize) {
        let info = DylibInfo {
            install_name: self.cstring(at + 8),
            current_version: read_ul32(&self.blob[at..]),
            compatibility_version: read_ul32(&self.blob[at + 4..]),
        };
        let nplatforms = read_ul32(&self.blob[at + 0x2c..]) as usize;
        let lists: usize =
            [0x34, 0x3c, 0x44].iter().map(|&f| read_ul32(&self.blob[at + f..]) as usize).sum();
        (info, DYLIB_INFO_SIZE + (4 * nplatforms).next_multiple_of(8) + 16 * lists)
    }

    /// `count` DylibFileInfoRO_2 records one after another from `at`.
    fn dylib_infos(&self, at: usize, count: usize) -> Vec<DylibInfo> {
        let mut out = Vec::with_capacity(count);
        let mut pos = at;
        for _ in 0..count {
            let (info, size) = self.dylib_info(pos);
            out.push(info);
            pos += size;
        }
        out
    }

    fn debug_infos(&self) -> Vec<DebugInfo> {
        let (first, count) = self.table(header::DEBUG_INFOS);
        (0..count)
            .map(|i| {
                let at = first + i * DEBUG_INFO_SIZE;
                DebugInfo {
                    mtime: read_ul32(&self.blob[at..]),
                    source_dir: self.cstring(at + 8),
                    source_name: self.cstring(at + 0x18),
                    object_path: self.cstring(at + 0x28),
                }
            })
            .collect()
    }

    fn entries(&self, symbols: &[&'static [u8]]) -> Vec<Entry> {
        let table = self.array(header::ENTRIES, ENTRY_SIZE);
        table.chunks(ENTRY_SIZE).map(|c| self.entry(c, symbols)).collect()
    }

    /// Reads an entry's 40 bytes (see Entry::write), its name by its
    /// index into `symbols`, its bytes from the content pool.
    fn entry(&self, c: &[u8], symbols: &[&'static [u8]]) -> Entry {
        let (nfixups, first_fixup) = (read_ul32(&c[4..]) as usize, read_ul32(&c[8..]) as usize);
        let name = match read_ul32(&c[12..]) {
            NO_NAME => None,
            n => Some(symbols[n as usize]),
        };
        let flags = read_ul32(&c[16..]);
        let size = read_ul32(&c[20..]);
        let (pool, _) = self.table(header::CONTENT_POOL);
        let content = match read_ul32(&c[24..]) as i32 {
            -1 => None,
            off => Some(self.string_at(pool, off as i64, size as usize)),
        };
        let custom = (flags >> 21) as u8;
        Entry {
            name,
            scope: (flags & 7) as u8,
            kind: ((flags >> 3) & 0x1f) as u8,
            content_type: ((flags >> 8) & 0x7f) as u8,
            cold: flags & (1 << 15) != 0,
            dds_if_refs_live: flags & (1 << 16) != 0,
            no_dead_strip: flags & (1 << 17) != 0,
            import: ((flags >> 19) & 3) as u8,
            custom_section: (custom != 0xff).then_some(custom),
            size,
            content,
            dylib: (c[0x1c] != 0xff).then_some(c[0x1c]),
            p2align: c[0x1d],
            modulus: read_ul16(&c[0x1e..]),
            debug: read_ul16(&c[0x20..]),
            fixups: first_fixup..first_fixup + nfixups,
        }
    }
}

/// The sections ld-prime gives the entries of a content type (its
/// StandardSection::fromContentType): segment and section names and
/// Mach-O flags, as an object would have them.
const STANDARD_SECTIONS: &[(u8, &[u8], &[u8], u32)] = &[
    (2, b"__TEXT", b"__text", TEXT),
    (9, b"__TEXT", b"__const", 0),
    (10, b"__TEXT", b"__cstring", S_CSTRING_LITERALS),
    (11, b"__TEXT", b"__objc_classname", S_CSTRING_LITERALS),
    (12, b"__TEXT", b"__objc_methname", S_CSTRING_LITERALS),
    (13, b"__TEXT", b"__objc_methtype", S_CSTRING_LITERALS),
    (14, b"__TEXT", b"__oslogstring", S_CSTRING_LITERALS),
    (15, b"__TEXT", b"__objc_methlist", 0),
    (16, b"__TEXT", b"__ustring", 0),
    (17, b"__TEXT", b"__literal4", S_4BYTE_LITERALS),
    (18, b"__TEXT", b"__literal8", S_8BYTE_LITERALS),
    (19, b"__TEXT", b"__literal16", S_16BYTE_LITERALS),
    // A slot of an object's GOT, which an object has as a regular
    // section (one of non-lazy pointers is refused).
    (22, b"__DATA", b"__got", 0),
    (26, b"__DATA", b"__const", 0),
    (27, b"__DATA", b"__data", 0),
    (28, b"__DATA", b"__cfstring", 0),
    (29, b"__DATA", b"__const_cfobj2", 0),
    (30, b"__TEXT", b"__gcc_except_tab", 0),
    (31, b"__TEXT", b"__eh_frame", EH_FRAME),
    (32, b"__LD", b"__compact_unwind", S_ATTR_DEBUG),
    (33, b"__DATA", b"__objc_classrefs", S_ATTR_NO_DEAD_STRIP),
    (34, b"__DATA", b"__objc_superrefs", S_ATTR_NO_DEAD_STRIP),
    (35, b"__DATA", b"__objc_selrefs", S_LITERAL_POINTERS | S_ATTR_NO_DEAD_STRIP),
    (36, b"__DATA", b"__objc_protorefs", S_COALESCED | S_ATTR_NO_DEAD_STRIP),
    (37, b"__DATA", b"__objc_ivar", 0),
    (38, b"__DATA", b"__objc_data", 0),
    (39, b"__DATA", b"__objc_const", 0),
    (40, b"__DATA", b"__objc_classlist", S_ATTR_NO_DEAD_STRIP),
    (41, b"__DATA", b"__objc_catlist", S_ATTR_NO_DEAD_STRIP),
    (42, b"__DATA", b"__objc_protolist", S_COALESCED),
    (43, b"__DATA", b"__objc_imageinfo", 0),
    (44, b"__DATA", b"__objc_nlclslist", S_ATTR_NO_DEAD_STRIP),
    (45, b"__DATA", b"__objc_nlcatlist", S_ATTR_NO_DEAD_STRIP),
    (46, b"__DATA", b"__objc_intobj", 0),
    (47, b"__DATA", b"__objc_floatobj", 0),
    (48, b"__DATA", b"__objc_doubleobj", 0),
    (49, b"__DATA", b"__objc_dateobj", 0),
    (50, b"__DATA", b"__objc_dictobj", 0),
    (51, b"__DATA", b"__objc_arrayobj", 0),
    (52, b"__DATA", b"__objc_arraydata", 0),
    (53, b"__DATA", b"__mod_init_func", S_MOD_INIT_FUNC_POINTERS),
    (54, b"__DATA", b"__mod_term_func", S_MOD_TERM_FUNC_POINTERS),
    // An initializer offset the dylib's link made of a pointer is the
    // pointer again (see object::Synth::place).
    (55, b"__DATA", b"__mod_init_func", S_MOD_INIT_FUNC_POINTERS),
    (56, b"__TEXT", b"__StaticInit", TEXT),
    (57, b"__DATA", b"__thread_vars", S_THREAD_LOCAL_VARIABLES),
    (58, b"__DATA", b"__thread_ptrs", S_THREAD_LOCAL_VARIABLE_POINTERS),
    (64, b"__DATA", b"__thread_data", S_THREAD_LOCAL_REGULAR),
    (65, b"__DATA", b"__thread_bss", S_THREAD_LOCAL_ZEROFILL),
    (66, b"__DATA", b"__common", S_ZEROFILL),
    (67, b"__DATA", b"__bss", S_ZEROFILL),
    (70, b"__DATA", b"__objc_catlist2", S_ATTR_NO_DEAD_STRIP),
    (72, b"__DATA", b"__objc_clsrolist", S_ATTR_NO_DEAD_STRIP),
    (73, b"__LD", b"__func_variants", 0),
];

const TEXT: u32 = S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
const EH_FRAME: u32 = S_COALESCED | S_ATTR_NO_TOC | S_ATTR_STRIP_STATIC_SYMS | S_ATTR_LIVE_SUPPORT;

/// The standard section of a content type: its segment and section
/// names and flags.
pub(crate) fn standard_section(ct: u8) -> Option<(&'static [u8], &'static [u8], u32)> {
    let &(_, seg, sect, flags) = STANDARD_SECTIONS.iter().find(|s| s.0 == ct)?;
    Some((seg, sect, flags))
}

/// The content type of the standard section an object's section is, by
/// its names and type: a section of initializer pointers is one of
/// them, not of the offsets made of them.
pub(crate) fn standard_content_type(hdr: &MachSection) -> Option<u8> {
    let &(ct, ..) = STANDARD_SECTIONS.iter().find(|&&(ct, seg, sect, flags)| {
        ct != ctype::INIT_OFFSET
            && hdr.segname() == seg
            && hdr.sectname() == sect
            && hdr.section_type() == flags & SECTION_TYPE
    })?;
    Some(ct)
}
