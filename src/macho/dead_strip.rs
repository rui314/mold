//! Dead-stripping (-dead_strip): mold's gc-sections for Mach-O.
//!
//! Subsections not reachable from the roots - the entry point,
//! exported symbols (for a dylib), initializers and everything the
//! format pins (no-dead-strip sections and symbols) - are removed.
//! Reachability follows relocations and unwind-info edges, so a live
//! function keeps its LSDA and personality. This is passes.rs's
//! liveness walk's section-level counterpart, and mirrors
//! gc_sections.rs in mold-rust (dead-strip.cc in sold).

use crate::macho::arch::Arch;
use crate::macho::context::Context;
use crate::macho::format::*;
use crate::macho::input_files::FileId;
use crate::macho::input_sections::RelocTarget;

/// Removes subsections that are not reachable from the roots: the entry
/// point, exported symbols (for a dylib), and everything the format
/// requires to stay (initializers, no-dead-strip sections and symbols).
/// Reachability follows relocations and unwind-info edges.
pub fn dead_strip<E: Arch>(ctx: &mut Context<E>) {
    // For -why_live: who first marked each subsection (usize::MAX for
    // roots), giving a spanning tree of the liveness walk. Only kept
    // when it will be printed.
    let mut pred: Vec<usize> =
        if ctx.args.why_live.is_empty() { Vec::new() } else { vec![usize::MAX; ctx.isecs.len()] };
    let mut stack: Vec<usize> = Vec::new();
    let redirects: Vec<usize> = {
        use rayon::prelude::*;
        (0..ctx.isecs.len()).into_par_iter().map(|i| ctx.resolve_isec(i)).collect()
    };
    let redirects = &redirects;
    // Liveness is marked in place, on the section's atomic visited bit
    // (mold-rust's IS_VISITED), rather than in side arrays copied back
    // at the end.
    let mark = move |ctx: &Context<E>,
                     pred: &mut Vec<usize>,
                     stack: &mut Vec<usize>,
                     id: usize,
                     from: usize| {
        let id = redirects[id];
        if ctx.isecs[id].mark_visited() {
            if !pred.is_empty() {
                pred[id] = from;
            }
            stack.push(id);
        }
    };

    // Section-level roots, found on all cores; marking stays serial
    // (it is a handful of sections). Sections of dead archive members
    // are not part of the link at all and must not be resurrected.
    let root_ids: Vec<usize> = {
        use rayon::prelude::*;
        ctx.isecs
            .par_iter()
            .enumerate()
            .filter_map(|(id, isec)| {
                if !isec.is_alive() {
                    return None;
                }
                let keep_type = matches!(
                    ctx.hdr_of(isec).section_type(),
                    S_MOD_INIT_FUNC_POINTERS | S_INIT_FUNC_OFFSETS
                );
                let keep_attr =
                    ctx.hdr_of(isec).flags & (S_ATTR_NO_DEAD_STRIP | S_ATTR_LIVE_SUPPORT) != 0;
                if keep_type || keep_attr || ctx.hdr_of(isec).sectname() == "__objc_imageinfo" {
                    Some(id)
                } else {
                    None
                }
            })
            .collect()
    };
    for id in root_ids {
        mark(ctx, &mut pred, &mut stack, id, usize::MAX);
    }

    // Initializers converted to __init_offsets are roots; their source
    // sections are gone.
    for &(isec, _) in &ctx.init_offsets.init_funcs {
        mark(ctx, &mut pred, &mut stack, isec, usize::MAX);
    }

    // Symbol-level roots, found on all cores like the section roots.
    let sym_roots: Vec<usize> = {
        use rayon::prelude::*;
        ctx.symbols
            .syms
            .par_iter()
            .filter_map(|sym| {
                let is_root = sym.no_dead_strip()
                    || ((ctx.args.output_type != MH_EXECUTE
                        || ctx.args.export_dynamic
                        || ctx
                            .args
                            .exported_symbols
                            .as_ref()
                            .is_some_and(|names| names.iter().any(|name| name == sym.name())))
                        && sym.is_extern()
                        && !sym.is_private_extern()
                        && sym.is_defined());
                if is_root { sym.input_section().map(|i| i as usize) } else { None }
            })
            .collect()
    };
    for isec in sym_roots {
        mark(ctx, &mut pred, &mut stack, isec, usize::MAX);
    }
    // -u retains the atom as well as extracting its containing archive
    // member. It may name a private external, unlike an export root.
    for name in &ctx.args.forced_undefined {
        if let Some(id) = ctx.symbols.get(name) {
            if let Some(isec) = ctx.symbols[id].input_section() {
                mark(ctx, &mut pred, &mut stack, isec as usize, usize::MAX);
            }
        }
    }
    if ctx.args.output_type == MH_EXECUTE {
        if let Some(id) = ctx.symbols.get(&ctx.args.entry) {
            if let Some(isec) = ctx.symbols[id].input_section().map(|i| i as usize) {
                mark(ctx, &mut pred, &mut stack, isec, usize::MAX);
            }
        }
    }

    // Propagate liveness. mold's gc-sections walks the graph in
    // parallel rounds: the frontier's out-edges are computed on all
    // cores, and an atomic visited bit decides which targets extend
    // the next frontier. The set of live sections is
    // order-independent, so the result is deterministic; only the
    // spanning tree (who marked whom) is not, so -why_live keeps the
    // serial walk to report stable chains.
    let edges_of = |id: usize, out: &mut Vec<usize>| {
        for rel in ctx.isec_relocs(id) {
            match rel.target() {
                RelocTarget::Sym(idx) => {
                    let sym =
                        &ctx.symbols[ctx.objs[ctx.isecs[id].file as usize].symbols[idx as usize]];
                    if let Some(isec) = sym.input_section().map(|i| i as usize) {
                        out.push(isec);
                    }
                }
                RelocTarget::Section(isec) => out.push(isec as usize),
            }
        }
        // A live function keeps its LSDA and personality alive, via
        // the subsection's compact-unwind record range.
        let isec = &ctx.isecs[id];
        let recs = isec.unwind_offset as usize..(isec.unwind_offset + isec.nunwind) as usize;
        for rec in &ctx.unwind_records[recs] {
            if let Some((lsda, _)) = rec.lsda() {
                out.push(lsda);
            }
            let mut personality = rec.personality();
            if let Some(fde) = rec.fde() {
                if let Some((lsda, _)) = ctx.fdes[fde].lsda {
                    out.push(lsda as usize);
                }
                personality = personality.or(ctx.cies[ctx.fdes[fde].cie as usize].personality);
            }
            if let Some(p) = personality {
                if let Some(isec) = ctx.symbols[p].input_section().map(|i| i as usize) {
                    out.push(isec);
                }
            }
        }
    };

    if ctx.args.why_live.is_empty() {
        use rayon::prelude::*;
        // mold's gc-sections marks with a work-stealing task pool, not
        // synchronous frontier rounds: each visit follows edges up to
        // three levels inline and banks the rest in small batches that
        // spawn as tasks (rayon tasks are heavier than TBB feeder
        // items, so batches of 16 amortize them). Unoptimized debug
        // code has deep call chains, which starve round-based marking;
        // dynamic tasks keep every core fed regardless of graph depth.
        const GC_BATCH: usize = 16;
        struct Gc<'a, E: Arch> {
            ctx: &'a Context<E>,
            redirects: &'a [usize],
        }
        fn visit_section<'s, E: Arch>(
            gc: &'s Gc<'s, E>,
            id: usize,
            depth: usize,
            scope: &rayon::Scope<'s>,
            next: &mut Vec<usize>,
        ) {
            let mut targets = Vec::new();
            for rel in gc.ctx.isec_relocs(id) {
                match rel.target() {
                    RelocTarget::Sym(idx) => {
                        let sym = &gc.ctx.symbols
                            [gc.ctx.objs[gc.ctx.isecs[id].file as usize].symbols[idx as usize]];
                        if let Some(isec) = sym.input_section().map(|i| i as usize) {
                            targets.push(isec);
                        }
                    }
                    RelocTarget::Section(isec) => targets.push(isec as usize),
                }
            }
            let isec = &gc.ctx.isecs[id];
            let recs = isec.unwind_offset as usize..(isec.unwind_offset + isec.nunwind) as usize;
            for rec in &gc.ctx.unwind_records[recs] {
                if let Some((lsda, _)) = rec.lsda() {
                    targets.push(lsda);
                }
                let mut personality = rec.personality();
                if let Some(fde) = rec.fde() {
                    if let Some((lsda, _)) = gc.ctx.fdes[fde].lsda {
                        targets.push(lsda as usize);
                    }
                    personality =
                        personality.or(gc.ctx.cies[gc.ctx.fdes[fde].cie as usize].personality);
                }
                if let Some(p) = personality {
                    if let Some(isec) = gc.ctx.symbols[p].input_section().map(|i| i as usize) {
                        targets.push(isec);
                    }
                }
            }
            for t in targets {
                let t = gc.redirects[t];
                if gc.ctx.isecs[t].mark_visited() {
                    if depth < 3 {
                        visit_section(gc, t, depth + 1, scope, next);
                    } else {
                        next.push(t);
                    }
                }
            }
        }
        fn visit_batch<'s, E: Arch>(
            gc: &'s Gc<'s, E>,
            batch: Vec<usize>,
            scope: &rayon::Scope<'s>,
        ) {
            let mut next = Vec::with_capacity(GC_BATCH);
            for id in batch {
                visit_section(gc, id, 0, scope, &mut next);
                if next.len() >= GC_BATCH {
                    let found = std::mem::replace(&mut next, Vec::with_capacity(GC_BATCH));
                    scope.spawn(move |scope| visit_batch(gc, found, scope));
                }
            }
            if !next.is_empty() {
                scope.spawn(move |scope| visit_batch(gc, next, scope));
            }
        }
        let gc = Gc { ctx, redirects };
        let gc = &gc;
        let roots = std::mem::take(&mut stack);
        rayon::scope(|scope| {
            roots.par_chunks(GC_BATCH).for_each(|batch| visit_batch(gc, batch.to_vec(), scope));
        });
    } else {
        while let Some(id) = stack.pop() {
            let mut out = Vec::new();
            edges_of(id, &mut out);
            for t in out {
                mark(ctx, &mut pred, &mut stack, t, id);
            }
        }
    }

    // A section stays alive if the walk reached it; the visited bit is
    // consumed here.
    for isec in ctx.isecs.iter_mut() {
        let visited = isec.take_visited();
        let alive = isec.is_alive();
        isec.set_alive(visited && alive);
    }

    print_why_live(ctx, &pred);

    // Drop unwind records and FDEs of dead functions, remapping the
    // record-to-FDE links.
    let mut fde_map = vec![usize::MAX; ctx.fdes.len()];
    let mut kept_fdes = Vec::new();
    let fdes = std::mem::take(&mut ctx.fdes);
    for (i, fde) in fdes.into_iter().enumerate() {
        if ctx.isecs[fde.isec as usize].is_alive() {
            fde_map[i] = kept_fdes.len();
            kept_fdes.push(fde);
        }
    }
    ctx.fdes = kept_fdes;
    let isecs = &ctx.isecs;
    let map = &fde_map;
    ctx.unwind_records.retain_mut(|rec| {
        if !isecs[rec.isec as usize].is_alive() {
            return false;
        }
        if rec.fde_idx != crate::macho::input_files::UNWIND_NONE {
            // usize::MAX (a dropped FDE) narrows to UNWIND_NONE.
            rec.fde_idx = map[rec.fde_idx as usize] as u32;
        }
        true
    });

    // The compaction moved the surviving records; refresh the ranges.
    crate::macho::passes::refresh_unwind_ranges(ctx);
}

/// Refresh symbol usage after atom liveness is known. Undefined references
/// in removed atoms must neither cause errors nor become dynamic imports.
pub fn mark_live_references<E: Arch>(ctx: &mut Context<E>) {
    use rayon::prelude::*;
    ctx.symbols.syms.par_iter().for_each(|sym| sym.unmark());
    ctx.isecs
        .par_iter()
        .filter(|isec| {
            isec.is_alive() && isec.replacement == crate::macho::input_sections::NO_REPLACEMENT
        })
        .for_each(|isec| {
            for rel in crate::macho::input_files::isec_relocs_of(&ctx.objs, isec) {
                if let Some(id) = ctx.reloc_target_sym(isec.file as usize, rel) {
                    ctx.symbols[id].mark();
                }
            }
        });
    for rec in &ctx.unwind_records {
        if let Some(id) = rec.personality() {
            ctx.symbols[id].mark();
        }
    }
    for fde in &ctx.fdes {
        if let Some(id) = ctx.cies[fde.cie as usize].personality {
            ctx.symbols[id].mark();
        }
    }
    if let Some(id) = ctx.objc_stubs.msgsend_sym {
        ctx.symbols[id].mark();
    }
    for name in ctx
        .args
        .forced_undefined
        .iter()
        .chain((ctx.args.output_type == MH_EXECUTE).then_some(&ctx.args.entry))
        .chain(ctx.args.aliases.iter().map(|(base, _)| base))
    {
        if let Some(id) = ctx.symbols.get(name) {
            ctx.symbols[id].mark();
        }
    }
    ctx.symbols.syms.par_iter_mut().for_each(|sym| {
        sym.set_is_used(sym.is_marked());
        sym.unmark();
    });
}

/// -why_live prints, for each symbol matching a -why_live pattern
/// ("*" wildcards), the chain of references that kept it alive: the
/// liveness walk's spanning tree read backwards, one "symbol from
/// file" line per hop, ending at a dead-strip root. Only meaningful
/// under -dead_strip, like ld64's option of the same name.
fn print_why_live<E: Arch>(ctx: &Context<E>, pred: &[usize]) {
    if ctx.args.why_live.is_empty() {
        return;
    }

    let matches = crate::macho::util::glob_match;

    // A displayable symbol for each live subsection: prefer an extern
    // symbol defined at it, else any named local.
    let mut name_of: std::collections::HashMap<usize, &str> = std::collections::HashMap::new();
    for sym in &ctx.symbols.syms {
        if !matches!(sym.file(), Some(FileId::Obj(_))) || sym.name().is_empty() {
            continue;
        }
        let Some(isec) = sym.input_section().map(|i| i as usize) else { continue };
        let isec = ctx.resolve_isec(isec);
        match name_of.entry(isec) {
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(sym.name());
            }
            std::collections::hash_map::Entry::Occupied(mut e) => {
                if sym.is_extern() && sym.value == 0 {
                    e.insert(sym.name());
                }
            }
        }
    }
    let describe = |isec: usize| -> String {
        let sec = &ctx.isecs[isec];
        let name = name_of.get(&isec).copied().map(String::from).unwrap_or_else(|| {
            format!("{},{}", ctx.hdr_of(sec).segname(), ctx.hdr_of(sec).sectname())
        });
        if ctx.is_internal(sec.file as usize) {
            return name;
        }
        format!(
            "{} from {}",
            name,
            crate::macho::passes::file_display(&ctx.objs[sec.file as usize])
        )
    };

    for sym in &ctx.symbols.syms {
        if !matches!(sym.file(), Some(FileId::Obj(_)))
            || !ctx.args.why_live.iter().any(|p| matches(p, sym.name()))
        {
            continue;
        }
        let Some(isec) = sym.input_section().map(|i| i as usize) else { continue };
        let mut isec = ctx.resolve_isec(isec);
        if !ctx.isecs[isec].is_alive() {
            continue;
        }
        println!(
            "{} from {}",
            sym.name(),
            crate::macho::passes::file_display(&ctx.objs[ctx.isecs[isec].file as usize])
        );
        let mut indent = 1;
        while pred[isec] != usize::MAX {
            isec = pred[isec];
            println!("{:indent$}{}", "", describe(isec), indent = indent * 2);
            indent += 1;
        }
    }
}
