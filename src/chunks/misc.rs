//! Small synthesized sections.

use std::collections::{BTreeMap, BTreeSet};

use bstr::BStr;
use rayon::prelude::*;

use crate::arch::Arch;
use crate::chunks::{self, ChunkHeader, ChunkId, DynRelBuffer, OutputSectionId};
use crate::context::Context;
use crate::elf::*;
use crate::error;
use crate::input_files::FileId;
use crate::input_sections::{r_delta, InputSection};
use crate::symbol::SymbolId;
use crate::util::align_to;
use crate::util::compress::Compressor;
use crate::util::{path_filename, write_cstr};

// .interp contains the pathname of a dynamic linker. Dynamically-linked
// executables have the section. If exists, the kernel runs the program at
// the specified path with the executable pathname as an argument,
// allowing the dynamic linker to run the program.
#[derive(Debug)]
pub struct InterpSection {
    pub hdr: ChunkHeader,
}

impl InterpSection {
    pub fn new() -> InterpSection {
        InterpSection {
            hdr: ChunkHeader::new(".interp", SHT_PROGBITS, SHF_ALLOC as u64),
        }
    }
}

impl Default for InterpSection {
    fn default() -> Self {
        Self::new()
    }
}

pub mod interp {
    use super::*;

    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
        let size = ctx.args.dynamic_linker.len() as u64 + 1;
        ctx.interp.as_mut().unwrap().hdr.shdr.sh_size = size;
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        write_cstr(buf, ctx.args.dynamic_linker.as_bytes());
    }
}

// .copyrel and .copyrel.rel.ro represent memory regions to which the
// runtime copies symbols from other ELF files for copy relocations.
#[derive(Debug)]
pub struct CopyrelSection {
    pub hdr: ChunkHeader,
    pub symbols: Vec<SymbolId>,
}

impl CopyrelSection {
    pub fn new(is_relro: bool) -> CopyrelSection {
        let name = if is_relro {
            ".copyrel.rel.ro"
        } else {
            ".copyrel"
        };
        let mut hdr = ChunkHeader::new(name, SHT_NOBITS, (SHF_ALLOC | SHF_WRITE) as u64);
        hdr.is_relro = is_relro;
        CopyrelSection {
            hdr,
            symbols: Vec::new(),
        }
    }
}

pub mod copyrel {
    use super::*;

    pub fn add_symbol<E: Arch>(ctx: &mut Context<E>, relro: bool, id: SymbolId) {
        debug_assert!(!ctx.args.shared);
        let sym = &ctx.symbols[id];
        if sym.has_copyrel() {
            return;
        }
        let Some(FileId::Dso(dso_id)) = sym.file() else {
            debug_assert!(sym.is_undef_weak());
            let file = ctx.file_display(sym.file().unwrap());
            error!(
                ctx,
                "{file}: cannot create a copy relocation for {sym}; recompile with -fPIE or -fPIC"
            );
            return;
        };
        let dso = &ctx.dsos[dso_id.index()];
        if sym.esym(ctx).st_visibility() == STV_PROTECTED {
            error!(
                ctx,
                "{dso}: cannot create a copy relocation for protected symbol '{sym}'; recompile with -fPIC"
            );
            return;
        }
        if !ctx.args.z_copyreloc {
            error!(
                ctx,
                "-z nocopyreloc: {dso}: cannot create a copy relocation for symbol '{sym}'; recompile with -fPIC"
            );
            return;
        }

        let alignment = dso.alignment(sym);
        let size = sym.esym(ctx).st_size;
        // We need to create dynamic symbols not only for this particular symbol
        // but also for its aliases (i.e. other symbols at the same address)
        // becasue otherwise the aliases are broken apart at runtime.
        // For example, `environ`, `_environ` and `__environ` in libc.so are
        // aliases. If one of the symbols is copied by a copy relocation, other
        // symbols have to refer to the copied place as well.
        let aliases: Vec<SymbolId> = dso.symbols_at(ctx, sym, dso_id).to_vec();

        let sec = if relro {
            &mut ctx.copyrel_relro
        } else {
            &mut ctx.copyrel
        };
        sec.symbols.push(id);
        let offset = align_to(sec.hdr.shdr.sh_size, alignment);
        sec.hdr.shdr.sh_size = offset + size;
        sec.hdr.shdr.sh_addralign = sec.hdr.shdr.sh_addralign.max(alignment);

        for alias in aliases {
            ctx.symbols.aux_mut(alias);
            let s = &mut ctx.symbols[alias];
            s.set_imported(true);
            s.set_exported(true);
            s.set_copyrel(true);
            s.set_copyrel_readonly(relro);
            s.value = offset;
            chunks::symtab::dynsym::add_symbol(ctx, alias);
        }
    }

    pub fn write_dynrels<E: Arch>(
        ctx: &Context<E>,
        sec: &CopyrelSection,
        mut out: DynRelBuffer<'_, E>,
    ) {
        for (i, &id) in sec.symbols.iter().enumerate() {
            let sym = &ctx.symbols[id];
            out.write(
                i,
                ElfRel::new(
                    sym.addr(ctx),
                    E::R_COPY,
                    sym.dynsym_idx(&ctx.symbols).unwrap_or(0),
                    0,
                ),
            );
        }
        debug_assert_eq!(sec.symbols.len(), out.len());
    }
}

// .note.gnu.build-id contains an identifier for an output ELF file. The
// contents of the section is usually a cryptogrpahic hash of the output
// file itself to guarantee uniqueness of build-id.
#[derive(Debug)]
pub struct BuildIdSection {
    pub hdr: ChunkHeader,
    pub contents: Vec<u8>,
}

impl BuildIdSection {
    pub fn new() -> BuildIdSection {
        let mut hdr = ChunkHeader::new(".note.gnu.build-id", SHT_NOTE, SHF_ALLOC as u64);
        hdr.shdr.sh_addralign = 4;
        hdr.shdr.sh_size = 1;
        BuildIdSection {
            hdr,
            contents: Vec::new(),
        }
    }
}

impl Default for BuildIdSection {
    fn default() -> Self {
        Self::new()
    }
}

pub mod build_id {
    use super::*;

    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
        let size = ctx.args.build_id.size() as u64 + 16; // +16 for the header
        ctx.buildid.as_mut().unwrap().hdr.shdr.sh_size = size;
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        let sec = ctx.buildid.as_ref().unwrap();
        buf.fill(0);
        E::Endian::write_u32(buf, 4); // Name size
        E::Endian::write_u32(&mut buf[4..], ctx.args.build_id.size() as u32); // Hash size
        E::Endian::write_u32(&mut buf[8..], NT_GNU_BUILD_ID);
        buf[12..16].copy_from_slice(b"GNU\0"); // Name string
        buf[16..16 + sec.contents.len()].copy_from_slice(&sec.contents); // Build ID
    }
}

// .note.package is an optional hint section that can contain arbitrary
// string. Package managers, such as dpkg or rpm, uses the section to
// embed package metadata into each ELF file so that it is easy to find
// the origin of an ELF file without any additional information.
#[derive(Debug)]
pub struct NotePackageSection {
    pub hdr: ChunkHeader,
}

impl NotePackageSection {
    pub fn new() -> NotePackageSection {
        let mut hdr = ChunkHeader::new(".note.package", SHT_NOTE, SHF_ALLOC as u64);
        hdr.shdr.sh_addralign = 4;
        NotePackageSection { hdr }
    }
}

impl Default for NotePackageSection {
    fn default() -> Self {
        Self::new()
    }
}

pub mod note_package {
    use super::*;

    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
        if !ctx.args.package_metadata.is_empty() {
            // +17 is for the header and the NUL terminator
            ctx.note_package.hdr.shdr.sh_size =
                align_to(ctx.args.package_metadata.len() as u64 + 17, 4);
        }
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        buf.fill(0);
        E::Endian::write_u32(buf, 4); // Name size
        E::Endian::write_u32(&mut buf[4..], ctx.note_package.hdr.shdr.sh_size as u32 - 16); // Content size
        E::Endian::write_u32(&mut buf[8..], NT_FDO_PACKAGING_METADATA);
        buf[12..16].copy_from_slice(b"FDO\0");
        write_cstr(&mut buf[16..], ctx.args.package_metadata.as_bytes()); // Content
    }
}

// .note.gnu.property section contains an additional runtime information
// about ISA variant.
#[derive(Debug)]
pub struct NotePropertySection {
    pub hdr: ChunkHeader,
    pub contents: Vec<(u32, u32)>,
}

impl NotePropertySection {
    pub fn new<E: Arch>() -> NotePropertySection {
        let mut hdr = ChunkHeader::new(".note.gnu.property", SHT_NOTE, SHF_ALLOC as u64);
        hdr.shdr.sh_addralign = E::WORD_SIZE as u64;
        NotePropertySection {
            hdr,
            contents: Vec::new(),
        }
    }
}

pub mod note_property {
    use super::*;

    fn entry_size<E: Arch>() -> usize {
        if E::IS_64 {
            16
        } else {
            12
        }
    }

    // Merges input files' .note.gnu.property values.
    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
        // Obtain the list of keys
        let files: Vec<&crate::input_files::ObjectFile> = ctx
            .objs
            .iter()
            .filter(|file| !ctx.is_internal(file.id()))
            .collect();
        let keys: BTreeSet<u32> = files
            .iter()
            .flat_map(|f| f.gnu_properties.keys().copied())
            .collect();
        let value = |f: &crate::input_files::ObjectFile, key: u32| {
            f.gnu_properties.get(&key).copied().unwrap_or(0)
        };

        // Merge values for each key
        let mut map: BTreeMap<u32, u32> = BTreeMap::new();
        for key in keys {
            if (GNU_PROPERTY_X86_UINT32_AND_LO..=GNU_PROPERTY_X86_UINT32_AND_HI).contains(&key) {
                // An AND feature is set if all input objects have the property and
                // the feature.
                map.insert(
                    key,
                    files.iter().fold(u32::MAX, |acc, f| acc & value(f, key)),
                );
            } else if (GNU_PROPERTY_X86_UINT32_OR_LO..=GNU_PROPERTY_X86_UINT32_OR_HI).contains(&key)
            {
                // An OR feature is set if some input object has the feature.
                map.insert(key, files.iter().fold(0, |acc, f| acc | value(f, key)));
            } else if (GNU_PROPERTY_X86_UINT32_OR_AND_LO..=GNU_PROPERTY_X86_UINT32_OR_AND_HI)
                .contains(&key)
            {
                // An OR-AND feature is set if all input object files have the property
                // and some of them has the feature.
                if files.iter().all(|f| f.gnu_properties.contains_key(&key)) {
                    map.insert(key, files.iter().fold(0, |acc, f| acc | value(f, key)));
                }
            }
        }

        if ctx.args.z_ibt {
            *map.entry(GNU_PROPERTY_X86_FEATURE_1_AND).or_insert(0) |=
                GNU_PROPERTY_X86_FEATURE_1_IBT;
        }
        if ctx.args.z_shstk {
            *map.entry(GNU_PROPERTY_X86_FEATURE_1_AND).or_insert(0) |=
                GNU_PROPERTY_X86_FEATURE_1_SHSTK;
        }
        *map.entry(GNU_PROPERTY_X86_ISA_1_NEEDED).or_insert(0) |= ctx.args.z_x86_64_isa_level;

        // Serialize the map
        let contents: Vec<(u32, u32)> = map.into_iter().filter(|&(_, v)| v != 0).collect();
        let sec = ctx.note_property.as_mut().unwrap();
        sec.hdr.shdr.sh_size = if contents.is_empty() {
            0
        } else {
            (16 + contents.len() * entry_size::<E>()) as u64
        };
        sec.contents = contents;
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        let sec = ctx.note_property.as_ref().unwrap();
        buf.fill(0);
        E::Endian::write_u32(buf, 4); // Name size
        E::Endian::write_u32(&mut buf[4..], sec.hdr.shdr.sh_size as u32 - 16); // Content size
        E::Endian::write_u32(&mut buf[8..], NT_GNU_PROPERTY_TYPE_0);
        buf[12..16].copy_from_slice(b"GNU\0");
        for (i, &(ty, val)) in sec.contents.iter().enumerate() {
            let off = 16 + i * entry_size::<E>();
            E::Endian::write_u32(&mut buf[off..], ty);
            E::Endian::write_u32(&mut buf[off + 4..], 4);
            E::Endian::write_u32(&mut buf[off + 8..], val); // Content
        }
    }
}

// .gnu_debuglink section contains a pathname and its CRC32 checksum for a
// separate debug info file. gdb can read the section to read debug info
// from an external file.
#[derive(Debug)]
pub struct GnuDebuglinkSection {
    pub hdr: ChunkHeader,
    pub filename: String,
    pub crc32: u32,
}

impl GnuDebuglinkSection {
    pub fn new() -> GnuDebuglinkSection {
        let mut hdr = ChunkHeader::new(".gnu_debuglink", SHT_PROGBITS, 0);
        hdr.shdr.sh_addralign = 4;
        GnuDebuglinkSection {
            hdr,
            filename: String::new(),
            crc32: 0,
        }
    }
}

impl Default for GnuDebuglinkSection {
    fn default() -> Self {
        Self::new()
    }
}

pub mod gnu_debuglink {
    use super::*;

    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
        let filename = path_filename(&ctx.args.separate_debug_file);
        let sec = ctx.gnu_debuglink.as_mut().unwrap();
        sec.hdr.shdr.sh_size = align_to(filename.len() as u64 + 1, 4) + 4;
        sec.filename = filename;
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        let sec = ctx.gnu_debuglink.as_ref().unwrap();
        buf.fill(0);
        write_cstr(buf, sec.filename.as_bytes());
        let n = buf.len();
        E::Endian::write_u32(&mut buf[n - 4..], sec.crc32);
    }
}

// PT_GNU_RELRO works on page granularity. We want to align its end to
// a page boundary. We append this section at end of a segment so that
// the segment always ends at a page boundary.
#[derive(Debug)]
pub struct RelroPaddingSection {
    pub hdr: ChunkHeader,
}

impl RelroPaddingSection {
    pub fn new() -> RelroPaddingSection {
        let mut hdr =
            ChunkHeader::new(".relro_padding", SHT_NOBITS, (SHF_ALLOC | SHF_WRITE) as u64);
        hdr.is_relro = true;
        hdr.shdr.sh_size = 1;
        RelroPaddingSection { hdr }
    }
}

impl Default for RelroPaddingSection {
    fn default() -> Self {
        Self::new()
    }
}

// Debug sections can be compressed with zlib or zstd to reduce the
// overall size of an ELF file. CompressedSection represents a compressed
// section.
#[derive(Debug)]
pub struct CompressedSection {
    pub hdr: ChunkHeader,
    pub chdr: ElfChdr,
    pub compressor: Compressor,
    /// Kept for --gdb-index, which reads the uncompressed contents.
    pub uncompressed_data: Option<Vec<u8>>,
    pub original: ChunkId,
}

impl std::fmt::Debug for Compressor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Compressor({} bytes)", self.compressed_size())
    }
}

pub mod compressed {
    use super::*;

    pub fn new<E: Arch>(ctx: &Context<E>, original: ChunkId) -> CompressedSection {
        let hdr = ctx.chunk_header(original);

        // The C++ implementation avoids zero-initializing this scratch buffer:
        //
        // Allocate a temporary buffer to write uncompressed contents. Note
        // that we use u8[] instead of std::vector<u8> to avoid the cost of
        // zero-initialization, as sh_size can be very large.
        let mut buf = vec![0u8; hdr.shdr.sh_size as usize];

        // Write uncompressed contents and then compress them
        chunks::write_to(ctx, original, &mut buf);

        let level = ctx.args.compress_debug_sections_level;
        let compressor = if ctx.args.compress_debug_sections == ELFCOMPRESS_ZLIB {
            Compressor::zlib(&buf, level as u32)
        } else {
            Compressor::zstd(&buf, level as i32)
        };

        // Compute header field values
        let chdr = ElfChdr {
            ch_type: ctx.args.compress_debug_sections,
            ch_size: hdr.shdr.sh_size,
            ch_addralign: hdr.shdr.sh_addralign,
        };
        let mut new_hdr = ChunkHeader::with_name(
            hdr.name,
            hdr.shdr.sh_type,
            hdr.shdr.sh_flags | SHF_COMPRESSED as u64,
        );
        new_hdr.shndx = hdr.shndx;
        new_hdr.is_compressed = true;
        new_hdr.shdr = hdr.shdr;
        new_hdr.shdr.sh_flags |= SHF_COMPRESSED as u64;
        new_hdr.shdr.sh_addralign = 1;
        new_hdr.shdr.sh_size = (ElfChdr::size::<E>() + compressor.compressed_size()) as u64;

        // We can discard the uncompressed contents unless --gdb-index is given
        CompressedSection {
            hdr: new_hdr,
            chdr,
            compressor,
            uncompressed_data: ctx.args.gdb_index.then_some(buf),
            original,
        }
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, i: u32, buf: &mut [u8]) {
        let sec = &ctx.compressed_sections[i as usize];
        sec.chdr.write::<E>(buf);
        sec.compressor.write_to(&mut buf[ElfChdr::size::<E>()..]);
    }
}

// RelocSection represents a relocation table for an output file.
// This is used only for the relocatable output (i.e. the `-r` output).
#[derive(Debug)]
pub struct RelocSection {
    pub hdr: ChunkHeader,
    pub output_section: OutputSectionId,
    /// The index of the first relocation of each member.
    offsets: Vec<u64>,
}

pub mod reloc {
    use super::*;

    pub fn new<E: Arch>(ctx: &Context<E>, osec_id: OutputSectionId) -> RelocSection {
        let osec = &ctx.output_sections[osec_id.index()];
        let name = format!(
            "{}{}",
            if E::IS_RELA { ".rela" } else { ".rel" },
            osec.hdr.name
        );
        let name = BStr::new(crate::util::leak_bytes(name.into_bytes()));
        let mut hdr = ChunkHeader::with_name(
            name,
            if E::IS_RELA { SHT_RELA } else { SHT_REL },
            SHF_INFO_LINK as u64,
        );
        hdr.shdr.sh_addralign = E::WORD_SIZE as u64;
        hdr.shdr.sh_entsize = ElfRel::size::<E>() as u64;

        // Compute an offset for each input section
        let mut offsets = Vec::with_capacity(osec.members.len());
        let mut sum = 0u64;
        for &m in &osec.members {
            offsets.push(sum);
            let isec = ctx.input_section(m);
            let file = &ctx.objs[isec.file.index()];
            sum += isec.rels::<E>(file).len() as u64;
        }
        hdr.shdr.sh_size = sum * ElfRel::size::<E>() as u64;
        RelocSection {
            hdr,
            output_section: osec_id,
            offsets,
        }
    }

    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>, i: u32) {
        let symtab_shndx = ctx.symtab.hdr.shndx;
        let osec = ctx.reloc_sections[i as usize].output_section;
        let osec_shndx = ctx.output_sections[osec.index()].hdr.shndx;
        let sec = &mut ctx.reloc_sections[i as usize];
        sec.hdr.shdr.sh_link = symtab_shndx;
        sec.hdr.shdr.sh_info = osec_shndx;
    }

    // Translates an input relocation's symbol reference into the {r_sym, addend}
    // pair that is valid in the output file. The returned r_sym is either an output
    // section index (for section-relative relocs) or an output symbol table index.
    fn symidx_addend<E: Arch>(ctx: &Context<E>, isec: &InputSection, rel: &ElfRel) -> (u32, i64) {
        let file = &ctx.objs[isec.file.index()];
        let sym = &ctx.symbols[file.base.symbols[rel.r_sym as usize]];

        if !isec.is_alloc() {
            if let Some((frag, addend)) = isec.fragment(ctx, rel) {
                let msec = &ctx.merged_sections[frag.section.index()];
                return (
                    msec.hdr.shndx,
                    msec.fragments.get(frag.entry).offset() as i64 + addend,
                );
            }
        }

        if sym.st_type() == STT_SECTION {
            if let Some(frag) = sym.fragment() {
                let msec = &ctx.merged_sections[frag.section.index()];
                return (
                    msec.hdr.shndx,
                    msec.fragments.get(frag.entry).offset() as i64
                        + sym.value as i64
                        + isec.rel_addend::<E>(rel),
                );
            }
            if let Some(target) = sym.input_section_ref() {
                if let Some(osec) = target.output_section {
                    return (
                        ctx.output_section(osec).hdr.shndx,
                        isec.rel_addend::<E>(rel) + target.offset() as i64,
                    );
                }
            }
            // This is usually a dead debug section referring to a
            // COMDAT-eliminated section.
            return (0, 0);
        }

        if sym.write_to_symtab() {
            return (sym.output_sym_idx(ctx), isec.rel_addend::<E>(rel));
        }
        (0, 0)
    }

    /// Writes the relocations. With `-r` on a REL target, the addends are
    /// written into the output section's bytes, passed as `osec_buf`.
    pub fn copy_buf<E: Arch>(
        ctx: &Context<E>,
        i: u32,
        buf: &mut [u8],
        osec_buf: Option<&mut [u8]>,
    ) {
        let sec = &ctx.reloc_sections[i as usize];
        let osec = &ctx.output_sections[sec.output_section.index()];
        let size = ElfRel::size::<E>();
        let mut osec_buf = osec_buf;

        for (mi, &m) in osec.members.iter().enumerate() {
            let isec = ctx.input_section(m);
            let file = &ctx.objs[isec.file.index()];
            let base = sec.offsets[mi] as usize;
            for (j, rel) in isec.rels::<E>(file).iter().enumerate() {
                let (symidx, addend) = symidx_addend(ctx, isec, &rel);
                let mut r_offset = osec.hdr.shdr.sh_addr + isec.offset() + rel.r_offset;
                if E::IS_RISCV || E::IS_LOONGARCH {
                    // On RISC-V and LoongArch, relaxation may have deleted instructions,
                    // shifting this relocation's offset.
                    r_offset -= r_delta(isec, rel.r_offset) as u64;
                }

                // SH4 object files store addends in the relocated places rather
                // than in r_addend, and the relocation records we emit here are
                // meant to be consumed as if they were in an object file, so we
                // follow that convention.
                let out_addend = if E::FAMILY == crate::arch::Family::Sh4 {
                    0
                } else {
                    addend
                };
                let r_type = crate::arch::emitted_rel_type::<E>(ctx, isec, &rel, j);
                ElfRel::new(r_offset, r_type, symidx, out_addend)
                    .write::<E>(&mut buf[(base + j) * size..]);

                if ctx.args.relocatable {
                    if let Some(osec_buf) = osec_buf.as_deref_mut() {
                        let loc = (isec.offset() + rel.r_offset) as usize;
                        E::write_addend(&mut osec_buf[loc..], addend, &rel);
                    }
                }
            }
        }
    }
}

// ComdatGroupSection represents a comdat group for an output file.
// This is used only for the relocatable output (i.e. the `-r` output).
#[derive(Debug)]
pub struct ComdatGroupSection {
    pub hdr: ChunkHeader,
    pub sym: SymbolId,
    pub members: Vec<ChunkId>,
}

impl ComdatGroupSection {
    pub fn new(sym: SymbolId, members: Vec<ChunkId>) -> ComdatGroupSection {
        let mut hdr = ChunkHeader::new(".group", SHT_GROUP, 0);
        hdr.shdr.sh_entsize = 4;
        hdr.shdr.sh_addralign = 4;
        hdr.shdr.sh_size = (members.len() * 4 + 4) as u64;
        ComdatGroupSection { hdr, sym, members }
    }
}

pub mod comdat_group {
    use super::*;

    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>, i: u32) {
        debug_assert!(ctx.args.relocatable);
        let sec = &ctx.comdat_group_sections[i as usize];
        let sym = &ctx.symbols[sec.sym];
        let sh_info = if sym.st_type() == STT_SECTION {
            let isec = sym.input_section_ref().unwrap();
            ctx.output_section(isec.output_section.unwrap()).hdr.shndx
        } else {
            sym.output_sym_idx(ctx)
        };
        let symtab_shndx = ctx.symtab.hdr.shndx;
        let sec = &mut ctx.comdat_group_sections[i as usize];
        sec.hdr.shdr.sh_link = symtab_shndx;
        sec.hdr.shdr.sh_info = sh_info;
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, i: u32, buf: &mut [u8]) {
        let sec = &ctx.comdat_group_sections[i as usize];
        E::Endian::write_u32(buf, GRP_COMDAT);
        for (j, &member) in sec.members.iter().enumerate() {
            E::Endian::write_u32(&mut buf[4 + j * 4..], ctx.chunk_header(member).shndx);
        }
    }
}

/// Runs a function over the output sections in parallel.
pub fn for_each_output_section<E: Arch>(ctx: &Context<E>, f: impl Fn(OutputSectionId) + Sync) {
    (0..ctx.output_sections.len())
        .into_par_iter()
        .for_each(|i| f(OutputSectionId::new(i as u32)));
}

/// `.riscv.attributes` describes the ISA the output requires, merged
/// from the input files' attributes.
#[derive(Debug)]
pub struct RiscvAttributesSection {
    pub hdr: ChunkHeader,
    pub contents: Vec<u8>,
}

impl RiscvAttributesSection {
    pub fn new() -> RiscvAttributesSection {
        RiscvAttributesSection {
            hdr: ChunkHeader::new(".riscv.attributes", SHT_RISCV_ATTRIBUTES, 0),
            contents: Vec::new(),
        }
    }
}

impl Default for RiscvAttributesSection {
    fn default() -> Self {
        Self::new()
    }
}

pub mod riscv_attributes {
    use super::*;

    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
        if !ctx.riscv_attributes.as_ref().unwrap().contents.is_empty() {
            return;
        }
        let contents = crate::arch::riscv::attributes_contents(ctx);
        let sec = ctx.riscv_attributes.as_mut().unwrap();
        sec.hdr.shdr.sh_size = contents.len() as u64;
        sec.contents = contents;
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        let sec = ctx.riscv_attributes.as_ref().unwrap();
        buf[..sec.contents.len()].copy_from_slice(&sec.contents);
    }
}

/// `.save_restore_regs`, the register save and restore routines that GCC
/// expects the linker to provide on PowerPC64 ELFv2.
#[derive(Debug)]
pub struct Ppc64SaveRestoreSection {
    pub hdr: ChunkHeader,
}

impl Ppc64SaveRestoreSection {
    pub fn new() -> Ppc64SaveRestoreSection {
        let mut hdr = ChunkHeader::new(
            ".save_restore_regs",
            SHT_PROGBITS,
            (SHF_ALLOC | SHF_EXECINSTR) as u64,
        );
        hdr.shdr.sh_addralign = 16;
        hdr.shdr.sh_size = (crate::arch::ppc64v2::SAVE_RESTORE_INSNS.len() * 4) as u64;
        Ppc64SaveRestoreSection { hdr }
    }
}

impl Default for Ppc64SaveRestoreSection {
    fn default() -> Self {
        Self::new()
    }
}

pub mod ppc64_save_restore {
    use super::*;

    pub fn copy_buf<E: Arch>(_ctx: &Context<E>, buf: &mut [u8]) {
        buf.copy_from_slice(&crate::arch::ppc64v2::save_restore_contents());
    }
}
