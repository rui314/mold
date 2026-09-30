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

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::chunks::{ChunkId, OutputSectionId};
use crate::context::Context;
use crate::error;
use crate::fatal;
use crate::input_files::FileId;
use crate::input_sections::RelocTarget;
use crate::macho::*;
use crate::output_file;
use crate::target::Target;
use crate::util::{align_to, encode_uleb};

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
fn in_init_term_list<E: Target>(ctx: &Context<E>, sym: crate::symbol::SymbolId) -> bool {
    let Some(isec) = ctx.symbols[sym].input_section() else { return false };
    let h = ctx.hdr_of(&ctx.isecs[isec as usize]);
    matches!(h.section_type(), S_MOD_INIT_FUNC_POINTERS | S_MOD_TERM_FUNC_POINTERS)
        && h.flags & S_ATTR_NO_DEAD_STRIP == 0
}

/// Where the -r output's defined externals sit, as (object, section,
/// address): an external names its atom over any local there (a weak
/// one only in an object with subsections).
fn external_places<E: Target>(ctx: &Context<E>) -> HashSet<(u32, u8, u64)> {
    let mut places = HashSet::new();
    for (obj_idx, obj) in ctx.objs.iter().enumerate() {
        if !obj.is_alive {
            continue;
        }
        let r = obj.global_range();
        for (nlist, &sym_id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
            let sym = &ctx.symbols[sym_id];
            if !nlist.is_stab()
                && nlist.is_extern()
                && nlist.n_type() == N_SECT
                && (ctx.args.keep_private_externs || !sym.is_private_extern())
                && (obj.subsections_via_symbols || !sym.is_weak_def())
                && matches!(sym.file(), Some(FileId::Obj(o)) if o as usize == obj_idx)
            {
                places.insert((obj_idx as u32, nlist.n_sect, nlist.n_value));
            }
        }
    }
    places
}

/// The places the -r output's symbols name, keyed by (subsection,
/// offset): the symbol indices of the first and the last of their
/// names, and their address. ld-prime ranks the names of a place
/// non-weak before weak, then global, private external and local, each
/// by descending name, an assembler's ltmpN label last. A reference to
/// the place names the first, and so does one past it, into the atom's
/// bytes: the names are aliases of one atom. But in an object without
/// subsections only those at the start of a section are; elsewhere
/// each name is an atom of its own, all empty but the last, which
/// holds the bytes and is the one a reference into them names.
fn symbol_places<E: Target>(
    ctx: &Context<E>,
    index_of_sym: &HashMap<crate::symbol::SymbolId, u32>,
) -> BTreeMap<(usize, u64), (u32, u32, u64)> {
    let mut names: Vec<_> = index_of_sym
        .iter()
        .filter_map(|(&id, &symnum)| {
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
    names.sort_unstable();
    names
        .chunk_by(|a, b| a.0 == b.0)
        .map(|names| {
            let (isec, at) = names[0].0;
            (names[0].0, (names[0].2, names[names.len() - 1].2, ctx.isec_addr(isec) + at))
        })
        .collect()
}

/// The symbols a live input section's relocations refer to by name.
fn referenced_syms<E: Target>(ctx: &Context<E>) -> HashSet<crate::symbol::SymbolId> {
    let mut syms = HashSet::new();
    for isec in ctx.isecs.iter() {
        if !isec.is_alive() || ctx.is_internal(isec.file as usize) {
            continue;
        }
        for rel in crate::input_files::isec_relocs_of(&ctx.objs, isec) {
            if let RelocTarget::Sym(idx) = rel.target() {
                syms.insert(ctx.objs[isec.file as usize].symbols[idx as usize]);
            }
        }
    }
    syms
}

/// The symbols the terms of a subtraction (a SUBTRACTOR and the
/// relocation it pairs with) refer to.
fn subtracted_syms<E: Target>(ctx: &Context<E>) -> HashSet<crate::symbol::SymbolId> {
    let mut syms = HashSet::new();
    for isec in ctx.isecs.iter() {
        if !isec.is_alive() || ctx.is_internal(isec.file as usize) {
            continue;
        }
        for rel in crate::input_files::isec_relocs_of(&ctx.objs, isec) {
            if let RelocTarget::Sym(idx) = rel.target()
                && (rel.r_type == E::RELOC_SUBTRACTOR || rel.is_subtracted)
            {
                syms.insert(ctx.objs[isec.file as usize].symbols[idx as usize]);
            }
        }
    }
    syms
}

/// The places an object names with a symbol other than an assembler
/// temporary (ltmpN), where an ltmpN label is a mere alias.
fn named_places<E: Target>(ctx: &Context<E>) -> HashSet<(usize, u8, u64)> {
    let mut places = HashSet::new();
    for (obj_idx, obj) in ctx.objs.iter().enumerate() {
        if !obj.is_alive {
            continue;
        }
        for (nlist, &sym_id) in obj.nlists.iter().zip(&obj.symbols) {
            if !nlist.is_stab()
                && nlist.n_type() == N_SECT
                && !ctx.symbols[sym_id].name().starts_with("ltmp")
            {
                places.insert((obj_idx, nlist.n_sect, nlist.n_value));
            }
        }
    }
    places
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
            if isec.is_alive() && isec.replacement == crate::input_sections::NO_REPLACEMENT {
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

pub fn link<E: Target>(ctx: &mut Context<E>) {
    // Lay out the merged sections from address zero, zero-fill
    // sections last: an object's file image mirrors its address
    // space (each section's file offset is the segment's plus its
    // address), so content sections must precede the sections that
    // occupy addresses but no file bytes.
    // Synthetic sections: the merged __objc_imageinfo, the
    // re-synthesized __LD,__compact_unwind and __TEXT,__eh_frame. Their
    // sizes are known before layout, so they take their places among
    // the merged sections in ld64's order; their contents are built
    // once addresses are assigned. `patches` are self-relative pointer
    // cells filled then: value = target_addr - (section_addr + offset).
    struct ExtraSection {
        segname: &'static str,
        sectname: &'static str,
        flags: u32,
        p2align: u8,
        size: u64,
        data: Vec<u8>,
        relocs: Vec<MachRel>,
        patches: Vec<(u32, u64, u8)>,
        addr: u64,
        fileoff: u64,
        reloff: u64,
    }
    let new_extra = |segname, sectname, flags, p2align, size| ExtraSection {
        segname,
        sectname,
        flags,
        p2align,
        size,
        data: Vec::new(),
        relocs: Vec::new(),
        patches: Vec::new(),
        addr: 0,
        fileoff: 0,
        reloff: 0,
    };
    let mut extras: Vec<ExtraSection> = Vec::new();

    // The merged __objc_imageinfo (create_output_sections folded the
    // inputs' records into ctx.objc_imageinfo.flags). The record is
    // what makes the Objective-C runtime look at an image at all:
    // without it, dyld never hands the image to the runtime, so no
    // class or category it defines is registered (a class referenced
    // from another image then dies with "Attempt to use unknown
    // class", and categories on framework classes never attach).
    // A prelinked object lacking it silently poisons the image that
    // links it. ld64 writes it into __DATA in a -r output, and
    // ld-prime renames it like the input sections (but not the
    // __eh_frame and __compact_unwind below).
    if ctx.objs.iter().any(|o| o.is_alive && o.objc_image_info.is_some()) {
        let (seg, sect) = crate::passes::renamed(&ctx.args, ("__DATA", "__objc_imageinfo"));
        let mut e = new_extra(seg, sect, 0, 2, 8);
        e.data = vec![0u8; 8];
        e.data[4..8].copy_from_slice(&ctx.objc_imageinfo.flags.to_le_bytes());
        extras.push(e);
    }

    // __LD,__compact_unwind: one 32-byte entry per surviving record.
    // An input's DWARF-mode record is copied as it came (ld64 does;
    // the next link regenerates its encoding from the FDE), but a
    // record we synthesized from an FDE alone is not: the next link
    // synthesizes it again from the __eh_frame emitted below. A
    // coalesced-away weak definition's record goes with it.
    let cu_kept: Vec<usize> = (0..ctx.unwind_records.len())
        .filter(|&i| {
            let rec = &ctx.unwind_records[i];
            let isec = &ctx.isecs[rec.isec as usize];
            isec.is_alive()
                && isec.replacement == crate::input_sections::NO_REPLACEMENT
                && (rec.fde().is_none() || rec.encoding & UNWIND_MODE_MASK == E::UNWIND_MODE_DWARF)
        })
        .collect();
    let mut cu_slot = None;
    if !cu_kept.is_empty() {
        // Each record keeps the alignment of the section it came from,
        // as an ld-prime atom does, so the section takes the largest.
        let p2align = cu_kept
            .iter()
            .filter_map(|&i| {
                let obj = &ctx.objs[ctx.isecs[ctx.unwind_records[i].isec as usize].file as usize];
                obj.sect_hdrs
                    .iter()
                    .find(|s| s.segname() == "__LD" && s.sectname() == "__compact_unwind")
            })
            .map(|s| s.p2align as u8)
            .max()
            .unwrap_or(3);
        cu_slot = Some(extras.len());
        extras.push(new_extra(
            "__LD",
            "__compact_unwind",
            S_ATTR_DEBUG,
            p2align,
            32 * cu_kept.len() as u64,
        ));
    }

    // __TEXT,__eh_frame: every input CIE and FDE whose function
    // survives (a coalesced-away weak copy's goes with it), laid out
    // per object in input order, as ld64 carries them. The loader kept
    // the FDEs of compactly-encoded functions for this.
    #[derive(Clone, Copy)]
    enum EhRec {
        Cie(usize),
        Fde(usize),
    }
    let mut eh_records: Vec<(EhRec, u32)> = Vec::new();
    {
        let mut per_obj: HashMap<u32, Vec<(u32, EhRec)>> = HashMap::new();
        let mut cies_used: HashSet<usize> = HashSet::new();
        for (f, fde) in ctx.fdes.iter().enumerate() {
            let isec = &ctx.isecs[fde.isec as usize];
            if !isec.is_alive() || isec.replacement != crate::input_sections::NO_REPLACEMENT {
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
        let mut off = 0u32;
        for obj in objs {
            let mut recs = per_obj.remove(&obj).unwrap();
            recs.sort_by_key(|r| r.0);
            for (_, r) in recs {
                eh_records.push((r, off));
                off += match r {
                    EhRec::Cie(c) => ctx.cies[c].data.len() as u32,
                    EhRec::Fde(f) => ctx.fdes[f].data.len() as u32,
                };
            }
        }
    }
    let mut eh_slot = None;
    if !eh_records.is_empty() {
        let size: u64 = eh_records
            .iter()
            .map(|&(r, _)| match r {
                EhRec::Cie(c) => ctx.cies[c].data.len() as u64,
                EhRec::Fde(f) => ctx.fdes[f].data.len() as u64,
            })
            .sum();
        eh_slot = Some(extras.len());
        extras.push(new_extra(
            "__TEXT",
            "__eh_frame",
            S_COALESCED | S_ATTR_NO_TOC | S_ATTR_STRIP_STATIC_SYMS | S_ATTR_LIVE_SUPPORT,
            3,
            size,
        ));
    }

    // Every output section, merged or synthetic, in ld64's order:
    // ranked, and first-seen within a rank (a synthetic section after
    // the merged ones).
    #[derive(Clone, Copy)]
    enum Sect {
        Chunk(OutputSectionId),
        Extra(usize),
    }
    let mut sects: Vec<Sect> = (0..ctx.output_sections.len())
        .map(|i| Sect::Chunk(OutputSectionId::new(i as u32)))
        .chain((0..extras.len()).map(Sect::Extra))
        .collect();
    let name_of = |s: Sect| match s {
        Sect::Chunk(i) => {
            let h = &ctx.output_section(i).hdr;
            (h.segname, &*h.sectname, h.flags)
        }
        Sect::Extra(i) => (extras[i].segname, extras[i].sectname, extras[i].flags),
    };
    let mut segs_seen: Vec<&str> = Vec::new();
    for &s in &sects {
        let seg = name_of(s).0;
        if !segs_seen.contains(&seg) {
            segs_seen.push(seg);
        }
    }
    sects.sort_by_key(|&s| {
        let (seg, sect, flags) = name_of(s);
        let (seg_rank, sect_rank) = section_rank(seg, sect, flags);
        (seg_rank, segs_seen.iter().position(|&x| x == seg), sect_rank)
    });

    // Addresses run from zero in that order; zero-fill sections take
    // address space like any other (ld64 leaves them in place too).
    let mut addr: u64 = 0;
    for &s in &sects {
        match s {
            Sect::Chunk(i) => {
                let hdr = &mut ctx.output_section_mut(i).hdr;
                addr = align_to(addr, 1 << hdr.p2align);
                hdr.addr = addr;
                addr += hdr.size;
            }
            Sect::Extra(i) => {
                addr = align_to(addr, 1 << extras[i].p2align);
                extras[i].addr = addr;
                addr += extras[i].size;
            }
        }
    }
    let vmsize = addr;
    let section_chunks: Vec<OutputSectionId> = sects
        .iter()
        .filter_map(|s| match *s {
            Sect::Chunk(i) => Some(i),
            Sect::Extra(_) => None,
        })
        .collect();

    // Section ordinals are 1-based positions among the emitted
    // sections, synthetic ones included.
    let mut extra_ordinals = vec![0u8; extras.len()];
    for (i, &s) in sects.iter().enumerate() {
        match s {
            Sect::Chunk(id) => ctx.output_section_mut(id).hdr.n_sect = i as u8 + 1,
            Sect::Extra(e) => extra_ordinals[e] = i as u8 + 1,
        }
    }
    let RSymtab { nlists: nlists_out, strtab, index_of_sym, atoms, entsize_of } =
        build_symtab(ctx, &section_chunks);
    // The symbol a reference to offset `off` of subsection `t` names
    // where ld-prime re-derives it from the address, and the symbol's
    // address: the nearest named place's first name, or in an object
    // without subsections, past a place that does not start the
    // section, its last (see symbol_places).
    let places = symbol_places(ctx, &index_of_sym);
    let name_at = |t: usize, off: u64| -> Option<(u32, u64)> {
        let (&(isec, at), &(first, last, addr)) = places.range(..=(t, off)).next_back()?;
        if isec != t {
            return None;
        }
        let whole = !ctx.objs[ctx.isecs[t].file as usize].subsections_via_symbols;
        Some((if whole && at != 0 && at != off { last } else { first }, addr))
    };
    // The symbol a section-relative relocation becomes an extern one
    // against, and its address: the atom's in a section whose atoms
    // ld64 names itself, else the name of the place.
    let atom_target = |t: usize, addend: i64| -> Option<(u32, u64)> {
        if let Some(ChunkId::Output(osec)) = ctx.isecs[t].output_section()
            && let Some(&entsize) = entsize_of.get(&osec)
            && let Some(&atom) = atoms.get(&(t, (addend as u64).checked_div(entsize).unwrap_or(0)))
        {
            return Some(atom);
        }
        name_at(t, u64::try_from(addend).ok()?)
    };

    // Re-synthesize __LD,__compact_unwind so unwind info survives the
    // merge: one 32-byte entry per record, its pointer fields set by
    // UNSIGNED relocations. ld64 names the function and the LSDA by
    // symbol where one names the place (an extern relocation, the
    // offset from the symbol in the field); a nameless one is referred
    // to section-relatively.
    let pointer_to = |t: usize, off: u64, len: u32| -> (u64, u32) {
        let target = ctx.isec_addr(t) + off;
        match name_at(t, off) {
            Some((symnum, addr)) => (target - addr, symnum | len | (1 << 27)),
            None => (target, ctx.isec_n_sect(&ctx.isecs[t]) as u32 | len),
        }
    };
    let narrow_fields = narrow_unwind_fields(ctx);
    let mut cu_data: Vec<u8> = Vec::new();
    let mut cu_relocs: Vec<MachRel> = Vec::new();
    for &r in &cu_kept {
        let rec = &ctx.unwind_records[r];
        let entry = cu_data.len() as u32;
        // A field's relocation: r_length 2 (4 bytes) or 3 (8 bytes).
        let narrow = narrow_fields.get(&(rec.isec, rec.input_offset)).copied().unwrap_or(0);
        let len = |field: u32| if narrow & (1 << (field / 8)) != 0 { 2 << 25 } else { 3 << 25 };
        let (func, bits) = pointer_to(rec.isec as usize, rec.input_offset as u64, len(0));
        cu_data.extend_from_slice(&func.to_le_bytes());
        cu_relocs.push(MachRel { r_address: entry, bits });
        cu_data.extend_from_slice(&rec.code_len.to_le_bytes());
        cu_data.extend_from_slice(&rec.encoding.to_le_bytes());

        match rec.personality() {
            Some(p) => {
                let Some(&symnum) = index_of_sym.get(&p) else {
                    fatal!("-r: unwind personality lost: {}", ctx.symbols[p]);
                };
                cu_data.extend_from_slice(&0u64.to_le_bytes());
                cu_relocs
                    .push(MachRel { r_address: entry + 16, bits: symnum | len(16) | (1 << 27) });
            }
            None => cu_data.extend_from_slice(&0u64.to_le_bytes()),
        }

        match rec.lsda() {
            Some((lsda, off)) => {
                let (lsda, bits) = pointer_to(ctx.resolve_isec(lsda), off as u64, len(24));
                cu_data.extend_from_slice(&lsda.to_le_bytes());
                cu_relocs.push(MachRel { r_address: entry + 24, bits });
            }
            None => cu_data.extend_from_slice(&0u64.to_le_bytes()),
        }
    }
    if let Some(slot) = cu_slot {
        debug_assert_eq!(cu_data.len() as u64, extras[slot].size);
        extras[slot].data = cu_data;
        extras[slot].relocs = cu_relocs;
    }

    // __TEXT,__eh_frame, in ld-prime's form: the input CIEs and FDEs
    // copied through with their self-relative fields recomputed for
    // the merged layout - the CIE pointer, pc_begin and the LSDA
    // pointer - and no symbols or relocations of their own but the
    // CIE's personality cell, a 4-byte pcrel GOT reference (the shape
    // compilers emit). ld64 classic named every CIE EH_Frame1 and
    // every FDE func.eh and wrote the fields as SUBTRACTOR pairs
    // against them; ld-prime does not.
    let mut eh_data: Vec<u8> = Vec::new();
    let mut eh_relocs: Vec<MachRel> = Vec::new();
    let mut eh_patches: Vec<(u32, u64, u8)> = Vec::new();
    {
        let mut cie_off: HashMap<usize, u32> = HashMap::new();
        for &(r, off) in &eh_records {
            if let EhRec::Cie(c) = r {
                cie_off.insert(c, off);
            }
        }
        for &(r, off) in &eh_records {
            debug_assert_eq!(off as usize, eh_data.len());
            match r {
                EhRec::Cie(c) => {
                    let cie = &ctx.cies[c];
                    eh_data.extend_from_slice(cie.data);
                    if let Some(p) = cie.personality {
                        // ld-prime writes 4 into the cell on either
                        // target, whatever the object held there (a
                        // compiler's x86-64 CIE holds 4 too).
                        let at = (off + cie.personality_offset) as usize;
                        eh_data[at..at + 4].copy_from_slice(&4u32.to_le_bytes());
                        let Some(&symnum) = index_of_sym.get(&p) else {
                            fatal!("-r: unwind personality lost: {}", ctx.symbols[p]);
                        };
                        eh_relocs.push(MachRel {
                            r_address: off + cie.personality_offset,
                            bits: symnum
                                | (1 << 24)
                                | (2 << 25)
                                | (1 << 27)
                                | ((E::RELOC_GOTPC as u32) << 28),
                        });
                    }
                }
                EhRec::Fde(f) => {
                    let fde = &ctx.fdes[f];
                    eh_data.extend_from_slice(fde.data);
                    let o = off as usize;
                    // The CIE pointer: how far back the CIE is from
                    // this field.
                    let cie_delta = (off + 4).wrapping_sub(cie_off[&(fde.cie as usize)]);
                    eh_data[o + 4..o + 8].copy_from_slice(&cie_delta.to_le_bytes());
                    // pc_begin: the function, relative to the field.
                    let cie = &ctx.cies[fde.cie as usize];
                    let func_isec = ctx.resolve_isec(fde.isec as usize);
                    let isec = &ctx.isecs[func_isec];
                    let func_addr = ctx.chunk_header(isec.output_section().unwrap()).addr
                        + isec.offset as u64
                        + fde.func_offset as u64;
                    eh_patches.push((off + 8, func_addr, cie.pc_size() as u8));
                    // The LSDA pointer, past the augmentation length.
                    if let Some((lsda, lsda_off)) = fde.lsda {
                        let pos = crate::chunks::eh_frame::lsda_pos(fde.data, cie.pc_size());
                        let size = cie.lsda_size() as u8;
                        let lsda = ctx.resolve_isec(lsda as usize);
                        let l = &ctx.isecs[lsda];
                        let lsda_addr = ctx.chunk_header(l.output_section().unwrap()).addr
                            + l.offset as u64
                            + lsda_off as u64;
                        eh_patches.push((off + pos as u32, lsda_addr, size));
                    }
                }
            }
        }
    }
    if let Some(slot) = eh_slot {
        debug_assert_eq!(eh_data.len() as u64, extras[slot].size);
        extras[slot].data = eh_data;
        extras[slot].relocs = eh_relocs;
        extras[slot].patches = eh_patches;
    }

    // Regenerate each section's relocations against the merged tables.
    let sect_relocs: Vec<Vec<MachRel>> = section_chunks
        .iter()
        .map(|&chunk_idx| section_relocs(ctx, chunk_idx, &index_of_sym, &atom_target))
        .collect();

    // Auto-link requests are not acted on in a -r link; each distinct
    // one is carried into the output as an LC_LINKER_OPTION command,
    // in first-seen order, for the final link to resolve.
    let mut linker_options: Vec<&Vec<Vec<u8>>> = Vec::new();
    for obj in &ctx.objs {
        if !obj.is_alive {
            continue;
        }
        for opt in &obj.linker_options {
            if !linker_options.contains(&opt) {
                linker_options.push(opt);
            }
        }
    }
    // cmd, cmdsize, count, then the NUL-terminated strings, padded to 8.
    let linker_option_cmdsize = |opt: &Vec<Vec<u8>>| -> usize {
        align_to(12 + opt.iter().map(|s| s.len() + 1).sum::<usize>() as u64, 8) as usize
    };

    let loh = optimization_hints(ctx);

    // File layout: header, one segment command with all sections, the
    // symtab, build version, data in code, linker option and hint
    // commands; then section contents, relocations, data in code,
    // hints, symbols and strings.
    let args = &ctx.args;
    let version_cmd = if crate::chunks::has_version_cmd(args) {
        crate::chunks::create_version_cmd::<E>(
            args.platform,
            args.platform_minos,
            args.platform_sdk,
        )
    } else {
        Vec::new()
    };
    let ncmds =
        3 + u32::from(!version_cmd.is_empty()) + linker_options.len() as u32 + loh.is_some() as u32;
    let num_sections = sects.len();
    let seg_cmd_size = size_of::<SegmentCommand>() + num_sections * size_of::<MachSection>();
    let sizeofcmds = seg_cmd_size
        + size_of::<SymtabCommand>()
        + version_cmd.len()
        + size_of::<LinkEditDataCommand>()
        + linker_options.iter().map(|o| linker_option_cmdsize(o)).sum::<usize>()
        + if loh.is_some() { size_of::<LinkEditDataCommand>() } else { 0 };
    // ld-prime leaves -headerpad (32 unless given) free after the load
    // commands, and more when LC_VERSION_MIN_MACOSX, or no command at
    // all, stands where its estimate of them counted a 32-byte
    // LC_BUILD_VERSION.
    let pad = ctx.args.headerpad + 32u64.saturating_sub(version_cmd.len() as u64);
    let mut off = (size_of::<MachHeader>() + sizeofcmds) as u64 + pad;

    // File offsets mirror addresses, except that the address span of a
    // zero-fill section (with the padding up to the next section) has
    // no file bytes: as in ld64's output, __bss can sit before
    // __LD,__compact_unwind without leaving a hole in the file.
    let seg_fileoff = off;
    let mut sect_offsets = Vec::new();
    let mut zerofill_start: Option<u64> = None;
    let mut skipped: u64 = 0;
    for &s in &sects {
        let (addr, size, zerofill) = match s {
            Sect::Chunk(i) => {
                let h = &ctx.output_section(i).hdr;
                (h.addr, h.size, h.is_zerofill())
            }
            Sect::Extra(i) => (extras[i].addr, extras[i].size, false),
        };
        if zerofill {
            zerofill_start.get_or_insert(addr);
            if let Sect::Chunk(_) = s {
                sect_offsets.push(0u64);
            }
            continue;
        }
        if let Some(start) = zerofill_start.take() {
            skipped += addr - start;
        }
        let fileoff = seg_fileoff + addr - skipped;
        match s {
            Sect::Chunk(i) => {
                ctx.output_sections[i.index()].hdr.fileoff = fileoff;
                sect_offsets.push(fileoff);
            }
            Sect::Extra(i) => extras[i].fileoff = fileoff,
        }
        off = fileoff + size;
    }
    for extra in &mut extras {
        // Self-relative cells can be resolved now the address is set.
        for &(cell, target, size) in &extra.patches {
            let val = target.wrapping_sub(extra.addr + cell as u64);
            let cell = cell as usize;
            match size {
                4 => extra.data[cell..cell + 4].copy_from_slice(&(val as u32).to_le_bytes()),
                8 => extra.data[cell..cell + 8].copy_from_slice(&val.to_le_bytes()),
                _ => unreachable!(),
            }
        }
    }
    let content_end = off;
    off = align_to(off, 8);
    let mut reloff = Vec::new();
    for rels in &sect_relocs {
        reloff.push(off);
        off += (rels.len() * size_of::<MachRel>()) as u64;
    }
    for extra in &mut extras {
        extra.reloff = off;
        off += (extra.relocs.len() * size_of::<MachRel>()) as u64;
    }
    // LC_DATA_IN_CODE, between the relocations and the symbol table
    // and present even with no entries (ld-prime): the inputs' entries
    // at their merged addresses, which is what an object's entries
    // hold rather than file offsets.
    let mut dice: Vec<(u32, u16, u16)> = crate::chunks::data_in_code::live_entries(ctx)
        .map(|(isec, off_in, len, kind)| {
            let addr =
                ctx.chunk_header(isec.output_section().unwrap()).addr + isec.offset as u64 + off_in;
            (addr as u32, len, kind)
        })
        .collect();
    dice.sort_unstable();
    let diceoff = off;
    off += dice.len() as u64 * 8;
    let lohoff = off;
    off += loh.as_ref().map_or(0, |l| l.len() as u64);
    let symoff = off;
    off += (nlists_out.len() * size_of::<NList>()) as u64;
    let stroff = off;
    off += strtab.len() as u64;

    let mut buf = vec![0u8; off as usize];

    // Mach header
    let hdr = MachHeader {
        magic: MH_MAGIC_64,
        cputype: E::CPUTYPE,
        cpusubtype: E::CPUSUBTYPE,
        filetype: MH_OBJECT,
        ncmds,
        sizeofcmds: sizeofcmds as u32,
        // Only if every input had it: one whole-section object makes
        // the output whole-section too (ld64).
        flags: if ctx
            .objs
            .iter()
            .enumerate()
            .filter(|(i, o)| o.is_alive && !ctx.is_internal(*i))
            .all(|(_, o)| o.subsections_via_symbols)
        {
            MH_SUBSECTIONS_VIA_SYMBOLS
        } else {
            0
        },
        reserved: 0,
    };
    hdr.write_to(&mut buf);
    let mut p = size_of::<MachHeader>();

    // The single nameless segment
    let seg = SegmentCommand {
        cmd: LC_SEGMENT_64,
        cmdsize: seg_cmd_size as u32,
        segname: [0; 16],
        vmaddr: 0,
        vmsize,
        fileoff: seg_fileoff,
        filesize: content_end - seg_fileoff,
        maxprot: VM_PROT_READ | VM_PROT_WRITE | VM_PROT_EXECUTE,
        initprot: VM_PROT_READ | VM_PROT_WRITE | VM_PROT_EXECUTE,
        nsects: num_sections as u32,
        flags: 0,
    };
    seg.write_to(&mut buf[p..]);
    p += size_of::<SegmentCommand>();

    let mut ci = 0;
    for &s in &sects {
        let sect = match s {
            Sect::Chunk(chunk_idx) => {
                let i = ci;
                ci += 1;
                let chunk = ctx.output_section(chunk_idx);
                MachSection {
                    sectname: str_to_name(&chunk.hdr.sectname),
                    segname: str_to_name(chunk.hdr.segname),
                    addr: chunk.hdr.addr,
                    size: chunk.hdr.size,
                    offset: sect_offsets[i] as u32,
                    p2align: chunk.hdr.p2align,
                    reloff: if sect_relocs[i].is_empty() { 0 } else { reloff[i] as u32 },
                    nreloc: sect_relocs[i].len() as u32,
                    flags: chunk.hdr.flags,
                    reserved1: 0,
                    reserved2: 0,
                    reserved3: 0,
                }
            }
            Sect::Extra(e) => {
                let extra = &extras[e];
                MachSection {
                    sectname: str_to_name(extra.sectname),
                    segname: str_to_name(extra.segname),
                    addr: extra.addr,
                    size: extra.data.len() as u64,
                    offset: extra.fileoff as u32,
                    p2align: extra.p2align as u32,
                    reloff: if extra.relocs.is_empty() { 0 } else { extra.reloff as u32 },
                    nreloc: extra.relocs.len() as u32,
                    flags: extra.flags,
                    reserved1: 0,
                    reserved2: 0,
                    reserved3: 0,
                }
            }
        };
        sect.write_to(&mut buf[p..]);
        p += size_of::<MachSection>();
    }

    // ld64's order: the symbol table, the build version, data in
    // code, then the carried auto-link options and hints. A -r output
    // has no LC_DYSYMTAB (ld-prime writes none).
    let st = SymtabCommand {
        cmd: LC_SYMTAB,
        cmdsize: size_of::<SymtabCommand>() as u32,
        symoff: symoff as u32,
        nsyms: nlists_out.len() as u32,
        stroff: stroff as u32,
        strsize: strtab.len() as u32,
    };
    st.write_to(&mut buf[p..]);
    p += size_of::<SymtabCommand>();

    buf[p..p + version_cmd.len()].copy_from_slice(&version_cmd);
    p += version_cmd.len();

    let dc = LinkEditDataCommand {
        cmd: LC_DATA_IN_CODE,
        cmdsize: size_of::<LinkEditDataCommand>() as u32,
        dataoff: diceoff as u32,
        datasize: (dice.len() * 8) as u32,
    };
    dc.write_to(&mut buf[p..]);
    p += size_of::<LinkEditDataCommand>();
    for (i, &(o, len, kind)) in dice.iter().enumerate() {
        let q = diceoff as usize + i * 8;
        buf[q..q + 4].copy_from_slice(&o.to_le_bytes());
        buf[q + 4..q + 6].copy_from_slice(&len.to_le_bytes());
        buf[q + 6..q + 8].copy_from_slice(&kind.to_le_bytes());
    }

    for opt in &linker_options {
        let cmdsize = linker_option_cmdsize(opt);
        buf[p..p + 4].copy_from_slice(&LC_LINKER_OPTION.to_le_bytes());
        buf[p + 4..p + 8].copy_from_slice(&(cmdsize as u32).to_le_bytes());
        buf[p + 8..p + 12].copy_from_slice(&(opt.len() as u32).to_le_bytes());
        let mut q = p + 12;
        for s in opt.iter() {
            buf[q..q + s.len()].copy_from_slice(s);
            q += s.len() + 1;
        }
        p += cmdsize;
    }

    if let Some(loh) = &loh {
        let cmd = LinkEditDataCommand {
            cmd: LC_LINKER_OPTIMIZATION_HINT,
            cmdsize: size_of::<LinkEditDataCommand>() as u32,
            dataoff: lohoff as u32,
            datasize: loh.len() as u32,
        };
        cmd.write_to(&mut buf[p..]);
        buf[lohoff as usize..lohoff as usize + loh.len()].copy_from_slice(loh);
    }

    // Section contents: raw copies, with non-external targets' embedded
    // addresses rewritten into the merged address space.
    for (i, &chunk_idx) in section_chunks.iter().enumerate() {
        let isecs = &ctx.output_section(chunk_idx).members;
        if sect_offsets[i] == 0 {
            continue;
        }
        let base = sect_offsets[i] as usize;
        for &id in isecs {
            let isec = &ctx.isecs[id];
            if isec.data().is_empty() {
                continue;
            }
            let dst = base + isec.offset as usize;
            buf[dst..dst + isec.data().len()].copy_from_slice(isec.data());

            for rel in crate::input_files::isec_relocs_of(&ctx.objs, isec) {
                if let Some(cell) = E::RELOCATABLE_GOTPC_CELL
                    && rel.r_type == E::RELOC_GOTPC
                    && rel.is_pcrel
                    && rel.size == 4
                {
                    let loc = dst + rel.offset as usize;
                    buf[loc..loc + 4].copy_from_slice(&cell.to_le_bytes());
                    continue;
                }
                let OutTarget::Section(target, addend) = out_target(ctx, isec, rel, &index_of_sym)
                else {
                    continue;
                };
                let t = &ctx.isecs[target];
                // The addend is negative for a target before its
                // section's start.
                let target_addr = (ctx.chunk_header(t.output_section().unwrap()).addr
                    + t.offset as u64)
                    .wrapping_add_signed(addend);
                let loc = dst + rel.offset as usize;
                if let Some((_, atom_addr)) = atom_target(target, addend) {
                    // Now a relocation against the atom's symbol: the
                    // field holds the addend relative to it, in the
                    // form an object's extern relocation uses.
                    let mut val = (target_addr - atom_addr) as i64;
                    if rel.is_pcrel {
                        val -= E::reloc_bias(rel.r_type);
                    }
                    match rel.size {
                        8 => buf[loc..loc + 8].copy_from_slice(&val.to_le_bytes()),
                        4 => buf[loc..loc + 4].copy_from_slice(&(val as i32).to_le_bytes()),
                        _ => {}
                    }
                    continue;
                }
                if rel.r_type == E::RELOC_UNSIGNED && !rel.is_pcrel {
                    match rel.size {
                        8 => buf[loc..loc + 8].copy_from_slice(&target_addr.to_le_bytes()),
                        4 => buf[loc..loc + 4].copy_from_slice(&(target_addr as u32).to_le_bytes()),
                        _ => {}
                    }
                } else if rel.is_pcrel {
                    // Pcrel non-external fields embed target - (P + 4).
                    let here = ctx.output_section(chunk_idx).hdr.addr
                        + isec.offset as u64
                        + rel.offset as u64;
                    let val = target_addr
                        .wrapping_sub(here + 4)
                        .wrapping_sub(E::reloc_bias(rel.r_type) as u64)
                        as u32;
                    if rel.size == 4 {
                        buf[loc..loc + 4].copy_from_slice(&val.to_le_bytes());
                    }
                } else {
                    error!("-r: unsupported non-external relocation");
                }
            }
        }
    }

    for extra in &extras {
        let fo = extra.fileoff as usize;
        buf[fo..fo + extra.data.len()].copy_from_slice(&extra.data);
        let mut p = extra.reloff as usize;
        for rel in &extra.relocs {
            rel.write_to(&mut buf[p..]);
            p += size_of::<MachRel>();
        }
    }

    // Relocations, symbols, strings
    for (i, rels) in sect_relocs.iter().enumerate() {
        let mut p = reloff[i] as usize;
        for rel in rels {
            rel.write_to(&mut buf[p..]);
            p += size_of::<MachRel>();
        }
    }
    let mut p = symoff as usize;
    for nlist in &nlists_out {
        nlist.write_to(&mut buf[p..]);
        p += size_of::<NList>();
    }
    buf[stroff as usize..stroff as usize + strtab.len()].copy_from_slice(&strtab);

    crate::error::checkpoint();
    output_file::write(&ctx.args.output, &buf);
}

/// A -r output section's relocations, regenerated against the merged
/// tables. ld-prime writes each atom's relocations by descending offset
/// whatever the input's order, keeping a pair - a SUBTRACTOR and its
/// UNSIGNED, an arm64 ADDEND and its PAGE21 or PAGEOFF12 - in order.
fn section_relocs<E: Target>(
    ctx: &Context<E>,
    chunk_idx: OutputSectionId,
    index_of_sym: &HashMap<crate::symbol::SymbolId, u32>,
    atom_target: &impl Fn(usize, i64) -> Option<(u32, u64)>,
) -> Vec<MachRel> {
    let mut rels = Vec::new();
    for &id in &ctx.output_section(chunk_idx).members {
        let isec = &ctx.isecs[id];
        let mut groups: Vec<Vec<MachRel>> = Vec::new();
        let mut open: Vec<MachRel> = Vec::new();
        for rel in crate::input_files::isec_relocs_of(&ctx.objs, isec) {
            let mut group = std::mem::take(&mut open);
            push_reloc(ctx, isec, rel, index_of_sym, atom_target, &mut group);
            if rel.r_type == E::RELOC_SUBTRACTOR {
                open = group;
            } else {
                groups.push(group);
            }
        }
        if !open.is_empty() {
            groups.push(open);
        }
        groups.sort_by_key(|g| std::cmp::Reverse(g[0].r_address));
        rels.extend(groups.into_iter().flatten());
    }
    rels
}

/// Appends the -r relocation entries standing for one input relocation.
fn push_reloc<E: Target>(
    ctx: &Context<E>,
    isec: &crate::input_sections::InputSection,
    rel: &crate::input_sections::Reloc,
    index_of_sym: &HashMap<crate::symbol::SymbolId, u32>,
    atom_target: &impl Fn(usize, i64) -> Option<(u32, u64)>,
    out: &mut Vec<MachRel>,
) {
    let r_address = (isec.offset as u64 + rel.offset as u64) as u32;
    let (symnum, is_extern) = match out_target(ctx, isec, rel, index_of_sym) {
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
        OutTarget::Section(target, addend) => match atom_target(target, addend) {
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
    /// against a label the output drops (see build_symtab).
    Section(usize, i64),
}

fn out_target<E: Target>(
    ctx: &Context<E>,
    isec: &crate::input_sections::InputSection,
    rel: &crate::input_sections::Reloc,
    index_of_sym: &HashMap<crate::symbol::SymbolId, u32>,
) -> OutTarget {
    match rel.target() {
        RelocTarget::Sym(idx) => {
            let sym_id = ctx.objs[isec.file as usize].symbols[idx as usize];
            if let Some(&symnum) = index_of_sym.get(&sym_id) {
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

/// A -r output's symbol and string tables, and where relocations find
/// the atoms ld64 names itself.
struct RSymtab {
    nlists: Vec<NList>,
    strtab: Vec<u8>,
    index_of_sym: HashMap<crate::symbol::SymbolId, u32>,
    /// The linker-named atoms by (subsection, record index): each one's
    /// symbol index and address.
    atoms: HashMap<(usize, u64), (u32, u64)>,
    /// The record size of each output section whose atoms are named; 0
    /// for one record per subsection.
    entsize_of: HashMap<OutputSectionId, u64>,
}

/// Builds a -r output's symbol table as ld-prime lays it out: each
/// object's local symbols in the order of its sections and of their
/// addresses in each (a zerofill section comes by ordinal), then
/// the stabs, opened by an N_SO of their own, then the defined
/// externals and the undefined symbols, each by name. Names at one
/// address go by rank - a private external, a local, a weak definition,
/// an ltmpN label - each rank by descending name.
fn build_symtab<E: Target>(ctx: &Context<E>, section_chunks: &[OutputSectionId]) -> RSymtab {
    const PEXT: u8 = 0;
    const LOCAL: u8 = 1;
    const WEAK: u8 = 2;
    const LTMP: u8 = 3;
    let sym_addr = |ctx: &Context<E>, id: crate::symbol::SymbolId| -> u64 {
        let sym = &ctx.symbols[id];
        match sym.input_section() {
            Some(isec) => {
                let isec = &ctx.isecs[ctx.resolve_isec(isec as usize)];
                ctx.chunk_header(isec.output_section().unwrap()).addr
                    + isec.offset as u64
                    + sym.value
            }
            None => sym.value,
        }
    };
    let mut index_of_sym: HashMap<crate::symbol::SymbolId, u32> = HashMap::new();
    let mut ents: Vec<(NList, Option<crate::symbol::SymbolId>)> = Vec::new();
    let mut names: Vec<&[u8]> = Vec::new();

    // Local symbols, in ld64's form. ld-prime makes the literals -
    // the strings of a cstring-literal section, the records of a
    // fixed-size literal section and the atoms has_unnamed_atoms names
    // (CFStrings, selector and class references, UTF-16 and ObjC
    // constant literals) - by content: their labels vanish, all but
    // those of the cstring and fixed-size literals a symbol names
    // (labeled, kept apart). arm64 relocations must name what they
    // refer to, so there ld64 names each such atom itself, with one
    // shared counter: a cstring literal LC<n>, the others l<nnn>, all
    // with N_PEXT set so that a later link can still coalesce them.
    // x86-64 ones refer to them section-relatively, and only the
    // literals of __TEXT,__cstring get names (LC<n>). The entries of
    // the __objc_*list sections get no symbol, and on x86-64 neither
    // do the class and protocol references only a linker-private label
    // (l...) names. Relocations against the vanished labels - by
    // label, or section-relative as x86-64 objects refer to literals -
    // are re-targeted at the new symbols, or are section-relative.
    // Other labels survive, those of the linker-private kind too,
    // except the ltmpN labels the arm64 assembler puts at each
    // section's start: in an object with subsections one survives
    // only where no other symbol names the place, or where a relocation
    // refers to it. Private externals are demoted to non-external
    // symbols that keep N_PEXT (below).
    #[derive(Clone, Copy, PartialEq)]
    enum Rename {
        None,
        Cstring,
        Anon,
    }
    struct Local {
        name: String,
        n_type: u8,
        n_desc: u16,
        n_sect: u8,
        addr: u64,
        rename: Rename,
        syms: Vec<crate::symbol::SymbolId>,
        /// The object, the section there and the address, which order
        /// the locals.
        at: (u32, u8, u64),
        rank: u8,
    }
    let mut locals: Vec<Local> = Vec::new();

    // The sections of literals: the record size; 0 for one record per
    // subsection, as cstring literals are split.
    let literal_size = |isec: usize, flags: u32| -> Option<u64> {
        match flags & SECTION_TYPE {
            S_CSTRING_LITERALS => return Some(0),
            S_4BYTE_LITERALS => return Some(4),
            S_8BYTE_LITERALS => return Some(8),
            S_16BYTE_LITERALS => return Some(16),
            _ => {}
        }
        let h = ctx.hdr_of(&ctx.isecs[isec]);
        if !crate::passes::has_unnamed_atoms(h) {
            return None;
        }
        match h.sectname() {
            "__cfstring" => Some(32),
            "__objc_selrefs" | "__objc_classrefs" => Some(8),
            _ => Some(0),
        }
    };
    let names_literals = |segname: &str, sectname: &str| {
        E::CPUTYPE == CPU_TYPE_ARM64 || (segname == "__TEXT" && sectname == "__cstring")
    };
    // The entries of the __objc_*list sections get no symbol at all
    // (ld-prime): their labels vanish.
    let unnamed_list = |isec: usize| -> bool {
        let h = ctx.hdr_of(&ctx.isecs[isec]);
        h.segname() == "__DATA"
            && matches!(
                h.sectname(),
                "__objc_classlist"
                    | "__objc_nlclslist"
                    | "__objc_catlist"
                    | "__objc_catlist2"
                    | "__objc_nlcatlist"
            )
    };
    // (subsection, record index) -> entry in `locals`; the record
    // size of each such output section; the literal sections whose
    // atoms get no name.
    let mut renamed: HashMap<(usize, u64), usize> = HashMap::new();
    let mut entsize_of: HashMap<OutputSectionId, u64> = HashMap::new();
    let mut unnamed: HashSet<OutputSectionId> = HashSet::new();
    for &chunk_idx in section_chunks {
        let chunk = ctx.output_section(chunk_idx);
        let Some(&first) = chunk.members.first() else { continue };
        let Some(entsize) = literal_size(first as usize, chunk.hdr.flags) else { continue };
        if !names_literals(chunk.hdr.segname, &chunk.hdr.sectname) {
            unnamed.insert(chunk_idx);
            continue;
        }
        entsize_of.insert(chunk_idx, entsize);
        for &id in &chunk.members {
            let id = id as usize;
            let isec = &ctx.isecs[id];
            // A literal a label names keeps that label instead.
            if !isec.is_alive()
                || isec.replacement != crate::input_sections::NO_REPLACEMENT
                || isec.is_labeled()
            {
                continue;
            }
            let size = isec.data().len() as u64;
            if size == 0 {
                continue;
            }
            let n = if entsize == 0 { 1 } else { size.div_ceil(entsize) };
            for k in 0..n {
                renamed.insert((id, k), locals.len());
                locals.push(Local {
                    name: String::new(),
                    n_type: N_PEXT | N_SECT,
                    n_desc: section_desc(ctx, id),
                    n_sect: chunk.hdr.n_sect,
                    addr: chunk.hdr.addr + isec.offset as u64 + k * entsize,
                    rename: if chunk.hdr.flags & SECTION_TYPE == S_CSTRING_LITERALS {
                        Rename::Cstring
                    } else {
                        Rename::Anon
                    },
                    syms: Vec::new(),
                    at: (isec.file, isec.shndx as u8 + 1, isec.input_addr as u64 + k * entsize),
                    rank: LOCAL,
                });
            }
        }
    }
    // The atom a relocation into one of those sections lands in.
    let atom_target = |t: usize, addend: i64| -> Option<usize> {
        let ChunkId::Output(osec) = ctx.isecs[t].output_section()? else {
            return None;
        };
        let entsize = *entsize_of.get(&osec)?;
        let k = (addend as u64).checked_div(entsize).unwrap_or(0);
        renamed.get(&(t, k)).copied()
    };

    // On x86-64 the label of a literal left unnamed vanishes, as does a
    // linker-private (l...) name of a class or protocol reference, a
    // demoted private external's too (Swift's protocol references) -
    // unless a subtraction names it, which has no section-relative form
    // (ld-prime fails an assertion on it).
    let subtracted =
        if E::CPUTYPE == CPU_TYPE_ARM64 { HashSet::new() } else { subtracted_syms(ctx) };
    let vanishes = |isec: usize, sym_id: crate::symbol::SymbolId| -> bool {
        if E::CPUTYPE == CPU_TYPE_ARM64 || subtracted.contains(&sym_id) {
            return false;
        }
        let t = &ctx.isecs[isec];
        if let Some(ChunkId::Output(osec)) = t.output_section()
            && unnamed.contains(&osec)
        {
            return !t.is_labeled();
        }
        let h = ctx.hdr_of(t);
        h.segname_is("__DATA")
            && (h.sectname_is("__objc_superrefs") || h.sectname_is("__objc_protorefs"))
            && ctx.symbols[sym_id].name().starts_with('l')
    };
    let referenced = referenced_syms(ctx);
    let named_at = named_places(ctx);
    for (obj_idx, obj) in ctx.objs.iter().enumerate() {
        if !obj.is_alive {
            continue;
        }
        // Without subsections the object's sections are whole atoms,
        // which ld64 marks no-dead-strip - every symbol, the
        // assembler's ltmpN labels (they name the atoms) included -
        // and an alt entry means nothing there.
        let whole = !obj.subsections_via_symbols;
        let r = obj.local_range();
        for (nlist, &sym_id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
            if nlist.is_stab() || nlist.is_extern() {
                continue;
            }
            let sym = &ctx.symbols[sym_id];
            let Some(input) = sym.input_section().map(|i| i as usize) else { continue };
            let isec = ctx.resolve_isec(input);
            if !ctx.isecs[isec].is_alive() || sym.name().is_empty() {
                continue;
            }
            // A label on an atom ld64 names itself stands for that atom.
            if let Some(e) = atom_target(isec, sym.value as i64) {
                locals[e].syms.push(sym_id);
                continue;
            }
            if (unnamed_list(isec) && !referenced.contains(&sym_id)) || vanishes(isec, sym_id) {
                continue;
            }
            if sym.name().starts_with("ltmp")
                && !whole
                && !referenced.contains(&sym_id)
                && named_at.contains(&(obj_idx, nlist.n_sect, nlist.n_value))
            {
                continue;
            }
            // A labeled literal is an atom of its own, not a whole
            // section's.
            let whole = whole && !ctx.isecs[isec].is_labeled();
            locals.push(Local {
                name: sym.name().to_string(),
                n_type: nlist.n_type,
                n_desc: whole_desc(nlist.n_desc, whole) | section_desc(ctx, input),
                n_sect: ctx.isec_n_sect(&ctx.isecs[isec]),
                addr: sym_addr(ctx, sym_id),
                rename: Rename::None,
                syms: vec![sym_id],
                at: (obj_idx as u32, nlist.n_sect, nlist.n_value),
                rank: if sym.name().starts_with("ltmp") { LTMP } else { LOCAL },
            });
        }
    }
    // Private externals (visibility hidden) become non-external
    // symbols in a -r output, as in ld64 - N_PEXT still set, which nm
    // reports as "was a private external" - unless
    // -keep_private_externs (which Apple's strip passes to the `ld -r`
    // it runs on each archive member).
    let keep_pext = ctx.args.keep_private_externs;
    if !keep_pext {
        for (obj_idx, obj) in ctx.objs.iter().enumerate() {
            if !obj.is_alive {
                continue;
            }
            let r = obj.global_range();
            for (nlist, &sym_id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
                let sym = &ctx.symbols[sym_id];
                // Only the copy that won resolution is emitted.
                if nlist.is_stab()
                    || !nlist.is_extern()
                    || !sym.is_private_extern()
                    || !matches!(sym.file(), Some(FileId::Obj(o)) if o as usize == obj_idx)
                {
                    continue;
                }
                let Some(input) = sym.input_section().map(|i| i as usize) else { continue };
                let isec = ctx.resolve_isec(input);
                if !ctx.isecs[isec].is_alive() || vanishes(isec, sym_id) {
                    continue;
                }
                locals.push(Local {
                    name: sym.name().to_string(),
                    n_type: N_PEXT | N_SECT,
                    n_desc: whole_desc(
                        nlist.n_desc & (N_ALT_ENTRY | N_NO_DEAD_STRIP | N_WEAK_DEF),
                        !obj.subsections_via_symbols,
                    ) | section_desc(ctx, input),
                    n_sect: ctx.isec_n_sect(&ctx.isecs[isec]),
                    addr: sym_addr(ctx, sym_id),
                    rename: Rename::None,
                    syms: vec![sym_id],
                    at: (obj_idx as u32, nlist.n_sect, nlist.n_value),
                    rank: if sym.is_weak_def() { WEAK } else { PEXT },
                });
            }
        }
    }
    // The locals in order, the linker-named atoms numbered in it with
    // one counter.
    let mut order: Vec<usize> = (0..locals.len()).collect();
    order.sort_by(|&a, &b| {
        let (a, b) = (&locals[a], &locals[b]);
        a.at.cmp(&b.at).then(a.rank.cmp(&b.rank)).then(b.name.cmp(&a.name))
    });
    let mut counter = 1u32;
    for &i in &order {
        let l = &mut locals[i];
        match l.rename {
            Rename::None => {}
            Rename::Cstring => {
                l.name = format!("LC{counter}");
                counter += 1;
            }
            Rename::Anon => {
                l.name = format!("l{counter:03}");
                counter += 1;
            }
        }
    }
    let mut symnum_of = vec![0u32; locals.len()];
    let externals = external_places(ctx);
    let mut prev_at = None;
    for &i in &order {
        let l = &locals[i];
        symnum_of[i] = ents.len() as u32;
        for &sym_id in &l.syms {
            index_of_sym.insert(sym_id, ents.len() as u32);
        }
        names.push(l.name.as_bytes());
        // The first name at a place names the atom, unless an external
        // there does; the others are its aliases.
        let alias = prev_at == Some(l.at) || externals.contains(&l.at);
        prev_at = Some(l.at);
        // (A name ld64 makes for a literal record has no symbol.)
        let n_desc = if alias && l.syms.first().is_some_and(|&s| in_init_term_list(ctx, s)) {
            l.n_desc & !N_NO_DEAD_STRIP
        } else {
            l.n_desc
        };
        let ent = NList { n_strx: 0, n_type: l.n_type, n_sect: l.n_sect, n_desc, n_value: l.addr };
        ents.push((ent, l.syms.first().copied()));
    }
    let nplain = ents.len();

    // Debug-note stabs: ld64 does not merge the inputs' DWARF into a -r
    // output, it names the objects that hold it (N_OSO) and where their
    // symbols landed, and a later link carries the notes through.
    let mut names_of: Vec<Option<crate::symbol::SymbolId>> = Vec::new();
    if !ctx.args.strip_debug {
        let cwd = std::env::current_dir().unwrap_or_default();
        let commons = crate::passes::common_stab_owners(ctx);
        let plans: Vec<crate::passes::StabPlan> = (0..ctx.objs.len())
            .map(|obj_idx| crate::passes::plan_object_stabs(ctx, obj_idx, &cwd, &commons))
            .collect();
        let stabs: Vec<crate::passes::Stab> = plans.iter().flat_map(|p| p.stabs(ctx)).collect();
        if !stabs.is_empty() {
            names.push(b"");
            ents.push((NList { n_strx: 1, n_type: N_SO, n_sect: 1, ..Default::default() }, None));
            names_of.push(None);
        }
        for stab in stabs {
            let mut ent = stab.ent;
            if let Some(id) = stab.value_of {
                ent.n_value = sym_addr(ctx, id);
            }
            names.push(stab.name);
            ents.push((ent, None));
            names_of.push(stab.name_of);
        }
    }
    let nlocal = ents.len();

    // The n_desc flags a defined global carries in its object, which the
    // next link needs as much as this one did. N_ALT_ENTRY is the
    // critical one: it marks a symbol that does not begin a new
    // subsection (Swift's class metadata symbol $s..CN is an alt entry
    // into the full-metadata object $s..CMf, referenced as CMf+0x18),
    // and a link that splits there re-aligns the tail and moves the
    // symbol away from every non-symbolic reference to it.
    let mut desc_of: HashMap<crate::symbol::SymbolId, u16> = HashMap::new();
    for (obj_idx, obj) in ctx.objs.iter().enumerate() {
        if !obj.is_alive {
            continue;
        }
        let r = obj.global_range();
        for (nlist, &sym_id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
            if !nlist.is_stab()
                && nlist.is_extern()
                && nlist.n_type() != N_UNDF
                && matches!(ctx.symbols[sym_id].file(), Some(FileId::Obj(o)) if o as usize == obj_idx)
            {
                desc_of.insert(sym_id, nlist.n_desc);
            }
        }
    }

    // Defined externals, sorted by name.
    let mut globals: Vec<usize> = (0..ctx.symbols.syms.len())
        .filter(|&i| {
            let sym = &ctx.symbols[i];
            sym.is_extern()
                && (keep_pext || !sym.is_private_extern())
                && matches!(sym.file(), Some(FileId::Obj(_)))
                && sym
                    .input_section()
                    .is_none_or(|isec| ctx.isecs[ctx.resolve_isec(isec as usize)].is_alive())
        })
        .collect();
    globals.sort_by_key(|&i| ctx.symbols[i].name());
    for &i in &globals {
        let sym = &ctx.symbols[i];
        let (n_type, n_sect) = match sym.input_section() {
            Some(isec) => (
                N_SECT | N_EXT | if sym.is_private_extern() { N_PEXT } else { 0 },
                ctx.isec_n_sect(&ctx.isecs[ctx.resolve_isec(isec as usize)]),
            ),
            None => (N_ABS | N_EXT, 0),
        };
        // N_WEAK_REF on a definition is .weak_def_can_be_hidden: with
        // N_WEAK_DEF it lets a final link auto-hide the symbol (ld-prime
        // makes PLCrashReporter's template instantiations local; ours
        // stayed exported after the -r prelink lost the marker).
        let mut n_desc = desc_of.get(&(i as u32)).copied().unwrap_or(0)
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
        if let Some(input) = sym.input_section() {
            n_desc |= section_desc(ctx, input as usize);
        }
        index_of_sym.insert(i as u32, ents.len() as u32);
        names.push(sym.name().as_bytes());
        let n_value = sym_addr(ctx, i as u32);
        ents.push((NList { n_strx: 0, n_type, n_sect, n_desc, n_value }, Some(i as u32)));
    }

    // Undefined and tentative symbols, sorted by name.
    let mut undefs: Vec<usize> = (0..ctx.symbols.syms.len())
        .filter(|&i| {
            let sym = &ctx.symbols[i];
            sym.is_used() && (!sym.is_defined() || sym.is_common())
        })
        .collect();
    undefs.sort_by_key(|&i| ctx.symbols[i].name());
    for &i in &undefs {
        let sym = &ctx.symbols[i];
        let mut n_desc = 0;
        let mut n_value = 0;
        if sym.is_common() {
            n_value = sym.value;
            n_desc |= (sym.common_p2align as u16) << 8;
        } else if sym.is_weak_ref() {
            n_desc |= N_WEAK_REF;
        }
        index_of_sym.insert(i as u32, ents.len() as u32);
        names.push(sym.name().as_bytes());
        let ent = NList { n_strx: 0, n_type: N_UNDF | N_EXT, n_sect: 0, n_desc, n_value };
        ents.push((ent, Some(i as u32)));
    }
    let entry_of =
        crate::chunks::symtab::symbol_entries(&ents, nplain, nlocal, ctx.symbols.syms.len());
    let size = crate::chunks::symtab::layout_strings(
        &mut ents,
        &mut names,
        nlocal,
        (nplain, &names_of),
        &entry_of,
    )
    .next_multiple_of(8);
    let mut strtab = vec![0u8; size];
    strtab[0] = b' ';
    for ((ent, _), name) in ents.iter().zip(&names) {
        let off = ent.n_strx as usize;
        strtab[off..off + name.len()].copy_from_slice(name);
    }

    let mut atoms = HashMap::new();
    for (&key, &e) in &renamed {
        atoms.insert(key, (symnum_of[e], locals[e].addr));
    }
    RSymtab {
        nlists: ents.into_iter().map(|e| e.0).collect(),
        strtab,
        index_of_sym,
        atoms,
        entsize_of,
    }
}
