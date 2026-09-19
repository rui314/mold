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

use std::collections::{HashMap, HashSet};

use crate::error;
use crate::fatal;
use crate::macho::arch::Arch;
use crate::macho::context::Context;
use crate::macho::format::*;
use crate::macho::input_files::FileId;
use crate::macho::input_sections::RelocTarget;
use crate::macho::output_chunks::{ChunkId, OutputSectionId};
use crate::macho::output_file;
use crate::util::align_to;

/// ld64's section order in a -r output, measured with ld-prime:
/// segments __TEXT, __DATA_CONST, __DATA, the rest as first seen, __LD
/// last. In __TEXT, __text leads, other code sections (__StaticInit)
/// follow, everything else keeps its first-seen order and __eh_frame
/// closes the segment. __DATA has fixed ranks for the sections ld64
/// knows - the order a final link gives __DATA_CONST, then __DATA -
/// unknown ones in first-seen order after them, then the thread-local
/// template and the zero-fill sections.
fn section_rank(segname: &str, sectname: &str, flags: u32) -> (u32, u32) {
    let seg = match segname {
        "__TEXT" => 0,
        "__DATA_CONST" => 1,
        "__DATA" => 2,
        "__LD" => 4,
        _ => 3,
    };
    let sect = match (segname, sectname) {
        ("__TEXT", "__text") => 0,
        ("__TEXT", "__eh_frame") => 3,
        ("__TEXT", _) if flags & S_ATTR_PURE_INSTRUCTIONS != 0 => 1,
        ("__TEXT", _) => 2,
        ("__DATA", "__got") => 0,
        ("__DATA", "__mod_init_func") => 1,
        ("__DATA", "__mod_term_func") => 2,
        ("__DATA", "__const") => 3,
        ("__DATA", "__cfstring") => 4,
        ("__DATA", "__objc_classlist") => 5,
        ("__DATA", "__objc_nlclslist") => 6,
        ("__DATA", "__objc_catlist") => 7,
        ("__DATA", "__objc_nlcatlist") => 8,
        ("__DATA", "__objc_protolist") => 9,
        ("__DATA", "__objc_imageinfo") => 10,
        ("__DATA", "__objc_const") => 11,
        ("__DATA", "__objc_selrefs") => 12,
        ("__DATA", "__objc_protorefs") => 13,
        ("__DATA", "__objc_classrefs") => 14,
        ("__DATA", "__objc_superrefs") => 15,
        ("__DATA", "__objc_ivar") => 16,
        ("__DATA", "__objc_data") => 17,
        ("__DATA", _) => match flags & SECTION_TYPE {
            S_THREAD_LOCAL_REGULAR => 19,
            S_THREAD_LOCAL_ZEROFILL => 20,
            S_ZEROFILL => 21,
            _ => 18,
        },
        _ => 0,
    };
    (seg, sect)
}

pub fn link<E: Arch>(ctx: &mut Context<E>) {
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
    // links it. ld64 writes it into __DATA in a -r output.
    if ctx.objs.iter().any(|o| o.is_alive && o.objc_image_info.is_some()) {
        let mut e = new_extra("__DATA", "__objc_imageinfo", 0, 2, 8);
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
                && isec.replacement == crate::macho::input_sections::NO_REPLACEMENT
                && (rec.fde().is_none() || rec.encoding & UNWIND_MODE_MASK == E::UNWIND_MODE_DWARF)
        })
        .collect();
    let mut cu_slot = None;
    if !cu_kept.is_empty() {
        cu_slot = Some(extras.len());
        extras.push(new_extra(
            "__LD",
            "__compact_unwind",
            S_ATTR_DEBUG,
            3,
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
            if !isec.is_alive() || isec.replacement != crate::macho::input_sections::NO_REPLACEMENT
            {
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
        extras.push(new_extra("__TEXT", "__eh_frame", 0, 3, size));
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
    sects.sort_by_key(|s| match *s {
        Sect::Chunk(i) => {
            let h = &ctx.output_section(i).hdr;
            section_rank(h.segname, &h.sectname, h.flags)
        }
        Sect::Extra(i) => section_rank(extras[i].segname, extras[i].sectname, extras[i].flags),
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

    // The output symbol table: locals per object, then defined
    // externals, then undefineds, with an index map for relocations.
    let mut strtab: Vec<u8> = vec![b' ', 0];
    let add_string = |strtab: &mut Vec<u8>, s: &str| -> u32 {
        let off = strtab.len() as u32;
        strtab.extend_from_slice(s.as_bytes());
        strtab.push(0);
        off
    };

    // Section ordinals are 1-based positions among the emitted
    // sections, synthetic ones included.
    let mut extra_ordinals = vec![0u8; extras.len()];
    for (i, &s) in sects.iter().enumerate() {
        match s {
            Sect::Chunk(id) => ctx.output_section_mut(id).hdr.n_sect = i as u8 + 1,
            Sect::Extra(e) => extra_ordinals[e] = i as u8 + 1,
        }
    }
    let mut nlists_out: Vec<NList> = Vec::new();
    let mut index_of_sym: HashMap<crate::macho::symbol::SymbolId, u32> = HashMap::new();

    let sym_addr = |ctx: &Context<E>, id: crate::macho::symbol::SymbolId| -> u64 {
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

    // Debug-note stabs first, as in a final link: ld64 does not merge
    // the inputs' DWARF into a -r output, it names the objects that
    // hold it (N_OSO) and where their symbols landed, and a later link
    // carries the notes through.
    if !ctx.args.strip_debug {
        let cwd =
            std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        for obj_idx in 0..ctx.objs.len() {
            for (name, mut ent, sym) in crate::macho::passes::plan_object_stabs(ctx, obj_idx, &cwd)
            {
                if let Some(id) = sym {
                    ent.n_value = sym_addr(ctx, id);
                }
                ent.n_strx = add_string(&mut strtab, name);
                nlists_out.push(ent);
            }
        }
    }

    // Local symbols, in ld64's form. ld64 -r emits them in atom order
    // (by address), and names some atoms itself with one shared
    // counter: every string in a cstring-literal section becomes LC<n>
    // (N_PEXT set, so a later link can still coalesce it), and the
    // anonymous records of __cfstring, __objc_selrefs and
    // __objc_classrefs (N_PEXT) and of the __objc_*list sections (no
    // N_PEXT) become l<nnn>. Their original labels vanish, and
    // relocations against them - by label, or section-relative as
    // x86-64 objects refer to literals - are re-targeted at the new
    // symbols. Otherwise assembler-local labels (L...) and
    // linker-private ones (l...) survive only when they name an atom
    // of their own: one no other symbol names, or that a relocation
    // refers to (the arm64 assembler names every section-relative
    // target ltmpN). Dropping the rest is the bulk of what made our
    // -r symbol tables larger than ld-prime's (7739 vs 6540 symbols
    // for NetNewsWire's RSCore.o). Private externals are demoted to
    // non-external symbols that keep N_PEXT (below).
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
        syms: Vec<crate::macho::symbol::SymbolId>,
    }
    let mut locals: Vec<Local> = Vec::new();

    // The sections whose atoms ld64 names itself: (N_PEXT, record
    // size; 0 for one record per subsection, as cstring literals are
    // split).
    let rename_kind = |flags: u32, segname: &str, sectname: &str| -> Option<(bool, u64)> {
        if flags & SECTION_TYPE == S_CSTRING_LITERALS {
            return Some((true, 0));
        }
        match (segname, sectname) {
            ("__DATA", "__cfstring") => Some((true, 32)),
            ("__DATA", "__objc_selrefs") | ("__DATA", "__objc_classrefs") => Some((true, 8)),
            ("__DATA", "__objc_classlist")
            | ("__DATA", "__objc_nlclslist")
            | ("__DATA", "__objc_catlist")
            | ("__DATA", "__objc_nlcatlist") => Some((false, 8)),
            _ => None,
        }
    };
    // (subsection, record index) -> entry in `locals`; and the record
    // size of each such output section.
    let mut renamed: HashMap<(usize, u64), usize> = HashMap::new();
    let mut entsize_of: HashMap<OutputSectionId, u64> = HashMap::new();
    for &chunk_idx in &section_chunks {
        let chunk = ctx.output_section(chunk_idx);
        let Some((pext, entsize)) =
            rename_kind(chunk.hdr.flags, chunk.hdr.segname, &chunk.hdr.sectname)
        else {
            continue;
        };
        entsize_of.insert(chunk_idx, entsize);
        for &id in &chunk.members {
            let id = id as usize;
            let isec = &ctx.isecs[id];
            if !isec.is_alive() || isec.replacement != crate::macho::input_sections::NO_REPLACEMENT
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
                    n_type: if pext { N_PEXT | N_SECT } else { N_SECT },
                    n_desc: 0,
                    n_sect: chunk.hdr.n_sect,
                    addr: chunk.hdr.addr + isec.offset as u64 + k * entsize,
                    rename: if entsize == 0 { Rename::Cstring } else { Rename::Anon },
                    syms: Vec::new(),
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

    let mut referenced: HashSet<crate::macho::symbol::SymbolId> = HashSet::new();
    for isec in ctx.isecs.iter() {
        if !isec.is_alive() || ctx.is_internal(isec.file as usize) {
            continue;
        }
        for rel in crate::macho::input_files::isec_relocs_of(&ctx.objs, isec) {
            if let RelocTarget::Sym(idx) = rel.target() {
                referenced.insert(ctx.objs[isec.file as usize].symbols[idx as usize]);
            }
        }
    }
    // How many symbols other than assembler temporaries (ltmpN) an
    // object defines at each place, so a label there is recognized as
    // an alias of a real name.
    let mut named_at: HashMap<(usize, u8, u64), u32> = HashMap::new();
    for (obj_idx, obj) in ctx.objs.iter().enumerate() {
        if !obj.is_alive {
            continue;
        }
        for (nlist, &sym_id) in obj.nlists.iter().zip(&obj.symbols) {
            if nlist.is_stab() || nlist.n_type() != N_SECT {
                continue;
            }
            if !ctx.symbols[sym_id].name().starts_with("ltmp") {
                *named_at.entry((obj_idx, nlist.n_sect, nlist.n_value)).or_default() += 1;
            }
        }
    }
    for (obj_idx, obj) in ctx.objs.iter().enumerate() {
        if !obj.is_alive {
            continue;
        }
        let r = obj.local_range();
        for (nlist, &sym_id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
            if nlist.is_stab() || nlist.is_extern() {
                continue;
            }
            let sym = &ctx.symbols[sym_id];
            let Some(isec) = sym.input_section().map(|i| i as usize) else { continue };
            let isec = ctx.resolve_isec(isec);
            if !ctx.isecs[isec].is_alive() || sym.name().is_empty() {
                continue;
            }
            // A label on an atom ld64 names itself stands for that atom.
            if let Some(e) = atom_target(isec, sym.value as i64) {
                locals[e].syms.push(sym_id);
                continue;
            }
            let is_label = sym.name().starts_with('l') || sym.name().starts_with('L');
            if is_label && !referenced.contains(&sym_id) {
                let others =
                    named_at.get(&(obj_idx, nlist.n_sect, nlist.n_value)).copied().unwrap_or(0)
                        - u32::from(!sym.name().starts_with("ltmp"));
                if others > 0 {
                    continue;
                }
                // A label on an empty section names nothing.
                if obj.sect_hdrs.get(nlist.n_sect as usize - 1).is_some_and(|h| h.size == 0) {
                    continue;
                }
            }
            locals.push(Local {
                name: sym.name().to_string(),
                n_type: nlist.n_type,
                n_desc: nlist.n_desc,
                n_sect: ctx.isec_n_sect(&ctx.isecs[isec]),
                addr: sym_addr(ctx, sym_id),
                rename: Rename::None,
                syms: vec![sym_id],
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
                let Some(isec) = sym.input_section().map(|i| i as usize) else { continue };
                let isec = ctx.resolve_isec(isec);
                if !ctx.isecs[isec].is_alive() {
                    continue;
                }
                locals.push(Local {
                    name: sym.name().to_string(),
                    n_type: N_PEXT | N_SECT,
                    n_desc: nlist.n_desc & (N_ALT_ENTRY | N_NO_DEAD_STRIP),
                    n_sect: ctx.isec_n_sect(&ctx.isecs[isec]),
                    addr: sym_addr(ctx, sym_id),
                    rename: Rename::None,
                    syms: vec![sym_id],
                });
            }
        }
    }
    // ld64 names the __eh_frame atoms too: every CIE is EH_Frame1 and
    // every FDE func.eh, plain local symbols the FDEs' relocations
    // (below) are expressed against.
    let mut eh_local: Vec<usize> = Vec::with_capacity(eh_records.len());
    if let Some(slot) = eh_slot {
        for &(r, off) in &eh_records {
            eh_local.push(locals.len());
            locals.push(Local {
                name: match r {
                    EhRec::Cie(_) => "EH_Frame1".to_string(),
                    EhRec::Fde(_) => "func.eh".to_string(),
                },
                n_type: N_SECT,
                n_desc: 0,
                n_sect: extra_ordinals[slot],
                addr: extras[slot].addr + off as u64,
                rename: Rename::None,
                syms: Vec::new(),
            });
        }
    }

    // Atom order, the linker-named atoms numbered in it.
    let mut order: Vec<usize> = (0..locals.len()).collect();
    order.sort_by_key(|&i| locals[i].addr);
    let mut entry_symnum: Vec<u32> = vec![0; locals.len()];
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
        let symnum = nlists_out.len() as u32;
        entry_symnum[i] = symnum;
        for &sym_id in &l.syms {
            index_of_sym.insert(sym_id, symnum);
        }
        nlists_out.push(NList {
            n_strx: add_string(&mut strtab, &l.name),
            n_type: l.n_type,
            n_sect: l.n_sect,
            n_desc: l.n_desc,
            n_value: l.addr,
        });
    }
    let nlocal = nlists_out.len() as u32;

    // The n_desc flags a defined global carries in its object, which the
    // next link needs as much as this one did. N_ALT_ENTRY is the
    // critical one: it marks a symbol that does not begin a new
    // subsection (Swift's class metadata symbol $s..CN is an alt entry
    // into the full-metadata object $s..CMf, referenced as CMf+0x18),
    // and a link that splits there re-aligns the tail and moves the
    // symbol away from every non-symbolic reference to it.
    let mut desc_of: HashMap<crate::macho::symbol::SymbolId, u16> = HashMap::new();
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
        index_of_sym.insert(i as u32, nlists_out.len() as u32);
        nlists_out.push(NList {
            n_strx: add_string(&mut strtab, sym.name()),
            n_type,
            n_sect,
            n_desc,
            n_value: sym_addr(ctx, i as u32),
        });
    }
    let nextdef = nlists_out.len() as u32 - nlocal;

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
        index_of_sym.insert(i as u32, nlists_out.len() as u32);
        nlists_out.push(NList {
            n_strx: add_string(&mut strtab, sym.name()),
            n_type: N_UNDF | N_EXT,
            n_sect: 0,
            n_desc,
            n_value,
        });
    }
    let nundef = nlists_out.len() as u32 - nlocal - nextdef;
    while !strtab.len().is_multiple_of(8) {
        strtab.push(0);
    }

    // Re-synthesize __LD,__compact_unwind so unwind info survives the
    // merge: one 32-byte entry per record, its pointer fields set by
    // UNSIGNED relocations. ld64 names the function and the LSDA by
    // symbol when they have one (an extern relocation with a zero
    // field); a nameless one is referred to section-relatively.
    let mut sym_at: HashMap<(usize, u64), u32> = HashMap::new();
    for (&sym_id, &symnum) in &index_of_sym {
        let sym = &ctx.symbols[sym_id];
        if let Some(isec) = sym.input_section() {
            sym_at.entry((ctx.resolve_isec(isec as usize), sym.value)).or_insert(symnum);
        }
    }
    let mut cu_data: Vec<u8> = Vec::new();
    let mut cu_relocs: Vec<MachRel> = Vec::new();
    for &r in &cu_kept {
        let rec = &ctx.unwind_records[r];
        let isec = &ctx.isecs[rec.isec as usize];
        let entry = cu_data.len() as u32;
        match sym_at.get(&(rec.isec as usize, rec.input_offset as u64)) {
            Some(&symnum) => {
                cu_data.extend_from_slice(&0u64.to_le_bytes());
                cu_relocs.push(MachRel { r_address: entry, bits: symnum | (3 << 25) | (1 << 27) });
            }
            None => {
                let func_addr = ctx.chunk_header(isec.output_section().unwrap()).addr
                    + isec.offset as u64
                    + rec.input_offset as u64;
                cu_data.extend_from_slice(&func_addr.to_le_bytes());
                cu_relocs.push(MachRel {
                    r_address: entry,
                    bits: ctx.isec_n_sect(isec) as u32 | (3 << 25),
                });
            }
        }
        cu_data.extend_from_slice(&rec.code_len.to_le_bytes());
        cu_data.extend_from_slice(&rec.encoding.to_le_bytes());

        match rec.personality() {
            Some(p) => {
                let Some(&symnum) = index_of_sym.get(&p) else {
                    fatal!("-r: unwind personality lost: {}", ctx.symbols[p]);
                };
                cu_data.extend_from_slice(&0u64.to_le_bytes());
                cu_relocs
                    .push(MachRel { r_address: entry + 16, bits: symnum | (3 << 25) | (1 << 27) });
            }
            None => cu_data.extend_from_slice(&0u64.to_le_bytes()),
        }

        match rec.lsda() {
            Some((lsda, off)) => {
                let lsda = ctx.resolve_isec(lsda);
                match sym_at.get(&(lsda, off as u64)) {
                    Some(&symnum) => {
                        cu_data.extend_from_slice(&0u64.to_le_bytes());
                        cu_relocs.push(MachRel {
                            r_address: entry + 24,
                            bits: symnum | (3 << 25) | (1 << 27),
                        });
                    }
                    None => {
                        let l = &ctx.isecs[lsda];
                        let lsda_addr = ctx.chunk_header(l.output_section().unwrap()).addr
                            + l.offset as u64
                            + off as u64;
                        cu_data.extend_from_slice(&lsda_addr.to_le_bytes());
                        cu_relocs.push(MachRel {
                            r_address: entry + 24,
                            bits: ctx.isec_n_sect(l) as u32 | (3 << 25),
                        });
                    }
                }
            }
            None => cu_data.extend_from_slice(&0u64.to_le_bytes()),
        }
    }
    if let Some(slot) = cu_slot {
        debug_assert_eq!(cu_data.len() as u64, extras[slot].size);
        extras[slot].data = cu_data;
        extras[slot].relocs = cu_relocs;
    }

    // __TEXT,__eh_frame, in ld64's form. A CIE's personality cell is a
    // 4-byte pcrel GOT reference (the shape compilers emit). An FDE's
    // self-relative fields become SUBTRACTOR pairs against the atoms'
    // symbols, the field holding the addend: the CIE pointer is
    // func.eh + 4 - EH_Frame1, pc_begin is the function's symbol - 8 -
    // func.eh, and the LSDA pointer its symbol - offset - func.eh. A
    // function or LSDA without a symbol keeps a self-relative value.
    let mut eh_data: Vec<u8> = Vec::new();
    let mut eh_relocs: Vec<MachRel> = Vec::new();
    let mut eh_patches: Vec<(u32, u64, u8)> = Vec::new();
    {
        let mut cie_local: HashMap<usize, usize> = HashMap::new();
        for (i, &(r, _)) in eh_records.iter().enumerate() {
            if let EhRec::Cie(c) = r {
                cie_local.insert(c, eh_local[i]);
            }
        }
        let pair = |rels: &mut Vec<MachRel>, at: u32, length: u32, from: u32, to: u32| {
            rels.push(MachRel {
                r_address: at,
                bits: from | (length << 25) | (1 << 27) | ((E::RELOC_SUBTRACTOR as u32) << 28),
            });
            rels.push(MachRel {
                r_address: at,
                bits: to | (length << 25) | (1 << 27) | ((E::RELOC_UNSIGNED as u32) << 28),
            });
        };
        for (i, &(r, off)) in eh_records.iter().enumerate() {
            debug_assert_eq!(off as usize, eh_data.len());
            match r {
                EhRec::Cie(c) => {
                    let cie = &ctx.cies[c];
                    eh_data.extend_from_slice(cie.data);
                    if let Some(p) = cie.personality {
                        let Some(&symnum) = index_of_sym.get(&p) else {
                            fatal!("-r: unwind personality lost: {}", ctx.symbols[p]);
                        };
                        // The cell keeps the object's addend (4 on
                        // x86-64, where a pcrel field is relative to
                        // its own end).
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
                    let me = entry_symnum[eh_local[i]];
                    eh_data.extend_from_slice(fde.data);
                    let o = off as usize;
                    // CIE pointer.
                    let cie_sym = entry_symnum[cie_local[&(fde.cie as usize)]];
                    eh_data[o + 4..o + 8].copy_from_slice(&4u32.to_le_bytes());
                    pair(&mut eh_relocs, off + 4, 2, cie_sym, me);
                    // pc_begin.
                    let func_isec = ctx.resolve_isec(fde.isec as usize);
                    match sym_at.get(&(func_isec, fde.func_offset as u64)) {
                        Some(&func_sym) => {
                            eh_data[o + 8..o + 16].copy_from_slice(&(-8i64).to_le_bytes());
                            pair(&mut eh_relocs, off + 8, 3, me, func_sym);
                        }
                        None => {
                            let isec = &ctx.isecs[func_isec];
                            let func_addr = ctx.chunk_header(isec.output_section().unwrap()).addr
                                + isec.offset as u64
                                + fde.func_offset as u64;
                            eh_patches.push((off + 8, func_addr, 8));
                        }
                    }
                    // LSDA.
                    if let Some((lsda, lsda_off)) = fde.lsda {
                        let mut pos = 24;
                        while eh_data[o + pos] & 0x80 != 0 {
                            pos += 1;
                        }
                        pos += 1;
                        let size = ctx.cies[fde.cie as usize].lsda_size;
                        let lsda = ctx.resolve_isec(lsda as usize);
                        match sym_at.get(&(lsda, lsda_off as u64)) {
                            Some(&lsda_sym) => {
                                let a = -(pos as i64);
                                match size {
                                    8 => eh_data[o + pos..o + pos + 8]
                                        .copy_from_slice(&a.to_le_bytes()),
                                    _ => eh_data[o + pos..o + pos + 4]
                                        .copy_from_slice(&(a as i32).to_le_bytes()),
                                }
                                pair(
                                    &mut eh_relocs,
                                    off + pos as u32,
                                    if size == 8 { 3 } else { 2 },
                                    me,
                                    lsda_sym,
                                );
                            }
                            None => {
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
        }
    }
    if let Some(slot) = eh_slot {
        debug_assert_eq!(eh_data.len() as u64, extras[slot].size);
        extras[slot].data = eh_data;
        extras[slot].relocs = eh_relocs;
        extras[slot].patches = eh_patches;
    }

    // Regenerate each section's relocations against the merged tables.
    let mut sect_relocs: Vec<Vec<MachRel>> = Vec::new();
    for &chunk_idx in &section_chunks {
        let isecs = &ctx.output_section(chunk_idx).members;
        let mut rels: Vec<MachRel> = Vec::new();
        for &id in isecs {
            let isec = &ctx.isecs[id];
            for rel in crate::macho::input_files::isec_relocs_of(&ctx.objs, isec) {
                let r_address = (isec.offset as u64 + rel.offset as u64) as u32;
                let length = rel.size.trailing_zeros();

                match rel.target() {
                    RelocTarget::Sym(idx) => {
                        let sym_id = ctx.objs[isec.file as usize].symbols[idx as usize];
                        let Some(&symnum) = index_of_sym.get(&sym_id) else {
                            fatal!(
                                "-r: cannot re-emit relocation against {}",
                                ctx.symbols[sym_id].name()
                            );
                        };
                        // An explicit addend record precedes relocations
                        // whose instruction can't hold one.
                        if rel.addend != 0 && E::relocatable_needs_addend(rel.r_type) {
                            rels.push(MachRel {
                                r_address,
                                bits: (rel.addend as u32 & 0xff_ffff)
                                    | (2 << 25)
                                    | ((E::RELOC_ADDEND as u32) << 28),
                            });
                        }
                        rels.push(MachRel {
                            r_address,
                            bits: symnum
                                | ((rel.is_pcrel as u32) << 24)
                                | (length << 25)
                                | (1 << 27)
                                | ((rel.r_type as u32) << 28),
                        });
                    }
                    RelocTarget::Section(target) => {
                        let target = ctx.resolve_isec(target as usize);
                        if let Some(e) = atom_target(target, rel.addend) {
                            rels.push(MachRel {
                                r_address,
                                bits: entry_symnum[e]
                                    | ((rel.is_pcrel as u32) << 24)
                                    | (length << 25)
                                    | (1 << 27)
                                    | ((rel.r_type as u32) << 28),
                            });
                            continue;
                        }
                        let t = &ctx.isecs[target];
                        let ord = ctx.isec_n_sect(t) as u32;
                        rels.push(MachRel {
                            r_address,
                            bits: ord
                                | ((rel.is_pcrel as u32) << 24)
                                | (length << 25)
                                | ((rel.r_type as u32) << 28),
                        });
                    }
                }
            }
        }
        sect_relocs.push(rels);
    }

    // Auto-link requests are not acted on in a -r link; each distinct
    // one is carried into the output as an LC_LINKER_OPTION command,
    // in first-seen order, for the final link to resolve.
    let mut linker_options: Vec<&Vec<String>> = Vec::new();
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
    let linker_option_cmdsize = |opt: &Vec<String>| -> usize {
        align_to(12 + opt.iter().map(|s| s.len() + 1).sum::<usize>() as u64, 8) as usize
    };

    // File layout: header, one segment command with all sections,
    // build version, linker options, symtab commands; then section
    // contents, relocations, symbols and strings.
    let ncmds = 4 + linker_options.len() as u32;
    let num_sections = sects.len();
    let seg_cmd_size = size_of::<SegmentCommand>() + num_sections * size_of::<MachSection>();
    let sizeofcmds = seg_cmd_size
        + size_of::<BuildVersionCommand>()
        + linker_options.iter().map(|o| linker_option_cmdsize(o)).sum::<usize>()
        + size_of::<SymtabCommand>()
        + size_of::<DysymtabCommand>();
    let mut off = (size_of::<MachHeader>() + sizeofcmds) as u64;

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
        flags: MH_SUBSECTIONS_VIA_SYMBOLS,
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
                    reloff: extra.reloff as u32,
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

    let bv = BuildVersionCommand {
        cmd: LC_BUILD_VERSION,
        cmdsize: size_of::<BuildVersionCommand>() as u32,
        platform: ctx.args.platform,
        minos: ctx.args.platform_minos,
        sdk: ctx.args.platform_sdk,
        ntools: 0,
    };
    bv.write_to(&mut buf[p..]);
    p += size_of::<BuildVersionCommand>();

    for opt in &linker_options {
        let cmdsize = linker_option_cmdsize(opt);
        buf[p..p + 4].copy_from_slice(&LC_LINKER_OPTION.to_le_bytes());
        buf[p + 4..p + 8].copy_from_slice(&(cmdsize as u32).to_le_bytes());
        buf[p + 8..p + 12].copy_from_slice(&(opt.len() as u32).to_le_bytes());
        let mut q = p + 12;
        for s in opt.iter() {
            buf[q..q + s.len()].copy_from_slice(s.as_bytes());
            q += s.len() + 1;
        }
        p += cmdsize;
    }

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

    let dst_cmd = DysymtabCommand {
        cmd: LC_DYSYMTAB,
        cmdsize: size_of::<DysymtabCommand>() as u32,
        ilocalsym: 0,
        nlocalsym: nlocal,
        iextdefsym: nlocal,
        nextdefsym: nextdef,
        iundefsym: nlocal + nextdef,
        nundefsym: nundef,
        ..Default::default()
    };
    dst_cmd.write_to(&mut buf[p..]);

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

            for rel in crate::macho::input_files::isec_relocs_of(&ctx.objs, isec) {
                let RelocTarget::Section(target) = rel.target() else {
                    continue;
                };
                let target = ctx.resolve_isec(target as usize);
                let t = &ctx.isecs[target];
                let target_addr = ctx.chunk_header(t.output_section().unwrap()).addr
                    + t.offset as u64
                    + rel.addend as u64;
                let loc = dst + rel.offset as usize;
                if let Some(e) = atom_target(target, rel.addend) {
                    // Now a relocation against the atom's symbol: the
                    // field holds the addend relative to it, in the
                    // form an object's extern relocation uses.
                    let mut val = (target_addr - locals[e].addr) as i64;
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
