//! -r: relocatable output.
//!
//! `ld -r` combines object files into one bigger object file instead
//! of a final image: sections are merged and laid out from address 0
//! in a single nameless segment, symbols keep their definitions and
//! undefined references, and - the essential part - relocations are
//! *regenerated* against the merged section and symbol tables rather
//! than applied. No dyld structures, no code signature.
//!
//! Section contents are copied raw (relocations stay unapplied), so
//! fields that embed addends keep them; only non-external relocations
//! need their embedded target addresses rewritten into the merged
//! address space. Literals are not deduplicated (the final link does
//! that), so every input label, and every relocation's target, carries
//! through as it was. DWARF is not merged (its section-relative offsets
//! carry no relocations); like ld64, the output gets debug-note stabs
//! naming the input objects, which a later link carries through.

use std::sync::atomic::{AtomicU16, AtomicU32, Ordering};

use hashbrown::{HashMap, HashSet};
use mold_common::bits::align_to;
use mold_common::bytes::display;
use mold_common::error;
use mold_common::fatal;
use mold_common::leb128::encode_uleb;
use mold_common::mem::leak_bytes;
use rayon::prelude::*;

use crate::arch::Target;
use crate::chunks::symtab::{SymtabSection, par_push_entries};
use crate::chunks::{ChunkHeader, OutputSectionId};
use crate::context::Context;
use crate::input_files::{FileId, LocalSymbol};
use crate::input_sections::{InputSection, Reloc, RelocTarget};
use crate::macho::*;
use crate::output_file;
use crate::symbol::SymbolId;

/// N_NO_DEAD_STRIP for a symbol from this input section: ld-prime
/// marks every symbol of a no_dead_strip section, local or global, so
/// the next link keeps it even when the output section takes another
/// member's attributes - but none of __objc_classrefs.
pub(crate) fn section_desc<E: Target>(ctx: &Context<E>, isec: usize) -> u16 {
    let isec = &ctx.isecs[isec];
    let h = isec.hdr(&ctx.objs[isec.file as usize]);
    if h.flags.get() & S_ATTR_NO_DEAD_STRIP != 0
        && !(h.segname() == b"__DATA" && h.sectname() == b"__objc_classrefs")
    {
        N_NO_DEAD_STRIP
    } else {
        0
    }
}

/// The payload of the output's LC_LINKER_OPTIMIZATION_HINT, or None
/// for no command. ld64 carries the arm64 hints through -r for the
/// final link to apply: each hint it takes (see
/// ObjectFile::hint_subsec) moves with its subsection, and goes with a
/// coalesced-away weak copy. As in ld64's output, the hints are
/// written subsection by subsection in address order, each
/// subsection's in input order: ULEB128 kind, count and addresses,
/// zero-padded to 8 bytes. (ld-prime 27037 drops them all.)
fn optimization_hints<E: Target>(ctx: &Context<E>) -> Option<Vec<u8>> {
    // The subsection's new and input addresses, and the hint.
    let mut hints = Vec::new();
    for obj in ctx.objs.iter().filter(|o| o.is_reachable) {
        for hint in &obj.loh {
            let Some(id) = obj.hint_subsec(&ctx.isecs, &hint.1) else {
                continue;
            };
            let isec = &ctx.isecs[id];
            if isec.is_emitted() {
                hints.push((isec.addr(ctx), isec.input_addr as u64, hint));
            }
        }
    }
    if hints.is_empty() {
        return None;
    }

    hints.sort_by_key(|h| h.0);
    let mut buf = Vec::new();
    for (addr, input_addr, (kind, addrs)) in hints {
        encode_uleb(&mut buf, *kind as u64);
        encode_uleb(&mut buf, addrs.len() as u64);
        for a in addrs {
            encode_uleb(&mut buf, addr + a - input_addr);
        }
    }
    buf.resize(align_to(buf.len() as u64, 8) as usize, 0);
    Some(buf)
}

/// The auto-link options (LC_LINKER_OPTION) a -r output carries for the
/// final link to act on (see reader::read_linker_options): those of
/// -add_linker_option and of its inputs as they are, in that order, each
/// only once - what a link of the inputs would see. (ld-prime rewrites
/// them, one per library, which loses -force_load, -weak_framework and
/// a framework's ",suffix".)
fn relocatable_linker_options<E: Target>(ctx: &Context<E>) -> Vec<&[Vec<u8>]> {
    if ctx.args.ignore_auto_link {
        return Vec::new();
    }
    let objs = ctx.objs.iter().filter(|obj| obj.is_reachable).flat_map(|obj| &obj.linker_options);
    let mut seen = HashSet::new();
    (ctx.cmdline_linker_options.iter().flatten().chain(objs))
        .filter(|opt| seen.insert(*opt))
        .map(Vec::as_slice)
        .collect()
}

/// Writes the -r output, returning its size.
pub fn combine_objects<E: Target>(ctx: &mut Context<E>) -> u64 {
    // The sections the output synthesizes: the merged __objc_imageinfo,
    // the re-synthesized __LD,__compact_unwind and __TEXT,__eh_frame.
    let t = ctx.timer("r-layout");
    let mut synthetic: Vec<SyntheticSection> =
        [objc_imageinfo_section(ctx), compact_unwind_section(ctx), eh_frame_section(ctx)]
            .into_iter()
            .flatten()
            .collect();

    // Every section - the -sectcreate options' own ones too - laid out
    // from address zero, and placed in the file right after the load
    // commands.
    let sects = sort_output_sections(ctx, &synthetic);
    let vmsize = assign_addresses(ctx, &mut synthetic, &sects);
    let cmds = LoadCommands::new(ctx, sects.len());
    let cmds_end = (size_of::<MachHeader>() + cmds.size()) as u64;
    let (seg_fileoff, content_end) = set_osec_offsets(ctx, &mut synthetic, &sects, cmds_end);
    drop(t);

    // The symbol table, then what refers to its symbols: the synthetic
    // sections' contents and the relocations, regenerated against the
    // merged tables.
    let ctx = &*ctx;
    let t = ctx.timer("r-symtab");
    let symtab = create_output_symtab(ctx);
    drop(t);
    let t = ctx.timer("r-relocs");
    let targets = RelocTargets::new(ctx, &symtab);
    for sec in &mut synthetic {
        sec.build_contents(&targets);
    }
    let merged_relocs: Vec<Vec<MachRel>> = (0..ctx.output_sections.len())
        .into_par_iter()
        .map(|i| section_relocs(&targets, OutputSectionId::new(i as u32)))
        .collect();
    // Each section's relocations, in output order.
    let relocs: Vec<&[MachRel]> = sects
        .iter()
        .map(|&s| match s {
            Sect::Merged(i) => &merged_relocs[i.index()][..],
            Sect::Synthetic(i) => &synthetic[i].relocs[..],
            Sect::Created(_) => &[],
        })
        .collect();
    drop(t);

    let layout = FileLayout::new(&cmds, &symtab, &relocs, vmsize, seg_fileoff, content_end);
    let t = ctx.timer("r-copy");
    let buf = write_object(&targets, &synthetic, &sects, &cmds, &relocs, &layout);
    drop(t);

    mold_common::error::checkpoint();
    // The reports come before the output, which may not be writable.
    // Xcode asks every link, its single-object prelinks included, for
    // -dependency_info and fails the build if the file is missing.
    crate::mapfile::write_dependency_info(ctx);
    if ctx.args.map.is_some() {
        let sections: Vec<crate::mapfile::MapSection> = (sects.iter())
            .map(|&s| match s {
                Sect::Merged(i) => (sect_hdr(ctx, &synthetic, s), Some(i)),
                _ => (sect_hdr(ctx, &synthetic, s), None),
            })
            .collect();
        crate::mapfile::print_map_of(ctx, &sections);
    }
    mold_common::error::checkpoint();
    let t = ctx.timer("r-write");
    output_file::write(&ctx.args.output, &buf);
    drop(t);
    buf.len() as u64
}

/// Builds the -r output in memory: the header and load commands, the
/// sections' contents, their relocations, then data in code, hints,
/// symbols and strings, where `layout` places them.
fn write_object<E: Target>(
    targets: &RelocTargets<E>,
    synthetic: &[SyntheticSection],
    sects: &[Sect],
    cmds: &LoadCommands,
    relocs: &[&[MachRel]],
    layout: &FileLayout,
) -> Vec<u8> {
    let ctx = targets.ctx;
    let headers: Vec<MachSection> = (sects.iter().zip(relocs).zip(&layout.reloffs))
        .map(|((&s, rels), &off)| section_header(sect_hdr(ctx, synthetic, s), rels, off))
        .collect();
    let merged: Vec<OutputSectionId> = (sects.iter())
        .filter_map(|s| match *s {
            Sect::Merged(i) => Some(i),
            _ => None,
        })
        .collect();

    let mut buf = vec![0u8; output_file::buffer_len(&ctx.args.output, layout.size)];
    write_load_commands(ctx, &mut buf, cmds, &headers, layout, targets.symtab);
    copy_section_contents(targets, &merged, &mut buf);
    for sec in synthetic {
        let fileoff = sec.hdr.fileoff as usize;
        buf[fileoff..fileoff + sec.data.len()].copy_from_slice(&sec.data);
    }
    for sec in &ctx.sectcreate_sections {
        let fileoff = sec.hdr.fileoff as usize;
        buf[fileoff..fileoff + sec.contents.len()].copy_from_slice(sec.contents);
    }
    for (rels, &off) in relocs.iter().zip(&layout.reloffs) {
        MachRel::write_all(rels, &mut buf[off as usize..]);
    }
    crate::chunks::data_in_code::write_entries(&cmds.dice, &mut buf[layout.diceoff as usize..]);
    if let Some(loh) = &cmds.loh {
        let lohoff = layout.lohoff as usize;
        buf[lohoff..lohoff + loh.len()].copy_from_slice(loh);
    }
    let symtab = &targets.symtab.table;
    let (syms, strtab) =
        buf[layout.symoff as usize..].split_at_mut(symtab.len() * size_of::<MachSym>());
    let strtab = &mut strtab[..symtab.strtab_size];
    crate::chunks::symtab::copy_buf(ctx, symtab, syms, strtab);
    buf
}

/// The local symbols that name the -sectcreate input sections, which
/// ld-prime lists after the objects' locals in command-line order:
/// "l<sect-create>" and the section's name as the option spelled it,
/// whichever section each went to, marked no-dead-strip so that a later
/// link keeps the data nothing refers to.
fn sectcreate_locals<E: Target>(ctx: &Context<E>) -> Vec<LocalSymbol> {
    (ctx.sectcreate_inputs.iter().zip(&ctx.args.sectcreate))
        .map(|(input, sc)| {
            let (value, sect) = input.place(ctx);
            LocalSymbol {
                name: leak_bytes(
                    [b"l<sect-create>", &sc.segname[..], b",", &sc.sectname[..]].concat(),
                ),
                msym: MachSym {
                    stroff: U32::new(0),
                    n_type: N_SECT,
                    sect,
                    desc: U16::new(N_NO_DEAD_STRIP),
                    value: U64::new(value),
                },
                hidden: false,
                sym: None,
            }
        })
        .collect()
}

/// A section of the -r output: merged from input subsections,
/// synthetic, or one the -sectcreate options made of a name no input
/// section has (an index into Context::sectcreate_sections).
#[derive(Clone, Copy)]
enum Sect {
    Merged(OutputSectionId),
    Synthetic(usize),
    Created(usize),
}

fn sect_hdr<'a, E: Target>(
    ctx: &'a Context<E>,
    synthetic: &'a [SyntheticSection],
    s: Sect,
) -> &'a ChunkHeader {
    match s {
        Sect::Merged(i) => &ctx.output_section(i).hdr,
        Sect::Synthetic(i) => &synthetic[i].hdr,
        Sect::Created(i) => &ctx.sectcreate_sections[i].hdr,
    }
}

fn sect_hdr_mut<'a, E: Target>(
    ctx: &'a mut Context<E>,
    synthetic: &'a mut [SyntheticSection],
    s: Sect,
) -> &'a mut ChunkHeader {
    match s {
        Sect::Merged(i) => &mut ctx.output_section_mut(i).hdr,
        Sect::Synthetic(i) => &mut synthetic[i].hdr,
        Sect::Created(i) => &mut ctx.sectcreate_sections[i].hdr,
    }
}

/// The sections of the output: the merged ones in creation order (that
/// of their first members), the ones only -sectcreate makes, then the
/// synthetic ones. A later link orders the sections by its own rules.
fn sort_output_sections<E: Target>(ctx: &Context<E>, synthetic: &[SyntheticSection]) -> Vec<Sect> {
    let merged =
        (0..ctx.output_sections.len()).map(|i| Sect::Merged(OutputSectionId::new(i as u32)));
    let own = (0..ctx.sectcreate_sections.len()).map(Sect::Created);
    merged.chain(own).chain((0..synthetic.len()).map(Sect::Synthetic)).collect()
}

/// Assigns the sections their addresses, from zero in output order, and
/// their ordinals, 1-based positions among them. Zero-fill sections take
/// address space like any other (ld64 leaves them in place too).
/// Returns the size of the address space.
fn assign_addresses<E: Target>(
    ctx: &mut Context<E>,
    synthetic: &mut [SyntheticSection],
    sects: &[Sect],
) -> u64 {
    let mut addr = 0;
    for (i, &s) in sects.iter().enumerate() {
        let hdr = sect_hdr_mut(ctx, synthetic, s);
        addr = align_to(addr, 1 << hdr.p2align);
        hdr.addr = addr;
        hdr.sect_idx = i as u8 + 1;
        addr += hdr.size;
    }
    addr
}

/// A -r output's load commands, in ld64's order: the single nameless
/// segment with every section, the symbol table, the build version,
/// data in code, then the carried auto-link options and hints. A -r
/// output has no LC_DYSYMTAB (ld-prime writes none). Their sizes, which
/// place the section contents, are known once the sections are laid
/// out; the offsets they record, once the whole file is.
struct LoadCommands {
    nsects: usize,
    /// LC_BUILD_VERSION or LC_VERSION_MIN_MACOSX, or nothing.
    version: Vec<u8>,
    /// The LC_LINKER_OPTION commands.
    linker_options: Vec<Vec<u8>>,
    /// LC_DATA_IN_CODE's entries, between the relocations and the
    /// symbol table and present even with no entries (ld-prime): the
    /// inputs' entries at their merged addresses, which is what an
    /// object's entries hold rather than file offsets.
    dice: Vec<(u32, u16, u16)>,
    /// LC_LINKER_OPTIMIZATION_HINT's payload, if the command is present.
    loh: Option<Vec<u8>>,
}

impl LoadCommands {
    fn new<E: Target>(ctx: &Context<E>, nsects: usize) -> Self {
        let args = &ctx.args;
        let version = if crate::chunks::has_version_cmd(args) {
            crate::chunks::create_version_cmd::<E>(
                args.platform,
                args.platform_minos,
                args.platform_sdk,
            )
        } else {
            Vec::new()
        };
        Self {
            nsects,
            version,
            linker_options: relocatable_linker_options(ctx)
                .iter()
                .map(|opt| crate::chunks::create_linker_option_cmd(opt))
                .collect(),
            dice: crate::chunks::data_in_code::construct(ctx, |hdr| hdr.addr),
            loh: optimization_hints(ctx),
        }
    }

    fn count(&self) -> u32 {
        3 + u32::from(!self.version.is_empty())
            + self.linker_options.len() as u32
            + u32::from(self.loh.is_some())
    }

    fn size(&self) -> usize {
        size_of::<SegmentCommand>()
            + self.nsects * size_of::<MachSection>()
            + size_of::<SymtabCommand>()
            + self.version.len()
            + size_of::<LinkEditDataCommand>()
            + self.linker_options.iter().map(Vec::len).sum::<usize>()
            + if self.loh.is_some() { size_of::<LinkEditDataCommand>() } else { 0 }
    }
}

/// Places the sections' contents in the file past the load commands,
/// which end at `cmds_end`, each aligned as in the address space, and
/// returns where they start and end. A zero-fill section takes no file
/// space. The contents start aligned for every section, so that none
/// lies further into them than into the address space: the segment's
/// file size is no larger than its size.
fn set_osec_offsets<E: Target>(
    ctx: &mut Context<E>,
    synthetic: &mut [SyntheticSection],
    sects: &[Sect],
    cmds_end: u64,
) -> (u64, u64) {
    let p2align = sects.iter().map(|&s| sect_hdr(ctx, synthetic, s).p2align).max();
    let start = align_to(cmds_end, 1 << p2align.unwrap_or(0));
    let mut off = start;
    for &s in sects {
        let hdr = sect_hdr_mut(ctx, synthetic, s);
        if hdr.is_zerofill() {
            hdr.fileoff = 0;
            continue;
        }
        off = align_to(off, 1 << hdr.p2align);
        hdr.fileoff = off;
        off += hdr.size;
    }
    (start, off)
}

/// A section the -r output synthesizes rather than merges from input
/// subsections. Its size is known before layout, so it takes its place
/// after the merged sections; its contents are built once addresses
/// and symbols are assigned.
struct SyntheticSection {
    hdr: ChunkHeader,
    kind: SyntheticKind,
    data: Vec<u8>,
    relocs: Vec<MachRel>,
}

/// What a synthetic section holds.
enum SyntheticKind {
    /// The merged __objc_imageinfo record.
    ObjcImageInfo,
    /// __LD,__compact_unwind's entries: these unwind records'.
    CompactUnwind(Vec<usize>),
    /// __TEXT,__eh_frame's records, each at its offset there.
    EhFrame(Vec<(EhRec, u32)>),
}

impl SyntheticSection {
    fn new(
        segname: &'static [u8],
        sectname: &'static [u8],
        flags: u32,
        p2align: u32,
        size: u64,
        kind: SyntheticKind,
    ) -> Self {
        let mut hdr = ChunkHeader::new(segname, sectname);
        hdr.flags = flags;
        hdr.p2align = p2align;
        hdr.size = size;
        Self { hdr, kind, data: Vec::new(), relocs: Vec::new() }
    }

    /// Builds the contents and relocations, which fill the size the
    /// section was laid out with.
    fn build_contents<E: Target>(&mut self, targets: &RelocTargets<E>) {
        let (data, relocs) = match &self.kind {
            SyntheticKind::ObjcImageInfo => {
                let mut data = vec![0u8; 8];
                crate::chunks::objc_imageinfo::copy_buf(targets.ctx, &mut data);
                (data, Vec::new())
            }
            SyntheticKind::CompactUnwind(records) => compact_unwind_contents(targets, records),
            SyntheticKind::EhFrame(records) => eh_frame_contents(targets, records, self.hdr.addr),
        };
        debug_assert_eq!(data.len() as u64, self.hdr.size);
        self.data = data;
        self.relocs = relocs;
    }
}

/// The merged __objc_imageinfo, if any input has one
/// (create_synthetic_sections folded the inputs' records into
/// ctx.objc_imageinfo.flags). The record is what makes the
/// Objective-C runtime look at an image at all: without it, dyld
/// never hands the image to the runtime, so no class or category it
/// defines is registered (a class referenced from another image then
/// dies with "Attempt to use unknown class", and categories on
/// framework classes never attach). A prelinked object lacking it
/// silently poisons the image that links it. ld64 writes it into
/// __DATA in a -r output, and ld-prime renames it like the input
/// sections (but not __eh_frame and __compact_unwind).
fn objc_imageinfo_section<E: Target>(ctx: &Context<E>) -> Option<SyntheticSection> {
    if !ctx.objs.iter().any(|o| o.is_reachable && o.objc_image_info.is_some()) {
        return None;
    }
    let (seg, sect) = crate::passes::renamed(&ctx.args, (b"__DATA", b"__objc_imageinfo"));
    Some(SyntheticSection::new(seg, sect, 0, 2, 8, SyntheticKind::ObjcImageInfo))
}

/// The unwind records __LD,__compact_unwind carries: every surviving
/// one. An input's DWARF-mode record is copied as it came (ld64 does;
/// the next link regenerates its encoding from the FDE), but a record
/// we synthesized from an FDE alone is not: the next link synthesizes
/// it again from the __eh_frame the output carries. A coalesced-away
/// weak definition's record goes with it.
fn compact_unwind_records<E: Target>(ctx: &Context<E>) -> Vec<usize> {
    (0..ctx.unwind_records.len())
        .into_par_iter()
        .filter(|&i| {
            let rec = &ctx.unwind_records[i];
            let isec = &ctx.isecs[rec.isec as usize];
            isec.is_emitted()
                && (rec.fde().is_none() || rec.encoding & UNWIND_MODE_MASK == E::UNWIND_MODE_DWARF)
        })
        .collect()
}

/// __LD,__compact_unwind, if any unwind record survives: one 32-byte
/// entry per record, aligned for its pointers.
fn compact_unwind_section<E: Target>(ctx: &Context<E>) -> Option<SyntheticSection> {
    let records = compact_unwind_records(ctx);
    if records.is_empty() {
        return None;
    }
    let size = 32 * records.len() as u64;
    let kind = SyntheticKind::CompactUnwind(records);
    Some(SyntheticSection::new(b"__LD", b"__compact_unwind", S_ATTR_DEBUG, 3, size, kind))
}

/// A record of __TEXT,__eh_frame: an input CIE or FDE.
#[derive(Clone, Copy)]
enum EhRec {
    Cie(usize),
    Fde(usize),
}

impl EhRec {
    /// The record's bytes as its object has them.
    fn data<E: Target>(self, ctx: &Context<E>) -> &'static [u8] {
        match self {
            EhRec::Cie(c) => ctx.cies[c].data,
            EhRec::Fde(f) => ctx.fdes[f].data,
        }
    }
}

/// __TEXT,__eh_frame's records and their offsets there: every input
/// FDE whose function survives (a coalesced-away weak copy's goes with
/// it), its CIE just before the first FDE that points at it (the
/// pointer is a backward offset). The loader kept the FDEs of
/// compactly-encoded functions for this.
fn eh_frame_records<E: Target>(ctx: &Context<E>) -> Vec<(EhRec, u32)> {
    let mut cies_used: HashSet<u32> = HashSet::new();
    let mut records = Vec::new();
    let mut off = 0u32;
    for (f, fde) in ctx.fdes.iter().enumerate() {
        let isec = &ctx.isecs[fde.isec as usize];
        if !isec.is_emitted() {
            continue;
        }
        let cie = cies_used.insert(fde.cie).then_some(EhRec::Cie(fde.cie as usize));
        for r in cie.into_iter().chain([EhRec::Fde(f)]) {
            records.push((r, off));
            off += r.data(ctx).len() as u32;
        }
    }
    records
}

/// __TEXT,__eh_frame, if any CIE or FDE survives.
fn eh_frame_section<E: Target>(ctx: &Context<E>) -> Option<SyntheticSection> {
    let records = eh_frame_records(ctx);
    if records.is_empty() {
        return None;
    }
    let size = records.iter().map(|&(r, _)| r.data(ctx).len() as u64).sum();
    Some(SyntheticSection::new(
        b"__TEXT",
        b"__eh_frame",
        S_COALESCED | S_ATTR_NO_TOC | S_ATTR_STRIP_STATIC_SYMS | S_ATTR_LIVE_SUPPORT,
        3,
        size,
        SyntheticKind::EhFrame(records),
    ))
}

/// __LD,__compact_unwind's contents, re-synthesized so unwind info
/// survives the merge: one 32-byte entry per record - the function, its
/// length and encoding, the personality and the LSDA - its pointer
/// fields set by 8-byte UNSIGNED relocations. Each entry is made on all
/// cores.
fn compact_unwind_contents<E: Target>(
    targets: &RelocTargets<E>,
    records: &[usize],
) -> (Vec<u8>, Vec<MachRel>) {
    let ctx = targets.ctx;
    let len = 3 << 25;
    let mut data = vec![0u8; 32 * records.len()];
    let relocs: Vec<MachRel> = data
        .par_chunks_mut(32)
        .zip(records)
        .enumerate()
        .flat_map_iter(|(i, (entry, &r))| {
            let rec = &ctx.unwind_records[r];
            let at = 32 * i as u32;
            let (func, bits) = targets.pointer_to(rec.isec as usize, rec.input_offset as u64, len);
            entry[..8].copy_from_slice(&func.to_le_bytes());
            entry[8..12].copy_from_slice(&rec.code_len.to_le_bytes());
            entry[12..16].copy_from_slice(&rec.encoding.to_le_bytes());
            let func = MachRel { offset: U32::new(at), bits: U32::new(bits) };
            let personality = rec.personality().map(|p| MachRel {
                offset: U32::new(at + 16),
                bits: U32::new(targets.personality(p) | len | (1 << 27)),
            });
            let lsda = rec.lsda().map(|(lsda, off)| {
                let (lsda, bits) = targets.pointer_to(ctx.isecs.resolve(lsda), off as u64, len);
                entry[24..].copy_from_slice(&lsda.to_le_bytes());
                MachRel { offset: U32::new(at + 24), bits: U32::new(bits) }
            });
            [Some(func), personality, lsda].into_iter().flatten()
        })
        .collect();
    (data, relocs)
}

/// __TEXT,__eh_frame's contents at address `addr`, in ld-prime's form:
/// the input CIEs and FDEs copied through with their self-relative
/// fields recomputed for the merged layout - the CIE pointer, pc_begin
/// and the LSDA pointer - and no symbols or relocations of their own
/// but the CIE's personality cell, a 4-byte pcrel GOT reference (the
/// shape compilers emit). ld64 classic named every CIE EH_Frame1 and
/// every FDE func.eh and wrote the fields as SUBTRACTOR pairs against
/// them; ld-prime does not.
fn eh_frame_contents<E: Target>(
    targets: &RelocTargets<E>,
    records: &[(EhRec, u32)],
    addr: u64,
) -> (Vec<u8>, Vec<MachRel>) {
    let ctx = targets.ctx;
    let cie_off: HashMap<usize, u32> = records
        .iter()
        .filter_map(|&(r, off)| match r {
            EhRec::Cie(c) => Some((c, off)),
            EhRec::Fde(_) => None,
        })
        .collect();
    let mut data: Vec<u8> = Vec::new();
    let mut relocs: Vec<MachRel> = Vec::new();
    for &(r, off) in records {
        debug_assert_eq!(off as usize, data.len());
        data.extend_from_slice(r.data(ctx));
        match r {
            EhRec::Cie(c) => {
                let cie = &ctx.cies[c];
                if let Some(p) = cie.personality {
                    relocs.push(MachRel {
                        offset: U32::new(off + cie.personality_offset),
                        bits: U32::new(
                            targets.personality(p)
                                | (1 << 24)
                                | (2 << 25)
                                | (1 << 27)
                                | ((E::RELOC_GOTPC as u32) << 28),
                        ),
                    });
                }
            }
            EhRec::Fde(f) => {
                let fde = &ctx.fdes[f];
                // The CIE pointer: how far back the CIE is from this
                // field.
                let cie_ptr = (off + 4).wrapping_sub(cie_off[&(fde.cie as usize)]);
                let fde_addr = addr + off as u64;
                crate::chunks::eh_frame::relocate_fde(
                    ctx,
                    fde,
                    &mut data[off as usize..],
                    fde_addr,
                    cie_ptr,
                );
            }
        }
    }
    (data, relocs)
}

/// A -r output section's relocations, regenerated against the merged
/// tables, each subsection's on a core of its own, in the order the
/// reader has them: by offset, a SUBTRACTOR just before the relocation
/// it pairs with (an arm64 ADDEND goes before its relocation too, see
/// push_reloc). Nothing else orders a section's relocations.
fn section_relocs<E: Target>(
    targets: &RelocTargets<E>,
    chunk_idx: OutputSectionId,
) -> Vec<MachRel> {
    let ctx = targets.ctx;
    ctx.output_section(chunk_idx)
        .members
        .par_iter()
        .flat_map_iter(|&id| {
            let isec = &ctx.isecs[id];
            let mut rels: Vec<MachRel> = Vec::new();
            for rel in isec.rels(&ctx.objs[isec.file as usize]) {
                push_reloc(targets, isec, rel, &mut rels);
            }
            rels
        })
        .collect()
}

/// Appends the -r relocation entries standing for one input relocation.
fn push_reloc<E: Target>(
    targets: &RelocTargets<E>,
    isec: &InputSection,
    rel: &Reloc,
    out: &mut Vec<MachRel>,
) {
    let ctx = targets.ctx;
    let offset = (isec.offset as u64 + rel.offset as u64) as u32;
    let (idx, is_extern) = match targets.out_target(isec, rel) {
        OutTarget::Sym(idx) => {
            // An explicit addend record precedes relocations whose
            // instruction can't hold one.
            if rel.addend != 0 && E::relocatable_needs_addend(rel.ty) {
                out.push(MachRel {
                    offset: U32::new(offset),
                    bits: U32::new(
                        (rel.addend as u32 & 0xff_ffff)
                            | (2 << 25)
                            | ((E::RELOC_ADDEND as u32) << 28),
                    ),
                });
            }
            (idx, true)
        }
        OutTarget::Section(target, _) => (ctx.isecs[target].sect_idx(ctx) as u32, false),
    };
    out.push(MachRel {
        offset: U32::new(offset),
        bits: U32::new(
            idx | ((rel.is_pcrel as u32) << 24)
                | (rel.size.trailing_zeros() << 25)
                | ((is_extern as u32) << 27)
                | ((rel.ty as u32) << 28),
        ),
    });
}

/// How a -r output refers to a relocation's target.
enum OutTarget {
    /// By the symbol at this index of its symbol table.
    Sym(u32),
    /// Section-relatively: a subsection and the offset in it. A
    /// section-relative input relocation stays so: a later link finds
    /// the subsection by the address, as this one did.
    Section(usize, i64),
}

/// How the -r output names the targets of its relocations and pointer
/// fields: by their symbols in its symbol table, or section-relatively.
struct RelocTargets<'a, E: Target> {
    ctx: &'a Context<E>,
    symtab: &'a RSymtab,
}

impl<'a, E: Target> RelocTargets<'a, E> {
    fn new(ctx: &'a Context<E>, symtab: &'a RSymtab) -> Self {
        Self { ctx, symtab }
    }

    /// How the output refers to a relocation's target.
    fn out_target(&self, isec: &InputSection, rel: &Reloc) -> OutTarget {
        let ctx = self.ctx;
        match rel.target() {
            RelocTarget::Sym(idx) => {
                let sym_id = ctx.objs[isec.file as usize].symbols[idx as usize];
                if let Some(i) = self.symtab.index_of(sym_id) {
                    return OutTarget::Sym(i);
                }
                let sym = &ctx.symbols[sym_id];
                let Some(t) = sym.input_section() else {
                    fatal!("-r: cannot re-emit relocation against {}", display(sym.name()));
                };
                OutTarget::Section(ctx.isecs.resolve(t as usize), sym.value as i64 + rel.addend)
            }
            RelocTarget::Section(t) => {
                OutTarget::Section(ctx.isecs.resolve(t as usize), rel.addend)
            }
        }
    }

    /// A pointer field to offset `off` of subsection `t`: its contents,
    /// the target's address, and its relocation's section and p2size
    /// (`len`) bits. It is section-relative, as clang writes the
    /// function and LSDA fields of __compact_unwind.
    fn pointer_to(&self, t: usize, off: u64, len: u32) -> (u64, u32) {
        let ctx = self.ctx;
        let isec = &ctx.isecs[t];
        (isec.addr(ctx) + off, isec.sect_idx(ctx) as u32 | len)
    }

    /// The symbol index of an unwind record's or a CIE's personality
    /// routine, which the output's symbol table names: its object lists
    /// it (see undefined_symbols).
    fn personality(&self, p: SymbolId) -> u32 {
        let Some(idx) = self.symtab.index_of(p) else {
            fatal!("-r: unwind personality lost: {}", self.ctx.symbols[p]);
        };
        idx
    }
}

/// Where the parts of a -r output lie in the file, besides the sections'
/// contents: the segment, which the contents make up, and what follows
/// it - each section's relocations, in output order, then data in code,
/// hints, symbols and strings - and the file's size.
struct FileLayout {
    vmsize: u64,
    seg_fileoff: u64,
    seg_filesize: u64,
    reloffs: Vec<u64>,
    diceoff: u64,
    lohoff: u64,
    symoff: u64,
    stroff: u64,
    size: u64,
}

impl FileLayout {
    /// Places what follows the sections' contents, which end at
    /// `content_end`, given each section's relocations, `relocs`.
    fn new(
        cmds: &LoadCommands,
        symtab: &RSymtab,
        relocs: &[&[MachRel]],
        vmsize: u64,
        seg_fileoff: u64,
        content_end: u64,
    ) -> Self {
        let mut off = align_to(content_end, 8);
        let mut place = |size: usize| {
            off += size as u64;
            off - size as u64
        };
        let reloffs = relocs.iter().map(|rels| place(size_of_val(*rels))).collect();
        let diceoff = place(cmds.dice.len() * 8);
        let lohoff = place(cmds.loh.as_ref().map_or(0, Vec::len));
        let symoff = place(symtab.table.len() * size_of::<MachSym>());
        let stroff = place(symtab.table.strtab_size);
        Self {
            vmsize,
            seg_fileoff,
            seg_filesize: content_end - seg_fileoff,
            reloffs,
            diceoff,
            lohoff,
            symoff,
            stroff,
            size: off,
        }
    }
}

/// A section's header in the segment command.
fn section_header(hdr: &ChunkHeader, relocs: &[MachRel], reloff: u64) -> MachSection {
    MachSection {
        sectname: bytes_to_name(hdr.sectname),
        segname: bytes_to_name(hdr.segname),
        addr: U64::new(hdr.addr),
        size: U64::new(hdr.size),
        offset: U32::new(hdr.fileoff as u32),
        p2align: U32::new(hdr.p2align),
        reloff: U32::new(if relocs.is_empty() { 0 } else { reloff as u32 }),
        nreloc: U32::new(relocs.len() as u32),
        flags: U32::new(hdr.flags),
        reserved1: U32::new(0),
        reserved2: U32::new(0),
        reserved3: U32::new(0),
    }
}

/// Writes the Mach header and the load commands.
fn write_load_commands<E: Target>(
    ctx: &Context<E>,
    buf: &mut [u8],
    cmds: &LoadCommands,
    headers: &[MachSection],
    layout: &FileLayout,
    symtab: &RSymtab,
) {
    // Subsections only if every input had them: one whole-section
    // object makes the output whole-section too (ld64).
    let subsections = ctx
        .objs
        .iter()
        .enumerate()
        .filter(|(i, o)| o.is_reachable && !ctx.is_internal(*i))
        .all(|(_, o)| o.subsections_via_symbols);
    let hdr = MachHeader {
        magic: U32::new(MH_MAGIC_64),
        cputype: U32::new(E::CPUTYPE),
        cpusubtype: U32::new(E::CPUSUBTYPE),
        filetype: U32::new(MH_OBJECT),
        ncmds: U32::new(cmds.count()),
        sizeofcmds: U32::new(cmds.size() as u32),
        flags: U32::new(if subsections { MH_SUBSECTIONS_VIA_SYMBOLS } else { 0 }),
        reserved: U32::new(0),
    };
    hdr.write(buf);
    let mut p = size_of::<MachHeader>();

    let seg = SegmentCommand {
        cmd: U32::new(LC_SEGMENT_64),
        cmdsize: U32::new((size_of::<SegmentCommand>() + size_of_val(headers)) as u32),
        segname: [0; 16],
        vmaddr: U64::new(0),
        vmsize: U64::new(layout.vmsize),
        fileoff: U64::new(layout.seg_fileoff),
        filesize: U64::new(layout.seg_filesize),
        maxprot: U32::new(VM_PROT_READ | VM_PROT_WRITE | VM_PROT_EXECUTE),
        initprot: U32::new(VM_PROT_READ | VM_PROT_WRITE | VM_PROT_EXECUTE),
        nsects: U32::new(headers.len() as u32),
        flags: U32::new(0),
    };
    seg.write(&mut buf[p..]);
    p += size_of::<SegmentCommand>();
    MachSection::write_all(headers, &mut buf[p..]);
    p += size_of_val(headers);

    let st = SymtabCommand {
        cmd: U32::new(LC_SYMTAB),
        cmdsize: U32::new(size_of::<SymtabCommand>() as u32),
        symoff: U32::new(layout.symoff as u32),
        nsyms: U32::new(symtab.table.len() as u32),
        stroff: U32::new(layout.stroff as u32),
        strsize: U32::new(symtab.table.strtab_size as u32),
    };
    st.write(&mut buf[p..]);
    p += size_of::<SymtabCommand>();

    buf[p..p + cmds.version.len()].copy_from_slice(&cmds.version);
    p += cmds.version.len();

    let dc = LinkEditDataCommand {
        cmd: U32::new(LC_DATA_IN_CODE),
        cmdsize: U32::new(size_of::<LinkEditDataCommand>() as u32),
        dataoff: U32::new(layout.diceoff as u32),
        datasize: U32::new((cmds.dice.len() * 8) as u32),
    };
    dc.write(&mut buf[p..]);
    p += size_of::<LinkEditDataCommand>();

    for cmd in &cmds.linker_options {
        buf[p..p + cmd.len()].copy_from_slice(cmd);
        p += cmd.len();
    }

    if let Some(loh) = &cmds.loh {
        let cmd = LinkEditDataCommand {
            cmd: U32::new(LC_LINKER_OPTIMIZATION_HINT),
            cmdsize: U32::new(size_of::<LinkEditDataCommand>() as u32),
            dataoff: U32::new(layout.lohoff as u32),
            datasize: U32::new(loh.len() as u32),
        };
        cmd.write(&mut buf[p..]);
    }
}

/// Copies the merged sections' contents to the output, each subsection
/// on a core of its own (their ranges are disjoint): raw copies, with
/// non-external targets' embedded addresses rewritten into the merged
/// address space.
fn copy_section_contents<E: Target>(
    targets: &RelocTargets<E>,
    merged: &[OutputSectionId],
    buf: &mut [u8],
) {
    let ctx = targets.ctx;
    // Each subsection with contents, and its section's header.
    let mut jobs: Vec<(&ChunkHeader, &InputSection)> = Vec::new();
    for &osec in merged {
        let osec = ctx.output_section(osec);
        if !osec.hdr.is_zerofill() {
            let isecs = osec.members.iter().map(|&id| &ctx.isecs[id]);
            jobs.extend(
                isecs.filter(|isec| !isec.contents().is_empty()).map(|isec| (&osec.hdr, isec)),
            );
        }
    }
    let ranges: Vec<std::ops::Range<u64>> = jobs
        .iter()
        .map(|&(hdr, isec)| {
            let start = hdr.fileoff + isec.offset as u64;
            start..start + isec.contents().len() as u64
        })
        .collect();
    let slices = output_file::split_ranges(buf, &ranges);
    jobs.into_par_iter().zip(slices).for_each(|((hdr, isec), out)| {
        out.copy_from_slice(isec.contents());
        for rel in isec.rels(&ctx.objs[isec.file as usize]) {
            let here = hdr.addr + isec.offset as u64 + rel.offset as u64;
            rewrite_field(targets, isec, rel, here, &mut out[rel.offset as usize..]);
        }
    });
}

/// Rewrites the field a relocation at address `here` applies to, as a
/// -r output holds it: the address a non-external relocation's field
/// embeds, now in the merged address space. Every other field keeps
/// the object's bytes.
fn rewrite_field<E: Target>(
    targets: &RelocTargets<E>,
    isec: &InputSection,
    rel: &Reloc,
    here: u64,
    field: &mut [u8],
) {
    let ctx = targets.ctx;
    let OutTarget::Section(target, addend) = targets.out_target(isec, rel) else {
        return;
    };
    // The addend is negative for a target before its section's start.
    let target_addr = ctx.isecs[target].addr(ctx).wrapping_add_signed(addend);
    if rel.ty == E::RELOC_UNSIGNED && !rel.is_pcrel {
        match rel.size {
            8 => field[..8].copy_from_slice(&target_addr.to_le_bytes()),
            4 => field[..4].copy_from_slice(&(target_addr as u32).to_le_bytes()),
            _ => {}
        }
    } else if rel.is_pcrel {
        // Pcrel non-external fields embed target - (P + 4).
        let val =
            target_addr.wrapping_sub(here + 4).wrapping_sub(E::reloc_bias(rel.ty) as u64) as u32;
        if rel.size == 4 {
            field[..4].copy_from_slice(&val.to_le_bytes());
        }
    } else {
        error!("-r: unsupported non-external relocation");
    }
}

/// A -r output's symbol and string tables.
struct RSymtab {
    /// The tables, laid out as a final image's are and written by the
    /// same writer (see copy_buf), but with every entry's value
    /// set already.
    table: SymtabSection,
    /// Each symbol's index in the table, or u32::MAX if it has none.
    index_of_sym: Vec<u32>,
}

impl RSymtab {
    /// A symbol's index in the table, if it has an entry.
    fn index_of(&self, id: SymbolId) -> Option<u32> {
        let index = self.index_of_sym[id as usize];
        (index != u32::MAX).then_some(index)
    }
}

/// A symbol's address in the -r output.
pub(crate) fn sym_addr<E: Target>(ctx: &Context<E>, id: SymbolId) -> u64 {
    let sym = &ctx.symbols[id];
    match sym.input_section() {
        Some(isec) => ctx.isecs[isec as usize].addr(ctx) + sym.value,
        None => sym.value,
    }
}

/// Builds a -r output's symbol table: the local symbols (see
/// local_symbols), then the -add_ast_path entries, as in a final image,
/// then the stabs, then the defined externals and the undefined
/// symbols. The strings are laid out as a final image's (see
/// layout_strings).
fn create_output_symtab<E: Target>(ctx: &Context<E>) -> RSymtab {
    let t = ctx.timer("r-symtab-locals");
    let locals = local_symbols(ctx);
    drop(t);

    // Debug-note stabs: ld64 does not merge the inputs' DWARF into a -r
    // output, it names the objects that hold it (N_OSO) and where their
    // symbols landed, and a later link carries the notes through.
    // Under -x, which strip -x passes, ld-prime writes none.
    let t = ctx.timer("r-symtab-stabs");
    let stabs = match ctx.args.strip_locals {
        true => Vec::new(),
        false => crate::chunks::symtab::plan_stabs(ctx),
    };
    let nstabs: usize = stabs.iter().map(|plan| plan.len()).sum();
    drop(t);

    // The defined externals, then the undefined and tentative symbols.
    let t = ctx.timer("r-symtab-externals");
    let mut externals = defined_externals(ctx);
    externals.extend(undefined_symbols(ctx));
    drop(t);

    // The entries, each made on all cores straight into its slot, and
    // their strings.
    let t = ctx.timer("r-symtab-strings");
    let mut table = SymtabSection::new();
    let nasts = crate::chunks::symtab::ast_paths(ctx).len();
    let total = locals.len() + nasts + externals.len();
    let mut names: Vec<&'static [u8]> = Vec::with_capacity(total);
    table.entries.reserve_exact(total);
    par_push_entries(&mut names, &mut table.entries, &locals, |l| (l.name, l.msym, None));
    crate::chunks::symtab::push_ast_paths(ctx, &mut names, &mut table.entries);
    let stabs_start = table.entries.len();
    par_push_entries(&mut names, &mut table.entries, &externals, |&(ent, id)| {
        (ctx.symbols[id].name(), ent, None)
    });
    let strtab_end = crate::chunks::symtab::layout_strings(&mut table.entries, &names);
    table.names = names;

    // Each symbol's index in the table, for the relocations, and its
    // string, which the notes naming it share - but for a local whose
    // name is hidden, whose notes keep its own name.
    let nsyms = ctx.symbols.syms.len();
    let index_of_sym: Vec<AtomicU32> =
        (0..nsyms).into_par_iter().map(|_| AtomicU32::new(u32::MAX)).collect();
    let strx_of: Vec<AtomicU32> =
        (0..nsyms).into_par_iter().map(|_| AtomicU32::new(u32::MAX)).collect();
    let entries = &table.entries;
    locals.par_iter().enumerate().for_each(|(i, l)| {
        if let Some(id) = l.sym {
            index_of_sym[id as usize].store(i as u32, Ordering::Relaxed);
            if !l.hidden {
                strx_of[id as usize].store(entries[i].0.stroff.get(), Ordering::Relaxed);
            }
        }
    });
    externals.par_iter().enumerate().for_each(|(k, &(_, id))| {
        let i = stabs_start + k;
        index_of_sym[id as usize].store((i + nstabs) as u32, Ordering::Relaxed);
        strx_of[id as usize].store(entries[i].0.stroff.get(), Ordering::Relaxed);
    });
    table.strx_of = strx_of.into_iter().map(AtomicU32::into_inner).collect();
    table.set_stabs(ctx, stabs, stabs_start, strtab_end);
    drop(t);

    RSymtab { table, index_of_sym: index_of_sym.into_iter().map(AtomicU32::into_inner).collect() }
}

/// The symbols `pred` takes, in symbol order, found on all cores. The
/// order is the inputs': nothing reads an object's symbol order (a -r
/// output has no LC_DYSYMTAB to sort externals by name for).
fn symbols_where<E: Target>(ctx: &Context<E>, pred: impl Fn(usize) -> bool + Sync) -> Vec<usize> {
    (0..ctx.symbols.syms.len()).into_par_iter().filter(|&i| pred(i)).collect()
}

/// A -r output's defined externals, with their entries.
fn defined_externals<E: Target>(ctx: &Context<E>) -> Vec<(MachSym, SymbolId)> {
    // The desc flags a defined global carries in its object, which the
    // next link needs as much as this one did. N_ALT_ENTRY is the
    // critical one: it marks a symbol that does not begin a new
    // subsection (Swift's class metadata symbol $s..CN is an alt entry
    // into the full-metadata object $s..CMf, referenced as CMf+0x18),
    // and a link that splits there re-aligns the tail and moves the
    // symbol away from every non-symbolic reference to it.
    let nsyms = ctx.symbols.syms.len();
    let desc_of: Vec<AtomicU16> = (0..nsyms).into_par_iter().map(|_| AtomicU16::new(0)).collect();
    ctx.objs.par_iter().enumerate().filter(|(_, obj)| obj.is_reachable).for_each(|(obj_idx, obj)| {
        let r = obj.global_range();
        for (msym, &sym_id) in obj.mach_syms[r.clone()].iter().zip(&obj.symbols[r]) {
            if !msym.is_stab()
                && msym.is_extern()
                && msym.ty() != N_UNDF
                && matches!(ctx.symbols[sym_id].file(), Some(FileId::Obj(o)) if o as usize == obj_idx)
            {
                desc_of[sym_id as usize].store(msym.desc.get(), Ordering::Relaxed);
            }
        }
    });

    let keep_pext = ctx.args.keep_private_externs;
    let globals = symbols_where(ctx, |i| {
        let sym = &ctx.symbols[i];
        sym.is_extern()
            && (keep_pext || !sym.is_private_extern())
            && matches!(sym.file(), Some(FileId::Obj(_)))
            && sym
                .input_section()
                .is_none_or(|isec| ctx.isecs[ctx.isecs.resolve(isec as usize)].is_alive())
    });
    globals
        .par_iter()
        .map(|&i| {
            let sym = &ctx.symbols[i];
            let pext = if sym.is_private_extern() { N_PEXT } else { 0 };
            // An absolute symbol names no subsection, and ld-prime gives
            // it no desc flags (the assembler marks one N_NO_DEAD_STRIP).
            let Some(input) = sym.input_section() else {
                let ent = MachSym {
                    n_type: N_ABS | N_EXT | pext,
                    value: U64::new(sym.value),
                    ..MachSym::default()
                };
                return (ent, i as u32);
            };
            let n_type = N_SECT | N_EXT | pext;
            let sect = ctx.isecs[ctx.isecs.resolve(input as usize)].sect_idx(ctx);
            // N_WEAK_REF on a definition is .weak_def_can_be_hidden: with
            // N_WEAK_DEF it lets a final link auto-hide the symbol, which
            // the -r output must leave it free to do.
            let mut desc = desc_of[i].load(Ordering::Relaxed)
                & (N_WEAK_DEF
                    | N_WEAK_REF
                    | N_ALT_ENTRY
                    | N_NO_DEAD_STRIP
                    | N_SYMBOL_RESOLVER
                    | N_COLD_FUNC
                    | REFERENCED_DYNAMICALLY);
            if sym.is_weak_def() {
                desc |= N_WEAK_DEF;
            }
            desc |= section_desc(ctx, input as usize);
            let value = sym_addr(ctx, i as u32);
            (
                MachSym {
                    stroff: U32::new(0),
                    n_type,
                    sect,
                    desc: U16::new(desc),
                    value: U64::new(value),
                },
                i as u32,
            )
        })
        .collect()
}

/// A -r output's undefined and tentative symbols, with their entries:
/// every one a live object lists or -u names, which is what a link of
/// the objects would see. (ld-prime keeps only those a relocation, an
/// unwind personality or -u names, and DTrace's; a final link ignores
/// the others.) A tentative definition that is a private external stays
/// one (N_PEXT), -keep_private_externs or not: a -r link allocates no
/// commons, and so has none to demote.
fn undefined_symbols<E: Target>(ctx: &Context<E>) -> Vec<(MachSym, SymbolId)> {
    let undefs = symbols_where(ctx, |i| {
        let sym = &ctx.symbols[i];
        sym.is_used() && (sym.is_common() || !sym.is_defined())
    });
    undefs
        .par_iter()
        .map(|&i| {
            let sym = &ctx.symbols[i];
            let mut n_type = N_UNDF | N_EXT;
            let mut desc = 0;
            let mut value = 0;
            if sym.is_common() {
                value = sym.value;
                desc |= (sym.common_p2align as u16) << 8;
                if sym.is_private_extern() {
                    n_type |= N_PEXT;
                }
            } else if sym.is_weak_ref() {
                desc |= N_WEAK_REF;
            }
            (
                MachSym {
                    stroff: U32::new(0),
                    n_type,
                    sect: 0,
                    desc: U16::new(desc),
                    value: U64::new(value),
                },
                i as u32,
            )
        })
        .collect()
}

/// The local symbols of a -r output: each object's (see
/// ObjectFile::populate_symtab), in the objects' order, then those
/// naming the -sectcreate inputs. Under -x, or where
/// -non_global_symbols_strip_list or -non_global_symbols_no_strip_list
/// strips a name, the symbol stays, as a relocation may name it, under
/// a name made up for it, l<n> numbered in the table's order; the notes
/// of its unit keep its own.
fn local_symbols<E: Target>(ctx: &Context<E>) -> Vec<LocalSymbol> {
    let per_obj: Vec<Vec<LocalSymbol>> =
        ctx.objs.par_iter().enumerate().map(|(i, obj)| obj.populate_symtab(ctx, i)).collect();
    let mut locals = per_obj.concat();
    locals.extend(sectcreate_locals(ctx));
    let mut counter = 0;
    for l in locals.iter_mut().filter(|l| l.hidden) {
        counter += 1;
        l.name = format!("l{counter:03}").leak().as_bytes();
    }
    locals
}
