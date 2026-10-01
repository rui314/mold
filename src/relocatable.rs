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
//! address space. DWARF is not merged (its section-relative offsets
//! carry no relocations); like ld64, the output gets debug-note stabs
//! naming the input objects, which a later link carries through.

use hashbrown::{HashMap, HashSet};
use rayon::prelude::*;
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, Ordering};

use crate::chunks::symtab::{SymtabSection, par_push_entries};
use crate::chunks::{ChunkHeader, ChunkId, OutputSectionId};
use crate::context::Context;
use crate::error;
use crate::fatal;
use crate::input_files::FileId;
use crate::input_sections::{InputSection, NO_REPLACEMENT, Reloc, RelocTarget};
use crate::macho::*;
use crate::output_file;
use crate::symbol::SymbolId;
use crate::target::Target;
use crate::util::{align_to, encode_uleb, name_sort_key};

/// ld64's section order in a -r output, measured with ld-prime 27037:
/// __TEXT first, __LD last, and the other segments - __DATA and
/// __DATA_CONST included - in the order their first section appears.
/// In __TEXT the code sections come first, __text among them, in
/// first-seen order, then __StaticInit, then everything else as first
/// seen, and __gcc_except_tab and __eh_frame close the segment. __DATA
/// has fixed ranks for the sections ld64 knows (__const, the
/// Objective-C sections, the initializer lists, __data), unknown ones
/// in first-seen order after them. The thread-local template and the
/// zero-fill sections close every segment but __TEXT: an object's
/// file image mirrors its address space. The first element ranks the
/// segment.
fn section_rank(segname: &str, sectname: &str, flags: u32) -> (u32, u32) {
    let seg = match segname {
        "__TEXT" => 0,
        "__LD" => 2,
        _ => 1,
    };
    let sect = match (segname, sectname) {
        ("__TEXT", "__StaticInit") => 1,
        // After __const and __cstring, whatever the input order.
        ("__TEXT", "__gcc_except_tab") => 3,
        ("__TEXT", "__eh_frame") => 4,
        ("__TEXT", _) if flags & S_ATTR_PURE_INSTRUCTIONS != 0 => 0,
        ("__TEXT", _) => 2,
        ("__DATA", "__got") => 0,
        ("__DATA", "__const") => 1,
        ("__DATA", "__cfstring") => 2,
        ("__DATA", "__objc_classlist") => 3,
        ("__DATA", "__objc_nlclslist") => 4,
        ("__DATA", "__objc_catlist") => 5,
        ("__DATA", "__objc_catlist2") => 6,
        ("__DATA", "__objc_nlcatlist") => 7,
        ("__DATA", "__objc_protolist") => 8,
        ("__DATA", "__objc_imageinfo") => 9,
        ("__DATA", "__objc_const") => 10,
        ("__DATA", "__objc_selrefs") => 11,
        ("__DATA", "__objc_protorefs") => 12,
        ("__DATA", "__objc_classrefs") => 13,
        ("__DATA", "__objc_superrefs") => 14,
        ("__DATA", "__objc_ivar") => 15,
        ("__DATA", "__objc_data") => 16,
        ("__DATA", "__mod_init_func") => 17,
        ("__DATA", "__mod_term_func") => 18,
        ("__DATA", "__data") => 19,
        _ => match flags & SECTION_TYPE {
            S_THREAD_LOCAL_REGULAR => 21,
            S_THREAD_LOCAL_ZEROFILL => 22,
            S_ZEROFILL => 23,
            _ => 20,
        },
    };
    (seg, sect)
}

/// N_NO_DEAD_STRIP for a symbol from this input section: ld-prime
/// marks every symbol of a no_dead_strip section, local or global, so
/// the next link keeps it even when the output section takes another
/// member's attributes - but none of __objc_classrefs - and those of
/// the initializer and terminator pointer lists, which dead stripping
/// keeps whatever their attributes.
fn section_desc<E: Target>(ctx: &Context<E>, isec: usize) -> u16 {
    let h = ctx.hdr_of(&ctx.isecs[isec]);
    let roots = matches!(h.section_type(), S_MOD_INIT_FUNC_POINTERS | S_MOD_TERM_FUNC_POINTERS);
    if roots
        || h.flags & S_ATTR_NO_DEAD_STRIP != 0
            && !(h.segname() == "__DATA" && h.sectname() == "__objc_classrefs")
    {
        N_NO_DEAD_STRIP
    } else {
        0
    }
}

/// Whether a symbol lies in an initializer or terminator pointer list
/// whose atoms dead stripping keeps by their section type alone: there
/// ld-prime marks only the name of each atom no-dead-strip, not its
/// aliases - unless the section says no_dead_strip itself.
fn in_init_term_list<E: Target>(ctx: &Context<E>, sym: SymbolId) -> bool {
    let Some(isec) = ctx.symbols[sym].input_section() else { return false };
    let h = ctx.hdr_of(&ctx.isecs[isec as usize]);
    matches!(h.section_type(), S_MOD_INIT_FUNC_POINTERS | S_MOD_TERM_FUNC_POINTERS)
        && h.flags & S_ATTR_NO_DEAD_STRIP == 0
}

/// Where the -r output's defined externals sit, as (object, section,
/// address): an external names its atom over any local there (a weak
/// one only in an object with subsections).
fn external_places<E: Target>(ctx: &Context<E>) -> HashSet<(u32, u8, u64)> {
    let per_obj: Vec<Vec<(u32, u8, u64)>> = ctx
        .objs
        .par_iter()
        .enumerate()
        .map(|(obj_idx, obj)| {
            if !obj.is_alive {
                return Vec::new();
            }
            let r = obj.global_range();
            obj.nlists[r.clone()]
                .iter()
                .zip(&obj.symbols[r])
                .filter(|&(nlist, &sym_id)| {
                    let sym = &ctx.symbols[sym_id];
                    !nlist.is_stab()
                        && nlist.is_extern()
                        && nlist.n_type() == N_SECT
                        && (ctx.args.keep_private_externs || !sym.is_private_extern())
                        && (obj.subsections_via_symbols || !sym.is_weak_def())
                        && matches!(sym.file(), Some(FileId::Obj(o)) if o as usize == obj_idx)
                })
                .map(|(nlist, _)| (obj_idx as u32, nlist.n_sect, nlist.n_value))
                .collect()
        })
        .collect();
    per_obj.into_iter().flatten().collect()
}

/// The places the -r output's symbols name, by subsection and offset:
/// the symbol indices of the first and the last of their names, and
/// their address. ld-prime ranks the names of a place non-weak before
/// weak, then global, private external and local, each by descending
/// name, an assembler's ltmpN label last. A reference to the place
/// names the first, and so does one past it, into the atom's bytes:
/// the names are aliases of one atom. But in an object without
/// subsections only those at the start of a section are; elsewhere
/// each name is an atom of its own, all empty but the last, which
/// holds the bytes and is the one a reference into them names.
fn symbol_places<E: Target>(ctx: &Context<E>, index_of_sym: &[u32]) -> Places {
    let mut names: Vec<_> = (0..index_of_sym.len())
        .into_par_iter()
        .filter(|&id| index_of_sym[id] != u32::MAX)
        .filter_map(|id| {
            let symnum = index_of_sym[id];
            let sym = &ctx.symbols[id];
            let isec = ctx.resolve_isec(sym.input_section()? as usize);
            let scope = match (sym.is_extern(), sym.is_private_extern()) {
                (true, false) => 0,
                (true, true) => 1,
                (false, _) => 2,
            };
            let is_ltmp = !sym.is_extern() && sym.name().starts_with("ltmp");
            let rank = (is_ltmp, sym.is_weak_def(), scope, Reverse(sym.name()));
            Some(((isec, sym.value), rank, symnum))
        })
        .collect();
    names.par_sort_unstable();
    let mut starts = vec![0u32; ctx.isecs.len() + 1];
    let places = names
        .chunk_by(|a, b| a.0 == b.0)
        .map(|names| {
            let (isec, at) = names[0].0;
            starts[isec + 1] += 1;
            (at, names[0].2, names[names.len() - 1].2, ctx.isec_addr(isec) + at)
        })
        .collect();
    for i in 1..starts.len() {
        starts[i] += starts[i - 1];
    }
    Places { places, starts }
}

/// The places a -r output's symbols name (see symbol_places), by
/// subsection.
struct Places {
    /// Each place's offset in its subsection, the symbol indices of the
    /// first and the last of its names, and its address, by subsection
    /// and offset.
    places: Vec<(u64, u32, u32, u64)>,
    /// Where each subsection's places start in `places`, and where the
    /// last one's end.
    starts: Vec<u32>,
}

impl Places {
    /// The nearest place at or before offset `off` of subsection `t`.
    fn at_or_before(&self, t: usize, off: u64) -> Option<(u64, u32, u32, u64)> {
        let places = &self.places[self.starts[t] as usize..self.starts[t + 1] as usize];
        places[..places.partition_point(|p| p.0 <= off)].last().copied()
    }
}

/// Which symbols the relocations of the live input sections that `pred`
/// takes refer to by name, by symbol.
fn reloc_syms<E: Target>(ctx: &Context<E>, pred: impl Fn(&Reloc) -> bool + Sync) -> Vec<bool> {
    let syms: Vec<AtomicBool> =
        (0..ctx.symbols.syms.len()).into_par_iter().map(|_| AtomicBool::new(false)).collect();
    ctx.isecs
        .par_iter()
        .filter(|isec| isec.is_alive() && !ctx.is_internal(isec.file as usize))
        .for_each(|isec| {
            for rel in crate::input_files::isec_relocs_of(&ctx.objs, isec) {
                if let RelocTarget::Sym(idx) = rel.target()
                    && pred(rel)
                {
                    let id = ctx.objs[isec.file as usize].symbols[idx as usize];
                    syms[id as usize].store(true, Ordering::Relaxed);
                }
            }
        });
    syms.into_iter().map(AtomicBool::into_inner).collect()
}

/// The symbols the output's relocations refer to by name, by symbol:
/// those a live input section's refer to, and the personality routines
/// of the unwind records (__compact_unwind) and CIEs (__eh_frame).
fn referenced_syms<E: Target>(ctx: &Context<E>) -> Vec<bool> {
    let mut syms = reloc_syms(ctx, |_| true);
    let unwind = ctx.unwind_records.iter().filter_map(|rec| rec.personality());
    for p in unwind.chain(ctx.cies.iter().filter_map(|cie| cie.personality)) {
        syms[p as usize] = true;
    }
    syms
}

/// The symbols the terms of a subtraction (a SUBTRACTOR and the
/// relocation it pairs with) refer to, by symbol.
fn subtracted_syms<E: Target>(ctx: &Context<E>) -> Vec<bool> {
    reloc_syms(ctx, |rel| rel.r_type == E::RELOC_SUBTRACTOR || rel.is_subtracted)
}

/// The places an object names with a symbol other than an assembler
/// temporary (ltmpN), where an ltmpN label is a mere alias.
fn named_places<E: Target>(
    ctx: &Context<E>,
    obj: &crate::input_files::ObjectFile,
) -> HashSet<(u8, u64)> {
    obj.nlists
        .iter()
        .zip(&obj.symbols)
        .filter(|&(nlist, &sym_id)| {
            !nlist.is_stab()
                && nlist.n_type() == N_SECT
                && !ctx.symbols[sym_id].name().starts_with("ltmp")
        })
        .map(|(nlist, _)| (nlist.n_sect, nlist.n_value))
        .collect()
}

/// The n_desc of a symbol from an object without subsections: ld64
/// marks the whole-section atoms no-dead-strip and drops the alt-entry
/// marker, which means nothing there.
fn whole_desc(desc: u16, whole: bool) -> u16 {
    if whole { (desc | N_NO_DEAD_STRIP) & !N_ALT_ENTRY } else { desc }
}

/// The payload of the output's LC_LINKER_OPTIMIZATION_HINT, or None
/// for no command. ld64 carries the arm64 hints through -r for the
/// final link to apply: each hint it takes (see
/// ObjectFile::hint_subsec) moves with its subsection, and goes with a
/// coalesced-away weak copy. The command appears if any input had such
/// a hint, even if none survives. As in ld64's output, the hints are
/// written subsection by subsection in address order, each
/// subsection's in input order: ULEB128 kind, count and addresses,
/// zero-padded to 8 bytes. (ld-prime 27037 drops them all.)
fn optimization_hints<E: Target>(ctx: &Context<E>) -> Option<Vec<u8>> {
    let mut found = false;
    // The subsection's new and input addresses, and the hint.
    let mut hints = Vec::new();
    for obj in ctx.objs.iter().filter(|o| o.is_alive) {
        for hint in &obj.loh {
            let Some(id) = obj.hint_subsec(&ctx.isecs, &hint.1) else {
                continue;
            };
            found = true;
            let isec = &ctx.isecs[id];
            if isec.is_alive() && isec.replacement == NO_REPLACEMENT {
                hints.push((ctx.isec_addr(id), isec.input_addr as u64, hint));
            }
        }
    }
    if !found {
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
/// final link to act on, as ld-prime rewrites those of its inputs (read
/// by passes::read_linker_options): one per library or framework, the
/// libraries first, each kind sorted by name. A framework goes by its
/// name less any ",suffix", and a library a -force_load, -needed_library
/// or -lazy_library names goes by its path, as -l<path>. The first
/// naming says whether the library loads lazily, and any whether it is
/// needed; -hidden-l and -force_load say nothing of it.
fn relocatable_linker_options<E: Target>(ctx: &Context<E>) -> Vec<Vec<Vec<u8>>> {
    if ctx.args.ignore_auto_link {
        return Vec::new();
    }
    // (framework, name) -> (lazy, needed)
    let mut libs: BTreeMap<(bool, &[u8]), (bool, bool)> = BTreeMap::new();
    let objs = ctx.objs.iter().filter(|obj| obj.is_alive).flat_map(|obj| &obj.linker_options);
    for opt in ctx.cmdline_linker_options.iter().flatten().chain(objs) {
        let (framework, name, kind) = match &opt[..] {
            [flag, name] if flag.ends_with(b"framework") => {
                let base = name.split(|&c| c == b',').next().unwrap();
                (true, base, flag.strip_suffix(b"framework").unwrap())
            }
            [flag, path] => (false, &path[..], flag.strip_suffix(b"library").unwrap_or(b"")),
            [lib] => {
                let (kind, name) = [&b"-needed-l"[..], b"-lazy-l", b"-hidden-l", b"-l"]
                    .into_iter()
                    .find_map(|kind| Some((kind, lib.strip_prefix(kind)?)))
                    .unwrap();
                (false, name, kind)
            }
            _ => unreachable!(),
        };
        let (lazy, needed) = (kind.starts_with(b"-lazy"), kind.starts_with(b"-needed"));
        libs.entry((framework, name))
            .and_modify(|(_, all_needed)| *all_needed |= needed)
            .or_insert((lazy, needed));
    }
    libs.into_iter()
        .map(|((framework, name), (lazy, needed))| {
            let kind = if lazy {
                "lazy"
            } else if needed {
                "needed"
            } else {
                ""
            };
            match (framework, kind) {
                (true, "") => vec![b"-framework".to_vec(), name.to_vec()],
                (true, _) => vec![format!("-{kind}_framework").into_bytes(), name.to_vec()],
                (false, "") => vec![[b"-l", name].concat()],
                (false, _) => vec![[format!("-{kind}-l").as_bytes(), name].concat()],
            }
        })
        .collect()
}

/// The __compact_unwind pointer fields an input set by a 4-byte
/// relocation, as a bit per field (1 << offset / 8) by record
/// (subsection and function offset): each keeps a 4-byte relocation,
/// as in ld-prime's output, its value fitting (the addresses of a -r
/// output start at zero).
fn narrow_unwind_fields<E: Target>(ctx: &Context<E>) -> HashMap<(u32, u32), u8> {
    let mut fields: HashMap<(u32, u32), u8> = HashMap::new();
    for obj in &ctx.objs {
        for &(isec, off, bit) in &obj.unwind_ptr32 {
            *fields.entry((isec, off)).or_default() |= bit;
        }
    }
    fields
}

/// Writes the -r output, returning its size.
pub fn link<E: Target>(ctx: &mut Context<E>) -> u64 {
    // The sections the output synthesizes: the merged __objc_imageinfo,
    // the re-synthesized __LD,__compact_unwind and __TEXT,__eh_frame.
    let t = ctx.timer("r-layout");
    let mut synthetic: Vec<SyntheticSection> =
        [objc_imageinfo_section(ctx), compact_unwind_section(ctx), eh_frame_section(ctx)]
            .into_iter()
            .flatten()
            .collect();

    // Every section in ld64's order, laid out from address zero, and
    // placed in the file after the load commands.
    let sects = sort_sections(ctx, &synthetic);
    let vmsize = assign_addresses(ctx, &mut synthetic, &sects);
    let cmds = LoadCommands::new(ctx, sects.len());
    let seg_fileoff = cmds.contents_offset(&ctx.args);
    let content_end = assign_file_offsets(ctx, &mut synthetic, &sects, seg_fileoff);
    drop(t);

    // The symbol table, then what refers to its symbols: the synthetic
    // sections' contents and the relocations, regenerated against the
    // merged tables.
    let ctx = &*ctx;
    let merged: Vec<OutputSectionId> = sects
        .iter()
        .filter_map(|s| match *s {
            Sect::Merged(i) => Some(i),
            Sect::Synthetic(_) => None,
        })
        .collect();
    let t = ctx.timer("r-symtab");
    let symtab = build_symtab(ctx, &merged);
    drop(t);
    let t = ctx.timer("r-relocs");
    let targets = RelocTargets::new(ctx, &symtab);
    for sec in &mut synthetic {
        sec.build_contents(&targets);
    }
    let mut relocs: Vec<Vec<MachRel>> = vec![Vec::new(); ctx.output_sections.len()];
    let merged_relocs: Vec<Vec<MachRel>> =
        merged.par_iter().map(|&osec| section_relocs(&targets, osec)).collect();
    for (&osec, rels) in merged.iter().zip(merged_relocs) {
        relocs[osec.index()] = rels;
    }
    drop(t);

    // After the contents: the relocations, section by section in output
    // order (x86-64's __eh_frame among the merged ones), and then data
    // in code, hints, symbols and strings.
    let mut off = align_to(content_end, 8);
    let mut place = |size: usize| {
        off += size as u64;
        off - size as u64
    };
    let mut reloff = vec![0; ctx.output_sections.len()];
    for &s in &sects {
        match s {
            Sect::Merged(i) => {
                reloff[i.index()] = place(relocs[i.index()].len() * size_of::<MachRel>());
            }
            Sect::Synthetic(i) => {
                synthetic[i].reloff = place(synthetic[i].relocs.len() * size_of::<MachRel>());
            }
        }
    }
    let layout = FileLayout {
        vmsize,
        seg_fileoff,
        seg_filesize: content_end - seg_fileoff,
        diceoff: place(cmds.dice.len() * 8),
        lohoff: place(cmds.loh.as_ref().map_or(0, Vec::len)),
        symoff: place(symtab.table.len() * size_of::<NList>()),
        stroff: place(symtab.table.strtab_size),
    };
    let headers: Vec<MachSection> = sects
        .iter()
        .map(|&s| match s {
            Sect::Merged(i) => {
                section_header(&ctx.output_section(i).hdr, &relocs[i.index()], reloff[i.index()])
            }
            Sect::Synthetic(i) => {
                let sec = &synthetic[i];
                section_header(&sec.hdr, &sec.relocs, sec.reloff)
            }
        })
        .collect();

    let t = ctx.timer("r-copy");
    let mut buf = vec![0u8; off as usize];
    write_load_commands(ctx, &mut buf, &cmds, &headers, &layout, &symtab);
    copy_section_contents(&targets, &merged, &mut buf);
    for sec in &synthetic {
        let fileoff = sec.hdr.fileoff as usize;
        buf[fileoff..fileoff + sec.data.len()].copy_from_slice(&sec.data);
        write_array(&mut buf, sec.reloff as usize, &sec.relocs);
    }
    for &osec in &merged {
        write_array(&mut buf, reloff[osec.index()] as usize, &relocs[osec.index()]);
    }
    crate::chunks::data_in_code::write_entries(&cmds.dice, &mut buf[layout.diceoff as usize..]);
    if let Some(loh) = &cmds.loh {
        let lohoff = layout.lohoff as usize;
        buf[lohoff..lohoff + loh.len()].copy_from_slice(loh);
    }
    let (syms, strtab) =
        buf[layout.symoff as usize..].split_at_mut(symtab.table.len() * size_of::<NList>());
    let strtab = &mut strtab[..symtab.table.strtab_size];
    crate::chunks::symtab::write_symtab(ctx, &symtab.table, syms, strtab);

    drop(t);

    crate::error::checkpoint();
    let t = ctx.timer("r-write");
    output_file::write(&ctx.args.output, &buf);
    drop(t);
    off
}

/// A section of the -r output: merged from input subsections, or
/// synthetic.
#[derive(Clone, Copy)]
enum Sect {
    Merged(OutputSectionId),
    Synthetic(usize),
}

fn sect_hdr<'a, E: Target>(
    ctx: &'a Context<E>,
    synthetic: &'a [SyntheticSection],
    s: Sect,
) -> &'a ChunkHeader {
    match s {
        Sect::Merged(i) => &ctx.output_section(i).hdr,
        Sect::Synthetic(i) => &synthetic[i].hdr,
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
    }
}

/// Every output section, merged or synthetic, in ld64's order: ranked
/// (see section_rank), and first-seen within a rank (a synthetic
/// section after the merged ones).
fn sort_sections<E: Target>(ctx: &Context<E>, synthetic: &[SyntheticSection]) -> Vec<Sect> {
    let mut sects: Vec<Sect> = (0..ctx.output_sections.len())
        .map(|i| Sect::Merged(OutputSectionId::new(i as u32)))
        .chain((0..synthetic.len()).map(Sect::Synthetic))
        .collect();
    let mut segs_seen: Vec<&str> = Vec::new();
    for &s in &sects {
        let seg = sect_hdr(ctx, synthetic, s).segname;
        if !segs_seen.contains(&seg) {
            segs_seen.push(seg);
        }
    }
    sects.sort_by_key(|&s| {
        let hdr = sect_hdr(ctx, synthetic, s);
        let (seg_rank, sect_rank) = section_rank(hdr.segname, &hdr.sectname, hdr.flags);
        (seg_rank, segs_seen.iter().position(|&x| x == hdr.segname), sect_rank)
    });
    sects
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
        hdr.n_sect = i as u8 + 1;
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
                .map(|opt| linker_option_command(opt))
                .collect(),
            dice: crate::chunks::data_in_code::build(ctx, |hdr| hdr.addr),
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

    /// Where the section contents start in the file: past the header,
    /// the load commands and the space ld-prime leaves free after them,
    /// -headerpad (32 unless given), and more when LC_VERSION_MIN_MACOSX,
    /// or no command at all, stands where its estimate of them counted a
    /// 32-byte LC_BUILD_VERSION.
    fn contents_offset(&self, args: &crate::cmdline::Args) -> u64 {
        let pad = args.headerpad + 32u64.saturating_sub(self.version.len() as u64);
        (size_of::<MachHeader>() + self.size()) as u64 + pad
    }
}

/// An LC_LINKER_OPTION command: cmd, cmdsize, count, then the
/// NUL-terminated strings, padded to 8 bytes.
fn linker_option_command(opt: &[Vec<u8>]) -> Vec<u8> {
    let mut cmd = Vec::new();
    cmd.extend_from_slice(&LC_LINKER_OPTION.to_le_bytes());
    cmd.extend_from_slice(&0u32.to_le_bytes());
    cmd.extend_from_slice(&(opt.len() as u32).to_le_bytes());
    for s in opt {
        cmd.extend_from_slice(s);
        cmd.push(0);
    }
    cmd.resize(align_to(cmd.len() as u64, 8) as usize, 0);
    let cmdsize = cmd.len() as u32;
    cmd[4..8].copy_from_slice(&cmdsize.to_le_bytes());
    cmd
}

/// Places the sections' contents in the file from `start`, past the
/// load commands, returning where they end. File offsets mirror
/// addresses, except that the address span of a zero-fill section
/// (with the padding up to the next section) has no file bytes: as in
/// ld64's output, __bss can sit before __LD,__compact_unwind without
/// leaving a hole in the file.
fn assign_file_offsets<E: Target>(
    ctx: &mut Context<E>,
    synthetic: &mut [SyntheticSection],
    sects: &[Sect],
    start: u64,
) -> u64 {
    let mut end = start;
    let mut zerofill_start: Option<u64> = None;
    let mut skipped = 0;
    for &s in sects {
        let hdr = sect_hdr_mut(ctx, synthetic, s);
        if hdr.is_zerofill() {
            zerofill_start.get_or_insert(hdr.addr);
            hdr.fileoff = 0;
            continue;
        }
        if let Some(zerofill_start) = zerofill_start.take() {
            skipped += hdr.addr - zerofill_start;
        }
        hdr.fileoff = start + hdr.addr - skipped;
        end = hdr.fileoff + hdr.size;
    }
    end
}

/// A section the -r output synthesizes rather than merges from input
/// subsections. Its size is known before layout, so it takes its place
/// among the merged sections in ld64's order; its contents are built
/// once addresses and symbols are assigned.
struct SyntheticSection {
    hdr: ChunkHeader,
    kind: SyntheticKind,
    data: Vec<u8>,
    relocs: Vec<MachRel>,
    /// Where the relocations start in the file.
    reloff: u64,
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
        segname: &'static str,
        sectname: &str,
        flags: u32,
        p2align: u32,
        size: u64,
        kind: SyntheticKind,
    ) -> Self {
        let mut hdr = ChunkHeader::new(segname, sectname);
        hdr.flags = flags;
        hdr.p2align = p2align;
        hdr.size = size;
        Self { hdr, kind, data: Vec::new(), relocs: Vec::new(), reloff: 0 }
    }

    /// Builds the contents and relocations, which fill the size the
    /// section was laid out with.
    fn build_contents<E: Target>(&mut self, targets: &RelocTargets<E>) {
        let (data, relocs) = match &self.kind {
            SyntheticKind::ObjcImageInfo => {
                let mut data = vec![0u8; 8];
                data[4..8].copy_from_slice(&targets.ctx.objc_imageinfo.flags.to_le_bytes());
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
/// (create_output_sections folded the inputs' records into
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
    if !ctx.objs.iter().any(|o| o.is_alive && o.objc_image_info.is_some()) {
        return None;
    }
    let (seg, sect) = crate::output_sections::renamed(&ctx.args, ("__DATA", "__objc_imageinfo"));
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
            isec.is_alive()
                && isec.replacement == NO_REPLACEMENT
                && (rec.fde().is_none() || rec.encoding & UNWIND_MODE_MASK == E::UNWIND_MODE_DWARF)
        })
        .collect()
}

/// __LD,__compact_unwind, if any unwind record survives: one 32-byte
/// entry per record.
fn compact_unwind_section<E: Target>(ctx: &Context<E>) -> Option<SyntheticSection> {
    let records = compact_unwind_records(ctx);
    if records.is_empty() {
        return None;
    }
    // Each record keeps the alignment of the section it came from, as
    // an ld-prime atom does, so the section takes the largest.
    let p2align = records
        .par_iter()
        .filter_map(|&i| {
            let obj = &ctx.objs[ctx.isecs[ctx.unwind_records[i].isec as usize].file as usize];
            obj.sect_hdrs.iter().find(|s| s.segname_is("__LD") && s.sectname_is("__compact_unwind"))
        })
        .map(|s| s.p2align)
        .max()
        .unwrap_or(3);
    let size = 32 * records.len() as u64;
    let kind = SyntheticKind::CompactUnwind(records);
    Some(SyntheticSection::new("__LD", "__compact_unwind", S_ATTR_DEBUG, p2align, size, kind))
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
/// CIE and FDE whose function survives (a coalesced-away weak copy's
/// goes with it), laid out per object in input order, as ld64 carries
/// them. The loader kept the FDEs of compactly-encoded functions for
/// this.
fn eh_frame_records<E: Target>(ctx: &Context<E>) -> Vec<(EhRec, u32)> {
    let mut per_obj: HashMap<u32, Vec<(u32, EhRec)>> = HashMap::new();
    let mut cies_used: HashSet<usize> = HashSet::new();
    for (f, fde) in ctx.fdes.iter().enumerate() {
        let isec = &ctx.isecs[fde.isec as usize];
        if !isec.is_alive() || isec.replacement != NO_REPLACEMENT {
            continue;
        }
        per_obj.entry(fde.obj).or_default().push((fde.input_addr, EhRec::Fde(f)));
        if cies_used.insert(fde.cie as usize) {
            let cie = &ctx.cies[fde.cie as usize];
            per_obj
                .entry(cie.obj)
                .or_default()
                .push((cie.input_addr, EhRec::Cie(fde.cie as usize)));
        }
    }
    let mut objs: Vec<u32> = per_obj.keys().copied().collect();
    objs.sort_unstable();
    let mut records = Vec::new();
    let mut off = 0u32;
    for obj in objs {
        let mut recs = per_obj.remove(&obj).unwrap();
        recs.sort_by_key(|r| r.0);
        for (_, r) in recs {
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
        "__TEXT",
        "__eh_frame",
        S_COALESCED | S_ATTR_NO_TOC | S_ATTR_STRIP_STATIC_SYMS | S_ATTR_LIVE_SUPPORT,
        3,
        size,
        SyntheticKind::EhFrame(records),
    ))
}

/// __LD,__compact_unwind's contents, re-synthesized so unwind info
/// survives the merge: one 32-byte entry per record - the function, its
/// length and encoding, the personality and the LSDA - its pointer
/// fields set by UNSIGNED relocations. Each entry is made on all cores.
fn compact_unwind_contents<E: Target>(
    targets: &RelocTargets<E>,
    records: &[usize],
) -> (Vec<u8>, Vec<MachRel>) {
    let ctx = targets.ctx;
    let narrow_fields = narrow_unwind_fields(ctx);
    let mut data = vec![0u8; 32 * records.len()];
    let relocs: Vec<MachRel> = data
        .par_chunks_mut(32)
        .zip(records)
        .enumerate()
        .flat_map_iter(|(i, (entry, &r))| {
            let rec = &ctx.unwind_records[r];
            let at = 32 * i as u32;
            // A field's relocation: r_length 2 (4 bytes) or 3 (8 bytes).
            let narrow = narrow_fields.get(&(rec.isec, rec.input_offset)).copied().unwrap_or(0);
            let len = |field: u32| if narrow & (1 << (field / 8)) != 0 { 2 << 25 } else { 3 << 25 };
            let (func, bits) =
                targets.pointer_to(rec.isec as usize, rec.input_offset as u64, len(0));
            entry[..8].copy_from_slice(&func.to_le_bytes());
            entry[8..12].copy_from_slice(&rec.code_len.to_le_bytes());
            entry[12..16].copy_from_slice(&rec.encoding.to_le_bytes());
            let func = MachRel { r_address: at, bits };
            let personality = rec.personality().map(|p| MachRel {
                r_address: at + 16,
                bits: targets.personality(p) | len(16) | (1 << 27),
            });
            let lsda = rec.lsda().map(|(lsda, off)| {
                let (lsda, bits) = targets.pointer_to(ctx.resolve_isec(lsda), off as u64, len(24));
                entry[24..].copy_from_slice(&lsda.to_le_bytes());
                MachRel { r_address: at + 24, bits }
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
                    // ld-prime writes 4 into the cell on either target,
                    // whatever the object held there (a compiler's
                    // x86-64 CIE holds 4 too).
                    let at = (off + cie.personality_offset) as usize;
                    data[at..at + 4].copy_from_slice(&4u32.to_le_bytes());
                    relocs.push(MachRel {
                        r_address: off + cie.personality_offset,
                        bits: targets.personality(p)
                            | (1 << 24)
                            | (2 << 25)
                            | (1 << 27)
                            | ((E::RELOC_GOTPC as u32) << 28),
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
/// tables, each subsection's on a core of its own. ld-prime writes each
/// atom's relocations by descending offset whatever the input's order,
/// keeping a pair - a SUBTRACTOR and its UNSIGNED, an arm64 ADDEND and
/// its PAGE21 or PAGEOFF12 - in order.
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
            // The entries, and where each group of them starts: one
            // relocation's, a SUBTRACTOR's with the next one's.
            let mut rels: Vec<MachRel> = Vec::new();
            let mut starts: Vec<usize> = Vec::new();
            let mut open = false;
            for rel in crate::input_files::isec_relocs_of(&ctx.objs, isec) {
                if !open {
                    starts.push(rels.len());
                }
                push_reloc(targets, isec, rel, &mut rels);
                open = rel.r_type == E::RELOC_SUBTRACTOR;
            }
            let ends = starts.iter().skip(1).copied().chain([rels.len()]);
            let mut groups: Vec<std::ops::Range<usize>> =
                starts.iter().zip(ends).map(|(&start, end)| start..end).collect();
            groups.sort_by_key(|g| Reverse(rels[g.start].r_address));
            let mut out = Vec::with_capacity(rels.len());
            for g in groups {
                out.extend_from_slice(&rels[g]);
            }
            out
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
    let r_address = (isec.offset as u64 + rel.offset as u64) as u32;
    let (symnum, is_extern) = match targets.out_target(isec, rel) {
        OutTarget::Sym(symnum) => {
            // An explicit addend record precedes relocations whose
            // instruction can't hold one.
            if rel.addend != 0 && E::relocatable_needs_addend(rel.r_type) {
                out.push(MachRel {
                    r_address,
                    bits: (rel.addend as u32 & 0xff_ffff)
                        | (2 << 25)
                        | ((E::RELOC_ADDEND as u32) << 28),
                });
            }
            (symnum, true)
        }
        OutTarget::Section(target, addend) => match targets.atom_target(target, addend) {
            Some((symnum, _)) => (symnum, true),
            None => (ctx.isec_n_sect(&ctx.isecs[target]) as u32, false),
        },
    };
    out.push(MachRel {
        r_address,
        bits: symnum
            | ((rel.is_pcrel as u32) << 24)
            | (rel.size.trailing_zeros() << 25)
            | ((is_extern as u32) << 27)
            | ((rel.r_type as u32) << 28),
    });
}

/// How a -r output refers to a relocation's target.
enum OutTarget {
    /// By the symbol at this index of its symbol table.
    Sym(u32),
    /// Section-relatively: a subsection and the offset in it. A
    /// section-relative input relocation stays so, and so does one
    /// against a label the output drops (see Locals).
    Section(usize, i64),
}

/// How the -r output names the targets of its relocations and pointer
/// fields: by their symbols in its symbol table, by a symbol ld-prime
/// re-derives from the address, or section-relatively.
struct RelocTargets<'a, E: Target> {
    ctx: &'a Context<E>,
    symtab: &'a RSymtab,
    /// The named places (see symbol_places).
    places: Places,
}

impl<'a, E: Target> RelocTargets<'a, E> {
    fn new(ctx: &'a Context<E>, symtab: &'a RSymtab) -> Self {
        let places = symbol_places(ctx, &symtab.index_of_sym);
        Self { ctx, symtab, places }
    }

    /// How the output refers to a relocation's target.
    fn out_target(&self, isec: &InputSection, rel: &Reloc) -> OutTarget {
        let ctx = self.ctx;
        match rel.target() {
            RelocTarget::Sym(idx) => {
                let sym_id = ctx.objs[isec.file as usize].symbols[idx as usize];
                if let Some(symnum) = self.symtab.index_of(sym_id) {
                    return OutTarget::Sym(symnum);
                }
                let sym = &ctx.symbols[sym_id];
                let Some(t) = sym.input_section() else {
                    fatal!("-r: cannot re-emit relocation against {}", sym.name());
                };
                OutTarget::Section(ctx.resolve_isec(t as usize), sym.value as i64 + rel.addend)
            }
            RelocTarget::Section(t) => OutTarget::Section(ctx.resolve_isec(t as usize), rel.addend),
        }
    }

    /// The symbol a reference to offset `off` of subsection `t` names
    /// where ld-prime re-derives it from the address, and the symbol's
    /// address: the nearest named place's first name, or in an object
    /// without subsections, past a place that does not start the
    /// section, its last (see symbol_places).
    fn name_at(&self, t: usize, off: u64) -> Option<(u32, u64)> {
        let (at, first, last, addr) = self.places.at_or_before(t, off)?;
        let whole = !self.ctx.objs[self.ctx.isecs[t].file as usize].subsections_via_symbols;
        Some((if whole && at != 0 && at != off { last } else { first }, addr))
    }

    /// The symbol a section-relative relocation becomes an extern one
    /// against, and its address: the atom's in a section whose atoms
    /// ld64 names itself, else the name of the place.
    fn atom_target(&self, t: usize, addend: i64) -> Option<(u32, u64)> {
        if let Some(atom) = self.symtab.atoms.get(self.ctx, t, addend) {
            return Some(atom);
        }
        self.name_at(t, u64::try_from(addend).ok()?)
    }

    /// A pointer field to offset `off` of subsection `t`: its contents
    /// and its relocation's symbol, length (`len`) and extern bits. ld64
    /// names the target by symbol where one names the place (an extern
    /// relocation, the offset from the symbol in the field); a nameless
    /// one is referred to section-relatively.
    fn pointer_to(&self, t: usize, off: u64, len: u32) -> (u64, u32) {
        let ctx = self.ctx;
        let target = ctx.isec_addr(t) + off;
        match self.name_at(t, off) {
            Some((symnum, addr)) => (target - addr, symnum | len | (1 << 27)),
            None => (target, ctx.isec_n_sect(&ctx.isecs[t]) as u32 | len),
        }
    }

    /// The symbol index of an unwind record's or a CIE's personality
    /// routine, which the output's symbol table names (see
    /// referenced_syms).
    fn personality(&self, p: SymbolId) -> u32 {
        let Some(symnum) = self.symtab.index_of(p) else {
            fatal!("-r: unwind personality lost: {}", self.ctx.symbols[p]);
        };
        symnum
    }
}

/// Where the parts of a -r output lie in the file, besides the sections'
/// contents and relocations: the segment, which the contents make up,
/// and the tables after it.
struct FileLayout {
    vmsize: u64,
    seg_fileoff: u64,
    seg_filesize: u64,
    diceoff: u64,
    lohoff: u64,
    symoff: u64,
    stroff: u64,
}

/// A section's header in the segment command.
fn section_header(hdr: &ChunkHeader, relocs: &[MachRel], reloff: u64) -> MachSection {
    MachSection {
        sectname: str_to_name(&hdr.sectname),
        segname: str_to_name(hdr.segname),
        addr: hdr.addr,
        size: hdr.size,
        offset: hdr.fileoff as u32,
        p2align: hdr.p2align,
        reloff: if relocs.is_empty() { 0 } else { reloff as u32 },
        nreloc: relocs.len() as u32,
        flags: hdr.flags,
        reserved1: 0,
        reserved2: 0,
        reserved3: 0,
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
        .filter(|(i, o)| o.is_alive && !ctx.is_internal(*i))
        .all(|(_, o)| o.subsections_via_symbols);
    let hdr = MachHeader {
        magic: MH_MAGIC_64,
        cputype: E::CPUTYPE,
        cpusubtype: E::CPUSUBTYPE,
        filetype: MH_OBJECT,
        ncmds: cmds.count(),
        sizeofcmds: cmds.size() as u32,
        flags: if subsections { MH_SUBSECTIONS_VIA_SYMBOLS } else { 0 },
        reserved: 0,
    };
    hdr.write_to(buf);
    let mut p = size_of::<MachHeader>();

    let seg = SegmentCommand {
        cmd: LC_SEGMENT_64,
        cmdsize: (size_of::<SegmentCommand>() + size_of_val(headers)) as u32,
        segname: [0; 16],
        vmaddr: 0,
        vmsize: layout.vmsize,
        fileoff: layout.seg_fileoff,
        filesize: layout.seg_filesize,
        maxprot: VM_PROT_READ | VM_PROT_WRITE | VM_PROT_EXECUTE,
        initprot: VM_PROT_READ | VM_PROT_WRITE | VM_PROT_EXECUTE,
        nsects: headers.len() as u32,
        flags: 0,
    };
    seg.write_to(&mut buf[p..]);
    p += size_of::<SegmentCommand>();
    write_array(buf, p, headers);
    p += size_of_val(headers);

    let st = SymtabCommand {
        cmd: LC_SYMTAB,
        cmdsize: size_of::<SymtabCommand>() as u32,
        symoff: layout.symoff as u32,
        nsyms: symtab.table.len() as u32,
        stroff: layout.stroff as u32,
        strsize: symtab.table.strtab_size as u32,
    };
    st.write_to(&mut buf[p..]);
    p += size_of::<SymtabCommand>();

    buf[p..p + cmds.version.len()].copy_from_slice(&cmds.version);
    p += cmds.version.len();

    let dc = LinkEditDataCommand {
        cmd: LC_DATA_IN_CODE,
        cmdsize: size_of::<LinkEditDataCommand>() as u32,
        dataoff: layout.diceoff as u32,
        datasize: (cmds.dice.len() * 8) as u32,
    };
    dc.write_to(&mut buf[p..]);
    p += size_of::<LinkEditDataCommand>();

    for cmd in &cmds.linker_options {
        buf[p..p + cmd.len()].copy_from_slice(cmd);
        p += cmd.len();
    }

    if let Some(loh) = &cmds.loh {
        let cmd = LinkEditDataCommand {
            cmd: LC_LINKER_OPTIMIZATION_HINT,
            cmdsize: size_of::<LinkEditDataCommand>() as u32,
            dataoff: layout.lohoff as u32,
            datasize: loh.len() as u32,
        };
        cmd.write_to(&mut buf[p..]);
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
            jobs.extend(isecs.filter(|isec| !isec.data().is_empty()).map(|isec| (&osec.hdr, isec)));
        }
    }
    let ranges: Vec<std::ops::Range<u64>> = jobs
        .iter()
        .map(|&(hdr, isec)| {
            let start = hdr.fileoff + isec.offset as u64;
            start..start + isec.data().len() as u64
        })
        .collect();
    let slices = output_file::split_ranges(buf, &ranges);
    jobs.into_par_iter().zip(slices).for_each(|((hdr, isec), out)| {
        out.copy_from_slice(isec.data());
        for rel in crate::input_files::isec_relocs_of(&ctx.objs, isec) {
            let here = hdr.addr + isec.offset as u64 + rel.offset as u64;
            rewrite_field(targets, isec, rel, here, &mut out[rel.offset as usize..]);
        }
    });
}

/// Rewrites the field a relocation at address `here` applies to, as a
/// -r output holds it: an x86-64 GOT load's cell, or the address a
/// non-external relocation's field embeds, now in the merged address
/// space - or relative to the atom's symbol where the relocation becomes
/// an extern one against it.
fn rewrite_field<E: Target>(
    targets: &RelocTargets<E>,
    isec: &InputSection,
    rel: &Reloc,
    here: u64,
    field: &mut [u8],
) {
    let ctx = targets.ctx;
    if let Some(cell) = E::RELOCATABLE_GOTPC_CELL
        && rel.r_type == E::RELOC_GOTPC
        && rel.is_pcrel
        && rel.size == 4
    {
        field[..4].copy_from_slice(&cell.to_le_bytes());
        return;
    }
    let OutTarget::Section(target, addend) = targets.out_target(isec, rel) else {
        return;
    };
    // The addend is negative for a target before its section's start.
    let target_addr = ctx.isec_addr(target).wrapping_add_signed(addend);
    if let Some((_, atom_addr)) = targets.atom_target(target, addend) {
        // Now a relocation against the atom's symbol: the field holds
        // the addend relative to it, in the form an object's extern
        // relocation uses.
        let mut val = (target_addr - atom_addr) as i64;
        if rel.is_pcrel {
            val -= E::reloc_bias(rel.r_type);
        }
        match rel.size {
            8 => field[..8].copy_from_slice(&val.to_le_bytes()),
            4 => field[..4].copy_from_slice(&(val as i32).to_le_bytes()),
            _ => {}
        }
        return;
    }
    if rel.r_type == E::RELOC_UNSIGNED && !rel.is_pcrel {
        match rel.size {
            8 => field[..8].copy_from_slice(&target_addr.to_le_bytes()),
            4 => field[..4].copy_from_slice(&(target_addr as u32).to_le_bytes()),
            _ => {}
        }
    } else if rel.is_pcrel {
        // Pcrel non-external fields embed target - (P + 4).
        let val = target_addr.wrapping_sub(here + 4).wrapping_sub(E::reloc_bias(rel.r_type) as u64)
            as u32;
        if rel.size == 4 {
            field[..4].copy_from_slice(&val.to_le_bytes());
        }
    } else {
        error!("-r: unsupported non-external relocation");
    }
}

/// A -r output's symbol and string tables, and where relocations find
/// the atoms ld64 names itself.
struct RSymtab {
    /// The tables, laid out as a final image's are and written by the
    /// same writer (see write_symtab), but with every entry's n_value
    /// set already.
    table: SymtabSection,
    /// Each symbol's index in the table, or u32::MAX if it has none.
    index_of_sym: Vec<u32>,
    /// The linker-named atoms: each one's symbol index and address.
    atoms: LiteralAtoms<(u32, u64)>,
}

impl RSymtab {
    /// A symbol's index in the table, if it has an entry.
    fn index_of(&self, id: SymbolId) -> Option<u32> {
        let index = self.index_of_sym[id as usize];
        (index != u32::MAX).then_some(index)
    }
}

/// A local symbol of a -r output.
#[derive(Clone, Copy)]
struct Local {
    name: &'static str,
    n_type: u8,
    n_desc: u16,
    n_sect: u8,
    addr: u64,
    rename: Rename,
    /// The input symbol it stands for: a label's own, or the first of the
    /// labels of a literal atom ld64 names itself (none, often), whose
    /// other labels stand for it too (see Locals::atom_labels).
    sym: Option<SymbolId>,
    /// The object, the section there and the address, which order the
    /// locals - for an absolute symbol, section 0 and its index in the
    /// object's symbol table.
    at: (u32, u8, u64),
    rank: Rank,
}

impl Local {
    /// Its entry but for the name.
    fn nlist(&self) -> NList {
        let (n_type, n_sect, n_desc) = (self.n_type, self.n_sect, self.n_desc);
        NList { n_strx: 0, n_type, n_sect, n_desc, n_value: self.addr }
    }
}

/// The name a local takes: its own, or one ld64 makes for a literal
/// atom.
#[derive(Clone, Copy, PartialEq)]
enum Rename {
    None,
    Cstring,
    Anon,
}

/// How a local ranks among the names of a place: ld-prime orders them
/// a private external, a local, a weak definition, an ltmpN label, each
/// rank by descending name.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    PrivateExtern,
    Local,
    Weak,
    Ltmp,
}

/// A symbol's address in the -r output.
fn sym_addr<E: Target>(ctx: &Context<E>, id: SymbolId) -> u64 {
    let sym = &ctx.symbols[id];
    match sym.input_section() {
        Some(isec) => ctx.isec_addr(isec as usize) + sym.value,
        None => sym.value,
    }
}

/// Builds a -r output's symbol table as ld-prime lays it out: each
/// object's local symbols in the order of its sections and of their
/// addresses in each (a zerofill section comes by ordinal), then
/// the stabs, opened by an N_SO of their own, then the defined
/// externals and the undefined symbols, each by name. The strings are
/// laid out as a final image's (see layout_strings).
fn build_symtab<E: Target>(ctx: &Context<E>, merged: &[OutputSectionId]) -> RSymtab {
    let t = ctx.timer("r-symtab-locals");
    let referenced = referenced_syms(ctx);
    let mut locals = Locals::new(ctx, merged);
    locals.add_labels(&referenced);
    // Private externals (visibility hidden) are demoted unless
    // -keep_private_externs (which Apple's strip passes to the `ld -r`
    // it runs on each archive member).
    if !ctx.args.keep_private_externs {
        locals.add_private_externs();
    }
    let (locals, atoms, atom_labels) = locals.finish();
    drop(t);

    // Debug-note stabs: ld64 does not merge the inputs' DWARF into a -r
    // output, it names the objects that hold it (N_OSO) and where their
    // symbols landed, and a later link carries the notes through.
    let t = ctx.timer("r-symtab-stabs");
    let stabs = crate::chunks::symtab::plan_stabs(ctx);
    let nstabs: usize = stabs.iter().map(|plan| plan.len()).sum();
    drop(t);

    // The defined externals, then the undefined and tentative symbols.
    let t = ctx.timer("r-symtab-externals");
    let mut externals = defined_externals(ctx);
    externals.extend(undefined_symbols(ctx, &referenced));
    drop(t);

    // The entries, each made on all cores straight into its slot, and
    // their strings.
    let t = ctx.timer("r-symtab-strings");
    let mut table = SymtabSection::new();
    let total = locals.len() + usize::from(nstabs != 0) + externals.len();
    let mut names: Vec<&'static [u8]> = Vec::with_capacity(total);
    table.entries.reserve_exact(total);
    par_push_entries(&mut names, &mut table.entries, &locals, |l| {
        (l.name.as_bytes(), l.nlist(), None)
    });
    if nstabs != 0 {
        names.push(b"");
        table.entries.push((crate::chunks::symtab::STAB_END, None));
    }
    let stabs_start = table.entries.len();
    par_push_entries(&mut names, &mut table.entries, &externals, |&(ent, id)| {
        (ctx.symbols[id].name().as_bytes(), ent, None)
    });
    let strtab_end = crate::chunks::symtab::layout_strings(&mut table.entries, &names, stabs_start);
    table.names = names;

    // Each symbol's index in the table, for the relocations, and its
    // string, for the notes naming it - but for the first local's,
    // whose notes ld-prime gives a copy of their own.
    let nsyms = ctx.symbols.syms.len();
    let index_of_sym: Vec<AtomicU32> =
        (0..nsyms).into_par_iter().map(|_| AtomicU32::new(u32::MAX)).collect();
    let strx_of: Vec<AtomicU32> =
        (0..nsyms).into_par_iter().map(|_| AtomicU32::new(u32::MAX)).collect();
    let entries = &table.entries;
    locals.par_iter().enumerate().for_each(|(i, l)| {
        if let Some(id) = l.sym {
            index_of_sym[id as usize].store(i as u32, Ordering::Relaxed);
            if i != 0 {
                strx_of[id as usize].store(entries[i].0.n_strx, Ordering::Relaxed);
            }
        }
    });
    atom_labels.par_iter().for_each(|&(e, id)| {
        index_of_sym[id as usize].store(e as u32, Ordering::Relaxed);
    });
    externals.par_iter().enumerate().for_each(|(k, &(_, id))| {
        let i = stabs_start + k;
        index_of_sym[id as usize].store((i + nstabs) as u32, Ordering::Relaxed);
        strx_of[id as usize].store(entries[i].0.n_strx, Ordering::Relaxed);
    });
    table.strx_of = strx_of.into_iter().map(AtomicU32::into_inner).collect();
    table.set_stabs(ctx, stabs, stabs_start, strtab_end);
    drop(t);

    RSymtab {
        table,
        index_of_sym: index_of_sym.into_iter().map(AtomicU32::into_inner).collect(),
        atoms,
    }
}

/// The symbols `pred` takes, sorted by name on all cores, ties by index.
/// Each is sorted with its name's sort key (see name_sort_key), which
/// settles most comparisons on one integer.
fn symbols_by_name<E: Target>(ctx: &Context<E>, pred: impl Fn(usize) -> bool + Sync) -> Vec<usize> {
    let mut syms: Vec<_> = (0..ctx.symbols.syms.len())
        .into_par_iter()
        .filter(|&i| pred(i))
        .map(|i| (name_sort_key(ctx.symbols[i].name()), i))
        .collect();
    syms.par_sort_unstable();
    syms.into_par_iter().map(|(_, i)| i).collect()
}

/// A -r output's defined externals, sorted by name, with their entries.
fn defined_externals<E: Target>(ctx: &Context<E>) -> Vec<(NList, SymbolId)> {
    // The n_desc flags a defined global carries in its object, which the
    // next link needs as much as this one did. N_ALT_ENTRY is the
    // critical one: it marks a symbol that does not begin a new
    // subsection (Swift's class metadata symbol $s..CN is an alt entry
    // into the full-metadata object $s..CMf, referenced as CMf+0x18),
    // and a link that splits there re-aligns the tail and moves the
    // symbol away from every non-symbolic reference to it.
    let nsyms = ctx.symbols.syms.len();
    let desc_of: Vec<AtomicU16> = (0..nsyms).into_par_iter().map(|_| AtomicU16::new(0)).collect();
    ctx.objs.par_iter().enumerate().filter(|(_, obj)| obj.is_alive).for_each(|(obj_idx, obj)| {
        let r = obj.global_range();
        for (nlist, &sym_id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
            if !nlist.is_stab()
                && nlist.is_extern()
                && nlist.n_type() != N_UNDF
                && matches!(ctx.symbols[sym_id].file(), Some(FileId::Obj(o)) if o as usize == obj_idx)
            {
                desc_of[sym_id as usize].store(nlist.n_desc, Ordering::Relaxed);
            }
        }
    });

    let keep_pext = ctx.args.keep_private_externs;
    let globals = symbols_by_name(ctx, |i| {
        let sym = &ctx.symbols[i];
        sym.is_extern()
            && (keep_pext || !sym.is_private_extern())
            && matches!(sym.file(), Some(FileId::Obj(_)))
            && sym
                .input_section()
                .is_none_or(|isec| ctx.isecs[ctx.resolve_isec(isec as usize)].is_alive())
    });
    globals
        .par_iter()
        .map(|&i| {
            let sym = &ctx.symbols[i];
            let pext = if sym.is_private_extern() { N_PEXT } else { 0 };
            // An absolute symbol names no atom, and ld-prime gives it no
            // n_desc flags (the assembler marks one N_NO_DEAD_STRIP).
            let Some(input) = sym.input_section() else {
                let ent =
                    NList { n_type: N_ABS | N_EXT | pext, n_value: sym.value, ..NList::default() };
                return (ent, i as u32);
            };
            let n_type = N_SECT | N_EXT | pext;
            let n_sect = ctx.isec_n_sect(&ctx.isecs[ctx.resolve_isec(input as usize)]);
            // N_WEAK_REF on a definition is .weak_def_can_be_hidden: with
            // N_WEAK_DEF it lets a final link auto-hide the symbol (ld-prime
            // makes PLCrashReporter's template instantiations local; ours
            // stayed exported after the -r prelink lost the marker).
            let mut n_desc = desc_of[i].load(Ordering::Relaxed)
                & (N_WEAK_DEF
                    | N_WEAK_REF
                    | N_ALT_ENTRY
                    | N_NO_DEAD_STRIP
                    | N_SYMBOL_RESOLVER
                    | N_COLD_FUNC
                    | REFERENCED_DYNAMICALLY);
            if sym.is_weak_def() {
                n_desc |= N_WEAK_DEF;
            }
            if let Some(FileId::Obj(o)) = sym.file()
                && !ctx.objs[o as usize].subsections_via_symbols
            {
                n_desc = whole_desc(n_desc, true);
            }
            n_desc |= section_desc(ctx, input as usize);
            let n_value = sym_addr(ctx, i as u32);
            (NList { n_strx: 0, n_type, n_sect, n_desc, n_value }, i as u32)
        })
        .collect()
}

/// A -r output's undefined and tentative symbols, sorted by name, with
/// their entries. ld-prime keeps an undefined one only if a relocation
/// refers to it (`referenced`) or the command line makes it an initial
/// undefine (-u): a stray `.globl`, a weak or lazy reference nothing
/// uses, goes.
fn undefined_symbols<E: Target>(ctx: &Context<E>, referenced: &[bool]) -> Vec<(NList, SymbolId)> {
    let forced: HashSet<&str> = ctx.args.forced_undefined.iter().map(String::as_str).collect();
    let undefs = symbols_by_name(ctx, |i| {
        let sym = &ctx.symbols[i];
        sym.is_used()
            && (sym.is_common()
                || !sym.is_defined() && (referenced[i] || forced.contains(sym.name())))
    });
    undefs
        .par_iter()
        .map(|&i| {
            let sym = &ctx.symbols[i];
            let mut n_desc = 0;
            let mut n_value = 0;
            if sym.is_common() {
                n_value = sym.value;
                n_desc |= (sym.common_p2align as u16) << 8;
            } else if sym.is_weak_ref() {
                n_desc |= N_WEAK_REF;
            }
            (NList { n_strx: 0, n_type: N_UNDF | N_EXT, n_sect: 0, n_desc, n_value }, i as u32)
        })
        .collect()
}

/// The atoms ld64 names itself in a -r output's literal sections (see
/// Locals), by subsection and record index.
struct LiteralAtoms<T> {
    /// The record size of each output section whose atoms are named; 0
    /// for one record per subsection.
    entsize_of: HashMap<OutputSectionId, u64>,
    atoms: HashMap<(usize, u64), T>,
}

impl<T: Copy> LiteralAtoms<T> {
    /// The atom a reference to offset `addend` of subsection `t` lands
    /// in, if ld64 names it.
    fn get<E: Target>(&self, ctx: &Context<E>, t: usize, addend: i64) -> Option<T> {
        let ChunkId::Output(osec) = ctx.isecs[t].output_section()? else {
            return None;
        };
        let entsize = *self.entsize_of.get(&osec)?;
        let k = (addend as u64).checked_div(entsize).unwrap_or(0);
        self.atoms.get(&(t, k)).copied()
    }
}

/// The local symbols of a -r output, in ld64's form, as they are
/// gathered. ld-prime makes the literals - the strings of a
/// cstring-literal section, the records of a fixed-size literal section
/// and the atoms has_unnamed_atoms names (CFStrings, selector and class
/// references, UTF-16 and ObjC constant literals) - by content: their
/// labels vanish, all but those of the cstring and fixed-size literals a
/// symbol names (labeled, kept apart). arm64 relocations must name what
/// they refer to, so there ld64 names each such atom itself, and each
/// superclass or protocol reference no label names, with one shared
/// counter: a cstring literal LC<n>, the others l<nnn>, all with N_PEXT
/// set so that a later link can still coalesce them. x86-64 ones
/// refer to them section-relatively, and only the literals of
/// __TEXT,__cstring get names (LC<n>). The entries of the __objc_*list
/// sections get no symbol, and on x86-64 neither do the class and
/// protocol references only a linker-private label (l...) names.
/// Relocations against the vanished labels - by label, or
/// section-relative as x86-64 objects refer to literals - are
/// re-targeted at the new symbols, or are section-relative. Other labels
/// survive, those of the linker-private kind too, except the ltmpN
/// labels the arm64 assembler puts at each section's start: in an object
/// with subsections one survives only where no other symbol names the
/// place, or where a relocation refers to it. Private externals are
/// demoted to non-external symbols that keep N_PEXT
/// (add_private_externs). Each object's are gathered on a core of its
/// own.
struct Locals<'a, E: Target> {
    ctx: &'a Context<E>,
    locals: Vec<Local>,
    /// The literal atoms ld64 names itself: each one's entry in
    /// `locals`.
    atoms: LiteralAtoms<usize>,
    /// The labels of those atoms, in the objects' order.
    atom_labels: Vec<AtomLabel>,
    /// The literal sections whose atoms get no name.
    unnamed: HashSet<OutputSectionId>,
    /// The symbols a subtraction names, on x86-64 (see vanishes).
    subtracted: Vec<bool>,
}

/// A label on a literal atom ld64 names itself, which stands for the
/// atom (see Locals): the atom's entry among the locals, and the
/// label's symbol.
type AtomLabel = (usize, SymbolId);

/// The size of the records of a literal subsection whose atoms ld-prime
/// makes by content, 0 for one record per subsection (as C string
/// literals are split), or None for any other subsection: that of a
/// section of another kind, or of a __ustring section without
/// subsections, which is one atom (see has_unnamed_atoms).
fn literal_size<E: Target>(ctx: &Context<E>, isec: usize) -> Option<u64> {
    let h = ctx.hdr_of(&ctx.isecs[isec]);
    match h.section_type() {
        S_CSTRING_LITERALS => return Some(0),
        S_4BYTE_LITERALS => return Some(4),
        S_8BYTE_LITERALS => return Some(8),
        S_16BYTE_LITERALS => return Some(16),
        _ => {}
    }
    let split = ctx.objs[ctx.isecs[isec].file as usize].subsections_via_symbols;
    if !crate::passes::has_unnamed_atoms(h, split) {
        // Superclass and protocol references are cut one per pointer
        // too; on arm64 ld-prime names those no label names (a labeled
        // one keeps its label).
        return (E::CPUTYPE == CPU_TYPE_ARM64 && crate::passes::is_class_or_protocol_ref(h))
            .then_some(8);
    }
    match h.sectname() {
        "__cfstring" => Some(32),
        "__objc_selrefs" | "__objc_classrefs" | "__objc_superrefs" | "__objc_protorefs" => Some(8),
        _ => Some(0),
    }
}

impl<'a, E: Target> Locals<'a, E> {
    /// Starts the locals with the literal atoms ld64 names itself.
    fn new(ctx: &'a Context<E>, merged: &[OutputSectionId]) -> Self {
        let names_literals = |segname: &str, sectname: &str| {
            E::CPUTYPE == CPU_TYPE_ARM64 || (segname == "__TEXT" && sectname == "__cstring")
        };

        let mut locals = Vec::new();
        let mut atoms = LiteralAtoms { entsize_of: HashMap::new(), atoms: HashMap::new() };
        let mut unnamed = HashSet::new();
        for &chunk_idx in merged {
            let chunk = ctx.output_section(chunk_idx);
            let Some(entsize) = chunk.members.iter().find_map(|&id| literal_size(ctx, id as usize))
            else {
                continue;
            };
            if !names_literals(chunk.hdr.segname, &chunk.hdr.sectname) {
                unnamed.insert(chunk_idx);
                continue;
            }
            atoms.entsize_of.insert(chunk_idx, entsize);
            for &id in &chunk.members {
                let id = id as usize;
                let isec = &ctx.isecs[id];
                // A literal a label names keeps that label instead, as
                // does a whole section that is one atom.
                if !isec.is_alive()
                    || isec.replacement != NO_REPLACEMENT
                    || isec.is_labeled()
                    || literal_size(ctx, id).is_none()
                {
                    continue;
                }
                let size = isec.data().len() as u64;
                if size == 0 {
                    continue;
                }
                let n = if entsize == 0 { 1 } else { size.div_ceil(entsize) };
                for k in 0..n {
                    atoms.atoms.insert((id, k), locals.len());
                    locals.push(Local {
                        name: "",
                        n_type: N_PEXT | N_SECT,
                        n_desc: section_desc(ctx, id),
                        n_sect: chunk.hdr.n_sect,
                        addr: chunk.hdr.addr + isec.offset as u64 + k * entsize,
                        rename: if chunk.hdr.flags & SECTION_TYPE == S_CSTRING_LITERALS {
                            Rename::Cstring
                        } else {
                            Rename::Anon
                        },
                        sym: None,
                        at: (isec.file, isec.shndx as u8 + 1, isec.input_addr as u64 + k * entsize),
                        rank: Rank::Local,
                    });
                }
            }
        }

        let subtracted =
            if E::CPUTYPE == CPU_TYPE_ARM64 { Vec::new() } else { subtracted_syms(ctx) };
        Self { ctx, locals, atoms, atom_labels: Vec::new(), unnamed, subtracted }
    }

    /// Whether a label vanishes. On x86-64 the label of a literal left
    /// unnamed vanishes, as does a linker-private (l...) name of a class
    /// or protocol reference, a demoted private external's too (Swift's
    /// protocol references) - unless a subtraction names it, which has
    /// no section-relative form (ld-prime fails an assertion on it).
    fn vanishes(&self, isec: usize, sym_id: SymbolId) -> bool {
        let ctx = self.ctx;
        if E::CPUTYPE == CPU_TYPE_ARM64 || self.subtracted[sym_id as usize] {
            return false;
        }
        let t = &ctx.isecs[isec];
        if let Some(ChunkId::Output(osec)) = t.output_section()
            && self.unnamed.contains(&osec)
            && literal_size(ctx, isec).is_some()
        {
            return !t.is_labeled();
        }
        let h = ctx.hdr_of(t);
        h.segname_is("__DATA")
            && (h.sectname_is("__objc_superrefs") || h.sectname_is("__objc_protorefs"))
            && ctx.symbols[sym_id].name().starts_with('l')
    }

    /// Whether a subsection is an entry of an __objc_*list section, which
    /// gets no symbol but its aliases (see objc_list_aliases).
    fn in_unnamed_list(&self, isec: usize) -> bool {
        crate::passes::is_unnamed_objc_list(self.ctx.hdr_of(&self.ctx.isecs[isec]))
    }

    /// Adds the objects' local labels that survive, `referenced` being
    /// the symbols the output's relocations name.
    fn add_labels(&mut self, referenced: &[bool]) {
        let per_obj: Vec<(Vec<Local>, Vec<AtomLabel>)> = (0..self.ctx.objs.len())
            .into_par_iter()
            .map(|obj_idx| self.object_labels(obj_idx, referenced))
            .collect();
        for (labels, atom_labels) in per_obj {
            self.locals.extend(labels);
            for (e, sym_id) in atom_labels {
                self.locals[e].sym.get_or_insert(sym_id);
                self.atom_labels.push((e, sym_id));
            }
        }
    }

    /// An object's local labels that survive, and those that stand for
    /// literal atoms ld64 names itself, each with its atom's entry.
    fn object_labels(&self, obj_idx: usize, referenced: &[bool]) -> (Vec<Local>, Vec<AtomLabel>) {
        let ctx = self.ctx;
        let obj = &ctx.objs[obj_idx];
        let mut labels = Vec::new();
        let mut atom_labels = Vec::new();
        if !obj.is_alive {
            return (labels, atom_labels);
        }
        // Without subsections the object's sections are whole atoms,
        // which ld64 marks no-dead-strip - every symbol, the
        // assembler's ltmpN labels (they name the atoms) included -
        // and an alt entry means nothing there.
        let whole = !obj.subsections_via_symbols;
        let aliases = crate::passes::objc_list_aliases(ctx, obj);
        let named_at = named_places(ctx, obj);
        for i in obj.local_range() {
            let (nlist, sym_id) = (&obj.nlists[i], obj.symbols[i]);
            if nlist.is_stab() || nlist.is_extern() {
                continue;
            }
            let sym = &ctx.symbols[sym_id];
            let Some(input) = sym.input_section().map(|i| i as usize) else {
                if nlist.n_type() == N_ABS {
                    labels.push(self.absolute(obj_idx, i, sym_id, N_ABS, Rank::Local));
                }
                continue;
            };
            let isec = ctx.resolve_isec(input);
            if !ctx.isecs[isec].is_alive() || sym.name().is_empty() {
                continue;
            }
            // A label on an atom ld64 names itself stands for that
            // atom.
            if let Some(e) = self.atoms.get(ctx, isec, sym.value as i64) {
                atom_labels.push((e, sym_id));
                continue;
            }
            if (self.in_unnamed_list(isec) && !referenced[sym_id as usize] && !aliases.contains(&i))
                || self.vanishes(isec, sym_id)
            {
                continue;
            }
            if sym.name().starts_with("ltmp")
                && !whole
                && !referenced[sym_id as usize]
                && named_at.contains(&(nlist.n_sect, nlist.n_value))
            {
                continue;
            }
            // A labeled literal is an atom of its own, not a whole
            // section's.
            let whole = whole && !ctx.isecs[isec].is_labeled();
            // An alias of a list entry is not the atom, which the
            // section's no_dead_strip marks.
            let section_desc = if aliases.contains(&i) { 0 } else { section_desc(ctx, input) };
            labels.push(Local {
                name: sym.name(),
                n_type: nlist.n_type,
                n_desc: whole_desc(nlist.n_desc, whole) | section_desc,
                n_sect: ctx.isec_n_sect(&ctx.isecs[isec]),
                addr: sym_addr(ctx, sym_id),
                rename: Rename::None,
                sym: Some(sym_id),
                at: (obj_idx as u32, nlist.n_sect, nlist.n_value),
                rank: if sym.name().starts_with("ltmp") { Rank::Ltmp } else { Rank::Local },
            });
        }
        (labels, atom_labels)
    }

    /// Adds the private externals, which become non-external symbols in
    /// a -r output, as in ld64 - N_PEXT still set, which nm reports as
    /// "was a private external".
    fn add_private_externs(&mut self) {
        let per_obj: Vec<Vec<Local>> = (0..self.ctx.objs.len())
            .into_par_iter()
            .map(|obj_idx| self.object_private_externs(obj_idx))
            .collect();
        self.locals.extend(per_obj.into_iter().flatten());
    }

    /// An object's private externals, as its locals.
    fn object_private_externs(&self, obj_idx: usize) -> Vec<Local> {
        let ctx = self.ctx;
        let obj = &ctx.objs[obj_idx];
        let mut out = Vec::new();
        if !obj.is_alive {
            return out;
        }
        for i in obj.global_range() {
            let (nlist, sym_id) = (&obj.nlists[i], obj.symbols[i]);
            let sym = &ctx.symbols[sym_id];
            // Only the copy that won resolution is emitted.
            if nlist.is_stab()
                || !nlist.is_extern()
                || !sym.is_private_extern()
                || !matches!(sym.file(), Some(FileId::Obj(o)) if o as usize == obj_idx)
            {
                continue;
            }
            let Some(input) = sym.input_section().map(|i| i as usize) else {
                if nlist.n_type() == N_ABS {
                    out.push(self.absolute(
                        obj_idx,
                        i,
                        sym_id,
                        N_PEXT | N_ABS,
                        Rank::PrivateExtern,
                    ));
                }
                continue;
            };
            let isec = ctx.resolve_isec(input);
            if !ctx.isecs[isec].is_alive()
                || self.in_unnamed_list(isec)
                || self.vanishes(isec, sym_id)
            {
                continue;
            }
            out.push(Local {
                name: sym.name(),
                n_type: N_PEXT | N_SECT,
                n_desc: whole_desc(
                    nlist.n_desc & (N_ALT_ENTRY | N_NO_DEAD_STRIP | N_WEAK_DEF),
                    !obj.subsections_via_symbols,
                ) | section_desc(ctx, input),
                n_sect: ctx.isec_n_sect(&ctx.isecs[isec]),
                addr: sym_addr(ctx, sym_id),
                rename: Rename::None,
                sym: Some(sym_id),
                at: (obj_idx as u32, nlist.n_sect, nlist.n_value),
                rank: if sym.is_weak_def() { Rank::Weak } else { Rank::PrivateExtern },
            });
        }
        out
    }

    /// Absolute symbol `i` of object `obj_idx`, which is in no section:
    /// ld-prime lists an object's absolute symbols after the symbols of
    /// its sections, in the object's order, and gives them no n_desc
    /// flags (the assembler marks one N_NO_DEAD_STRIP).
    fn absolute(
        &self,
        obj_idx: usize,
        i: usize,
        sym_id: SymbolId,
        n_type: u8,
        rank: Rank,
    ) -> Local {
        let sym = &self.ctx.symbols[sym_id];
        Local {
            name: sym.name(),
            n_type,
            n_desc: 0,
            n_sect: 0,
            addr: sym.value,
            rename: Rename::None,
            sym: Some(sym_id),
            at: (obj_idx as u32, 0, i as u64),
            rank,
        }
    }

    /// The locals in ld-prime's order, the literal atoms ld64 names
    /// numbered in it with one counter, and those atoms' symbol indices
    /// (the locals open the symbol table) and addresses, and their
    /// labels' (see atom_labels).
    fn finish(self) -> (Vec<Local>, LiteralAtoms<(u32, u64)>, Vec<AtomLabel>) {
        let Self { ctx, locals, atoms, atom_labels, .. } = self;
        // A stable sort, by object first: each object's locals as they
        // were gathered where all else ties.
        let mut order: Vec<usize> = (0..locals.len()).collect();
        let place = |l: &Local| (l.at.0, l.n_type & N_TYPE == N_ABS, l.at.1, l.at.2);
        order.par_sort_by(|&a, &b| {
            let (a, b) = (&locals[a], &locals[b]);
            place(a).cmp(&place(b)).then_with(|| (a.rank, b.name).cmp(&(b.rank, a.name)))
        });
        let mut index_of = vec![0u32; locals.len()];
        for (i, &e) in order.iter().enumerate() {
            index_of[e] = i as u32;
        }
        let mut locals: Vec<Local> = order.par_iter().map(|&e| locals[e]).collect();

        let mut counter = 1u32;
        for l in &mut locals {
            let name = match l.rename {
                Rename::None => continue,
                Rename::Cstring => format!("LC{counter}"),
                Rename::Anon => format!("l{counter:03}"),
            };
            l.name = name.leak();
            counter += 1;
        }

        // The first name at a place names the atom, unless an external
        // there does; the others are its aliases. An alias in an
        // initializer or terminator list is not marked no-dead-strip
        // (see in_init_term_list). (A name ld64 makes for a literal
        // record has no symbol.)
        let in_lists: Vec<usize> = (0..locals.len())
            .into_par_iter()
            .filter(|&i| locals[i].sym.is_some_and(|s| in_init_term_list(ctx, s)))
            .collect();
        if !in_lists.is_empty() {
            let externals = external_places(ctx);
            for i in in_lists {
                let at = locals[i].at;
                if (i > 0 && locals[i - 1].at == at) || externals.contains(&at) {
                    locals[i].n_desc &= !N_NO_DEAD_STRIP;
                }
            }
        }

        let atoms = LiteralAtoms {
            entsize_of: atoms.entsize_of,
            atoms: atoms
                .atoms
                .into_iter()
                .map(|(key, e)| {
                    let i = index_of[e];
                    (key, (i, locals[i as usize].addr))
                })
                .collect(),
        };
        let atom_labels =
            atom_labels.into_iter().map(|(e, id)| (index_of[e] as usize, id)).collect();
        (locals, atoms, atom_labels)
    }
}
