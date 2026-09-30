//! Dead-stripping (-dead_strip): mold's gc-sections for Mach-O.
//!
//! Subsections not reachable from the roots - the entry point,
//! exported symbols (for a dylib), initializers and everything the
//! format pins (no-dead-strip sections and symbols) - are removed.
//! Reachability follows relocations and unwind-info edges, so a live
//! function keeps its LSDA and personality. This is passes.rs's
//! liveness walk's section-level counterpart, and mirrors
//! gc_sections.rs in mold (dead-strip.cc in sold).

use rayon::prelude::*;

use crate::context::Context;
use crate::input_files::FileId;
use crate::input_sections::{InputSection, RelocTarget};
use crate::macho::*;
use crate::symbol::Symbol;
use crate::target::Target;

/// Removes subsections that are not reachable from the roots: the entry
/// point, exported symbols (for a dylib), and everything the format
/// requires to stay (initializers, no-dead-strip sections and symbols).
/// Reachability follows relocations and unwind-info edges.
pub fn dead_strip<E: Target>(ctx: &mut Context<E>) {
    // Merged literals and rewritten ObjC data stand in for the
    // subsections they replaced; roots and edges are marked through to
    // the replacement.
    let redirects: Vec<usize> =
        (0..ctx.isecs.len()).into_par_iter().map(|i| ctx.resolve_isec(i)).collect();
    let redirects = &redirects;

    let roots = collect_root_set(ctx, redirects);

    // For -why_live: who first marked each subsection (usize::MAX for
    // roots), giving a spanning tree of the liveness walk. The set of
    // live sections is order-independent, but the spanning tree is not,
    // so -why_live keeps the serial walk to report stable chains.
    let mut pred = Vec::new();
    if ctx.args.why_live.is_empty() {
        mark(ctx, redirects, &roots);
    } else {
        pred = vec![usize::MAX; ctx.isecs.len()];
        walk(ctx, redirects, &mut pred, roots);
    }

    mark_live_support(ctx, redirects, &mut pred);
    sweep(ctx);
    print_why_live(ctx, &pred);
}

/// Sections the format keeps regardless of references: initializers,
/// no-dead-strip sections and the ObjC image info.
fn should_keep<E: Target>(ctx: &Context<E>, isec: &InputSection) -> bool {
    let hdr = ctx.hdr_of(isec);
    matches!(hdr.section_type(), S_MOD_INIT_FUNC_POINTERS | S_INIT_FUNC_OFFSETS)
        || hdr.flags & S_ATTR_NO_DEAD_STRIP != 0
        || hdr.sectname() == "__objc_imageinfo"
}

/// Marks the roots and returns them in a fixed order, so that
/// -why_live's serial walk is reproducible. They are found on all
/// cores; marking stays serial (it is a handful of sections).
fn collect_root_set<E: Target>(ctx: &Context<E>, redirects: &[usize]) -> Vec<usize> {
    let mut roots = Vec::new();
    // Liveness is marked in place, on the section's atomic visited bit
    // (mold's IS_VISITED), rather than in side arrays copied back at
    // the end.
    let mut enqueue = |id: usize| {
        let id = redirects[id];
        if ctx.isecs[id].mark_visited() {
            roots.push(id);
        }
    };

    // Sections of dead archive members are not part of the link at all
    // and must not be resurrected.
    let sections: Vec<usize> = ctx
        .isecs
        .par_iter()
        .enumerate()
        .filter(|(_, isec)| isec.is_alive() && should_keep(ctx, isec))
        .map(|(id, _)| id)
        .collect();
    for id in sections {
        enqueue(id);
    }

    // Initializers converted to __init_offsets are roots; their source
    // sections are gone.
    for &(isec, _) in &ctx.init_offsets.init_funcs {
        enqueue(isec);
    }

    // Sections defining a no-dead-strip or an exported symbol.
    let exports_all = ctx.args.output_type != MH_EXECUTE || ctx.args.export_dynamic;
    let is_exported = |sym: &Symbol| {
        sym.is_extern()
            && !sym.is_private_extern()
            && sym.is_defined()
            && (exports_all
                || ctx
                    .args
                    .exported_symbols
                    .as_ref()
                    .is_some_and(|exported| exported.find(sym.name().as_bytes()) != -1))
    };
    let syms: Vec<usize> = ctx
        .symbols
        .syms
        .par_iter()
        .filter(|sym| sym.no_dead_strip() || is_exported(sym))
        .filter_map(|sym| sym.input_section().map(|i| i as usize))
        .collect();
    for id in syms {
        enqueue(id);
    }

    // -u retains the atom as well as extracting its containing archive
    // member. It may name a private external, unlike an export root.
    for name in &ctx.args.forced_undefined {
        if let Some(id) = ctx.symbols.get(name)
            && let Some(isec) = ctx.symbols[id].input_section()
        {
            enqueue(isec as usize);
        }
    }

    if ctx.args.output_type == MH_EXECUTE
        && let Some(id) = ctx.symbols.get(&ctx.args.entry)
        && let Some(isec) = ctx.symbols[id].input_section()
    {
        enqueue(isec as usize);
    }
    roots
}

/// Calls `f` with each subsection that subsection `id` references:
/// its relocation targets and, via its compact-unwind record range,
/// its LSDAs and personality routine, so a live function keeps them
/// alive.
#[inline]
fn for_each_edge<E: Target>(ctx: &Context<E>, id: usize, mut f: impl FnMut(usize)) {
    let isec = &ctx.isecs[id];
    let file = &ctx.objs[isec.file as usize];
    for rel in ctx.isec_relocs(id) {
        match rel.target() {
            RelocTarget::Sym(idx) => {
                if let Some(target) = ctx.symbols[file.symbols[idx as usize]].input_section() {
                    f(target as usize);
                }
            }
            RelocTarget::Section(target) => f(target as usize),
        }
    }

    let recs = isec.unwind_offset as usize..(isec.unwind_offset + isec.nunwind) as usize;
    for rec in &ctx.unwind_records[recs] {
        if let Some((lsda, _)) = rec.lsda() {
            f(lsda);
        }
        let mut personality = rec.personality();
        if let Some(fde) = rec.fde() {
            if let Some((lsda, _)) = ctx.fdes[fde].lsda {
                f(lsda as usize);
            }
            personality = personality.or(ctx.cies[ctx.fdes[fde].cie as usize].personality);
        }
        if let Some(p) = personality
            && let Some(target) = ctx.symbols[p].input_section()
        {
            f(target as usize);
        }
    }
}

const GC_BATCH: usize = 16;

/// Marks what subsection `id` references. Recurses for a few levels
/// before queueing more work in `next` so that we do not create a
/// rayon task for every edge.
fn visit_section<E: Target>(
    ctx: &Context<E>,
    redirects: &[usize],
    id: usize,
    depth: usize,
    next: &mut Vec<usize>,
) {
    // Read all the edges before recursing into any: recursing midway
    // through the relocations evicts them and costs a few percent more
    // CPU on a large link.
    let mut targets = Vec::new();
    for_each_edge(ctx, id, |target| targets.push(target));
    for target in targets {
        let target = redirects[target];
        if ctx.isecs[target].mark_visited() {
            if depth < 3 {
                visit_section(ctx, redirects, target, depth + 1, next);
            } else {
                next.push(target);
            }
        }
    }
}

/// Visits marked sections and publishes newly found work in batches.
/// Batching preserves dynamic load balancing while amortizing rayon
/// task allocation.
fn visit_batch<'s, E: Target>(
    ctx: &'s Context<E>,
    redirects: &'s [usize],
    batch: &[usize],
    scope: &rayon::Scope<'s>,
) {
    let mut next = Vec::with_capacity(GC_BATCH);
    for &id in batch {
        visit_section(ctx, redirects, id, 0, &mut next);
        if next.len() >= GC_BATCH {
            let found = std::mem::replace(&mut next, Vec::with_capacity(GC_BATCH));
            scope.spawn(move |scope| visit_batch(ctx, redirects, &found, scope));
        }
    }
    if !next.is_empty() {
        scope.spawn(move |scope| visit_batch(ctx, redirects, &next, scope));
    }
}

/// Marks everything reachable from the roots. mold's gc-sections marks
/// with a work-stealing task pool, not synchronous frontier rounds:
/// each visit follows edges up to three levels inline and banks the
/// rest in small batches that spawn as tasks (rayon tasks are heavier
/// than TBB feeder items, so batches of 16 amortize them). Unoptimized
/// debug code has deep call chains, which starve round-based marking;
/// dynamic tasks keep every core fed regardless of graph depth.
fn mark<E: Target>(ctx: &Context<E>, redirects: &[usize], roots: &[usize]) {
    rayon::scope(|scope| {
        roots.par_chunks(GC_BATCH).for_each(|batch| visit_batch(ctx, redirects, batch, scope));
    });
}

/// The serial walk from the marked sections on `stack`, for -why_live's
/// stable chains and for what a live-support atom keeps. Records who
/// first marked each section in `pred` unless it is empty.
fn walk<E: Target>(
    ctx: &Context<E>,
    redirects: &[usize],
    pred: &mut [usize],
    mut stack: Vec<usize>,
) {
    while let Some(id) = stack.pop() {
        for_each_edge(ctx, id, |target| {
            let target = redirects[target];
            if ctx.isecs[target].mark_visited() {
                if !pred.is_empty() {
                    pred[target] = id;
                }
                stack.push(target);
            }
        });
    }
}

/// A live-support atom (S_ATTR_LIVE_SUPPORT) lives only if it
/// references a live atom, and then keeps what it references. ld64
/// checks them once, in input order, after what the roots reach is
/// marked, so one that only a later live-support atom would make live
/// stays dead.
fn mark_live_support<E: Target>(ctx: &Context<E>, redirects: &[usize], pred: &mut [usize]) {
    let live_support: Vec<usize> = ctx
        .isecs
        .par_iter()
        .enumerate()
        .filter(|(_, isec)| isec.is_alive() && ctx.hdr_of(isec).flags & S_ATTR_LIVE_SUPPORT != 0)
        .map(|(id, _)| id)
        .collect();

    for id in live_support {
        let mut references_live = false;
        for_each_edge(ctx, id, |target| {
            references_live |= ctx.isecs[redirects[target]].is_visited();
        });
        let id = redirects[id];
        if references_live && ctx.isecs[id].mark_visited() {
            walk(ctx, redirects, pred, vec![id]);
        }
    }
}

/// Kills the sections the walk did not reach, consuming the visited
/// bit, and drops the unwind records and FDEs of dead functions.
fn sweep<E: Target>(ctx: &mut Context<E>) {
    for isec in ctx.isecs.iter_mut() {
        let visited = isec.take_visited();
        let alive = isec.is_alive();
        isec.set_alive(visited && alive);
    }

    // Remap the record-to-FDE links around the dropped FDEs.
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
        if rec.fde_idx != crate::input_files::UNWIND_NONE {
            // usize::MAX (a dropped FDE) narrows to UNWIND_NONE.
            rec.fde_idx = map[rec.fde_idx as usize] as u32;
        }
        true
    });

    // The compaction moved the surviving records; refresh the ranges.
    crate::passes::refresh_unwind_ranges(ctx);
}

/// Refresh symbol usage after atom liveness is known. Undefined references
/// in removed atoms must neither cause errors nor become dynamic imports.
pub fn mark_live_references<E: Target>(ctx: &mut Context<E>) {
    ctx.symbols.syms.par_iter().for_each(|sym| sym.unmark());
    ctx.isecs
        .par_iter()
        .filter(|isec| isec.is_alive() && isec.replacement == crate::input_sections::NO_REPLACEMENT)
        .for_each(|isec| {
            for rel in crate::input_files::isec_relocs_of(&ctx.objs, isec) {
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
fn print_why_live<E: Target>(ctx: &Context<E>, pred: &[usize]) {
    if ctx.args.why_live.is_empty() {
        return;
    }

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
            crate::passes::resolved_file_name(ctx.objs[sec.file as usize].mf)
        )
    };

    for sym in &ctx.symbols.syms {
        if !matches!(sym.file(), Some(FileId::Obj(_)))
            || ctx.args.why_live.find(sym.name().as_bytes()) == -1
        {
            continue;
        }
        let Some(isec) = sym.input_section().map(|i| i as usize) else { continue };
        let mut isec = ctx.resolve_isec(isec);
        if !ctx.isecs[isec].is_alive() {
            continue;
        }
        // On stderr, as ld-prime prints it.
        eprintln!(
            "{} from {}",
            sym.name(),
            crate::passes::resolved_file_name(ctx.objs[ctx.isecs[isec].file as usize].mf)
        );
        let mut indent = 1;
        while pred[isec] != usize::MAX {
            isec = pred[isec];
            eprintln!("{:indent$}{}", "", describe(isec), indent = indent * 2);
            indent += 1;
        }
    }
}
