//! Dead-stripping (-dead_strip): mold's gc-sections for Mach-O.
//!
//! Subsections not reachable from the roots - the entry point,
//! exported symbols (for a dylib), initializers and everything the
//! format pins (no-dead-strip sections and symbols) - are removed.
//! Reachability follows relocations and unwind-info edges, so a live
//! function keeps its LSDA and personality. This is passes.rs's
//! liveness walk's section-level counterpart, and mirrors
//! gc_sections.rs in mold (dead-strip.cc in sold).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use rayon::prelude::*;

use crate::chunks::init_offsets::InitFunc;
use crate::context::Context;
use crate::input_files::{FileId, is_literal_section};
use crate::input_sections::{InputSection, Reloc, RelocTarget};
use crate::macho::*;
use crate::symbol::{Symbol, SymbolId};
use crate::target::Target;

/// Strips dead code (see Context::strips_dead_code) and refreshes which
/// symbols live code uses. An import only stripped code used stays in
/// the symbol table if -dead_strip didn't ask for the strip.
pub fn strip_dead_code<E: Target>(ctx: &mut Context<E>) {
    let imports: Vec<SymbolId> = if ctx.args.dead_strip {
        Vec::new()
    } else {
        (0..ctx.symbols.syms.len() as SymbolId)
            .into_par_iter()
            .filter(|&id| {
                let sym = &ctx.symbols[id];
                sym.is_used() && matches!(sym.file(), Some(FileId::Dylib(_)))
            })
            .collect()
    };
    dead_strip(ctx);
    mark_live_references(ctx);
    let unused = imports.into_iter().filter(|&id| !ctx.symbols[id].is_used());
    ctx.unbound_imports.extend(unused.collect::<Vec<_>>());
}

/// Removes subsections that are not reachable from the roots: the entry
/// point, exported symbols (for a dylib), and everything the format
/// requires to stay (initializers, no-dead-strip sections and symbols).
/// Reachability follows relocations and unwind-info edges.
fn dead_strip<E: Target>(ctx: &mut Context<E>) {
    // Merged literals and rewritten ObjC data stand in for the
    // subsections they replaced; roots and edges are marked through to
    // the replacement.
    let redirects: Vec<usize> =
        (0..ctx.isecs.len()).into_par_iter().map(|i| ctx.resolve_isec(i)).collect();
    let redirects = &redirects;

    // The set of live sections is order-independent, which lets the
    // walk run on all cores; -why_live's chains are not, and it walks
    // serially as ld-prime does.
    if ctx.args.why_live.is_empty() {
        let roots = collect_root_set(ctx, redirects, |_, sym| symbol_root(ctx, sym).is_some());
        mark(ctx, redirects, &roots);
    } else {
        WhyLive::new(ctx, redirects).walk();
    }

    mark_live_support(ctx, redirects);
    sweep(ctx);
}

/// Why an atom is a dead-strip root, in ld-prime's words for -why_live.
#[derive(Clone, Copy)]
enum Root {
    /// The entry point or a -u symbol.
    InitialUndef,
    /// An exported symbol.
    GlobalDontStrip,
    /// A section or symbol the format keeps (see should_keep).
    DontDeadStrip,
}

impl Root {
    fn name(self) -> &'static str {
        match self {
            Root::InitialUndef => "initial-undef",
            Root::GlobalDontStrip => "global-dont-strip",
            Root::DontDeadStrip => "dont-dead-strip",
        }
    }
}

/// Whether a symbol makes its subsection a root, and why: a
/// no-dead-strip symbol, or an export dead stripping keeps.
fn symbol_root<E: Target>(ctx: &Context<E>, sym: &Symbol) -> Option<Root> {
    let is_exported = || {
        sym.is_extern()
            && !sym.is_private_extern()
            && sym.is_defined()
            && keeps_export(ctx, sym.name())
    };
    if sym.no_dead_strip() {
        Some(Root::DontDeadStrip)
    } else if is_exported() {
        Some(Root::GlobalDontStrip)
    } else {
        None
    }
}

/// The symbols that are roots by name, in ld-prime's order: the -u ones
/// as the command line gives them, then the entry point.
fn initial_undefines<E: Target>(ctx: &Context<E>) -> impl Iterator<Item = SymbolId> {
    let entry = ctx.args.has_entry_point().then_some(&ctx.args.entry);
    ctx.args.forced_undefined.iter().chain(entry).filter_map(|name| ctx.symbols.get(name))
}

/// Sections the format keeps regardless of references: initializers
/// and terminators, no-dead-strip sections and the ObjC image info. So
/// is every section of an object without MH_SUBSECTIONS_VIA_SYMBOLS
/// that ld64 cuts at symbols: it cannot tell where such an atom ends,
/// so it models the object as one huge atom. Sections it cuts by
/// content (literals, CFStrings, class references, thread-local
/// variable descriptors) are stripped as usual. ld-prime strips a class
/// reference nothing uses although clang marks __objc_classrefs
/// no-dead-strip (it keeps unused selector references).
fn should_keep<E: Target>(ctx: &Context<E>, isec: &InputSection) -> bool {
    let hdr = ctx.hdr_of(isec);
    matches!(hdr.section_type(), S_MOD_INIT_FUNC_POINTERS | S_MOD_TERM_FUNC_POINTERS)
        || hdr.section_type() == S_INIT_FUNC_OFFSETS
        || (hdr.flags & S_ATTR_NO_DEAD_STRIP != 0
            && !(hdr.segname() == "__DATA" && hdr.sectname() == "__objc_classrefs"))
        || hdr.sectname() == "__objc_imageinfo"
        || (!ctx.objs[isec.file as usize].subsections_via_symbols
            && !is_literal_section(hdr)
            && hdr.section_type() != S_THREAD_LOCAL_VARIABLES)
}

/// Marks the roots and returns them: the sections the format keeps,
/// those defining a symbol `is_root` takes, and those of the
/// initializers and initial undefines. They are found on all cores;
/// marking stays serial (it is a handful of sections).
fn collect_root_set<E: Target>(
    ctx: &Context<E>,
    redirects: &[usize],
    is_root: impl Fn(SymbolId, &Symbol) -> bool + Sync,
) -> Vec<usize> {
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
    for &func in &ctx.init_offsets.init_funcs {
        if let InitFunc::Local(isec, _) = func {
            enqueue(isec);
        }
    }

    // Sections defining a no-dead-strip or an exported symbol.
    let syms: Vec<usize> = ctx
        .symbols
        .syms
        .par_iter()
        .enumerate()
        .filter(|&(id, sym)| is_root(id as SymbolId, sym))
        .filter_map(|(_, sym)| Some(sym.input_section()? as usize))
        .collect();
    for id in syms {
        enqueue(id);
    }

    // -u retains the atom as well as extracting its containing archive
    // member. It may name a private external, unlike an export root.
    for id in initial_undefines(ctx) {
        if let Some(isec) = ctx.symbols[id].input_section() {
            enqueue(isec as usize);
        }
    }
    roots
}

/// The symbols live Mach-O code refers to before LTO, as ld-prime finds
/// them to tell libLTO what to preserve: it walks the atoms as dead
/// stripping does - in any link with bitcode, -dead_strip or not - from
/// the same roots, each bitcode file's "internal" atom among them,
/// which stands for its module's code and refers to every symbol the
/// module does. A bitcode definition is an atom that refers to that
/// one. So native code that only bitcode, a root or another such
/// function reaches counts, and code nothing does (a hidden helper no
/// one calls) doesn't. The export lists have hidden nothing yet; the
/// roots go by them, as `exported` says (see exported_before_lto).
pub fn native_refs_before_lto<E: Target>(
    ctx: &Context<E>,
    exported: impl Fn(SymbolId) -> bool + Sync,
) -> Vec<AtomicBool> {
    let redirects: Vec<usize> =
        (0..ctx.isecs.len()).into_par_iter().map(|i| ctx.resolve_isec(i)).collect();
    let is_root = |id: SymbolId, sym: &Symbol| sym.no_dead_strip() || exported(id);
    let mut roots = collect_root_set(ctx, &redirects, is_root);
    for module in &ctx.lto_modules {
        let obj = &ctx.objs[module.obj];
        if !obj.is_alive {
            continue;
        }
        for (nlist, &id) in obj.nlists.iter().zip(&obj.symbols) {
            if nlist.n_type() == N_UNDF
                && let Some(isec) = ctx.symbols[id].input_section()
                && ctx.isecs[redirects[isec as usize]].mark_visited()
            {
                roots.push(redirects[isec as usize]);
            }
        }
    }
    mark(ctx, &redirects, &roots);
    mark_live_support(ctx, &redirects);

    let refs: Vec<AtomicBool> =
        (0..ctx.symbols.syms.len()).map(|_| AtomicBool::new(false)).collect();
    ctx.isecs.par_iter().enumerate().for_each(|(id, isec)| {
        if !isec.is_visited() {
            return;
        }
        isec.unmark_visited();
        let file = &ctx.objs[isec.file as usize];
        for rel in ctx.isec_relocs(id) {
            if let RelocTarget::Sym(idx) = rel.target() {
                refs[file.symbols[idx as usize] as usize].store(true, Ordering::Relaxed);
            }
        }
        for_each_unwind_edge(ctx, id, |_, sym| {
            if let Some(sym) = sym {
                refs[sym as usize].store(true, Ordering::Relaxed);
            }
        });
    });
    refs
}

/// Whether a definition is exported before LTO, as ld-prime's walk
/// before it and libLTO's preserve set see it: under an export list if
/// the list names it, hidden or not; otherwise, unless
/// -unexported_symbols_list names it, any external definition in -r,
/// and a visible one in an image that exports any (see keeps_export) -
/// but not one every copy of which can be hidden, which the image
/// auto-hides (see passes::auto_hide_weak_defs).
pub fn exported_before_lto<E: Target>(ctx: &Context<E>, sym: &Symbol, hidable: bool) -> bool {
    if !sym.is_extern() || !matches!(sym.file(), Some(FileId::Obj(_))) {
        return false;
    }
    let name = sym.name().as_bytes();
    if let Some(exported) = &ctx.args.exported_symbols {
        return exported.find(name) != -1;
    }
    if ctx.args.unexported_symbols.find(name) != -1 {
        return false;
    }
    ctx.args.relocatable || (!sym.is_private_extern() && !hidable && keeps_export(ctx, sym.name()))
}

/// Whether dead stripping keeps an export named `name`: every one of a
/// dylib or bundle, and of an executable those an export list names, or
/// all its globals with -export_dynamic (but not a -preload image's,
/// which ld-prime still strips).
pub fn keeps_export<E: Target>(ctx: &Context<E>, name: &str) -> bool {
    ctx.args.output_type != MH_EXECUTE
        || (ctx.args.export_dynamic && !ctx.args.preload)
        || (ctx.args.exported_symbols.as_ref())
            .is_some_and(|exported| exported.find(name.as_bytes()) != -1)
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
    for_each_unwind_edge(ctx, id, |target, _| f(target));
}

/// Calls `f` with the LSDAs and the personality routine (and the symbol
/// naming it) of subsection `id`'s compact-unwind records.
#[inline]
fn for_each_unwind_edge<E: Target>(
    ctx: &Context<E>,
    id: usize,
    mut f: impl FnMut(usize, Option<SymbolId>),
) {
    let isec = &ctx.isecs[id];
    let recs = isec.unwind_offset as usize..(isec.unwind_offset + isec.nunwind) as usize;
    for rec in &ctx.unwind_records[recs] {
        if let Some((lsda, _)) = rec.lsda() {
            f(lsda, None);
        }
        let mut personality = rec.personality();
        if let Some(fde) = rec.fde() {
            if let Some((lsda, _)) = ctx.fdes[fde].lsda {
                f(lsda as usize, None);
            }
            personality = personality.or(ctx.cies[ctx.fdes[fde].cie as usize].personality);
        }
        if let Some(p) = personality
            && let Some(target) = ctx.symbols[p].input_section()
        {
            f(target as usize, Some(p));
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

/// The serial walk from the marked sections on `stack`, for what a
/// live-support atom keeps.
fn walk<E: Target>(ctx: &Context<E>, redirects: &[usize], mut stack: Vec<usize>) {
    while let Some(id) = stack.pop() {
        for_each_edge(ctx, id, |target| {
            let target = redirects[target];
            if ctx.isecs[target].mark_visited() {
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
fn mark_live_support<E: Target>(ctx: &Context<E>, redirects: &[usize]) {
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
            walk(ctx, redirects, vec![id]);
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
fn mark_live_references<E: Target>(ctx: &mut Context<E>) {
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
    for &func in &ctx.init_offsets.init_funcs {
        if let InitFunc::Imported(id) = func {
            ctx.symbols[id].mark();
        }
    }
    for name in ctx
        .args
        .forced_undefined
        .iter()
        .chain(ctx.args.has_entry_point().then_some(&ctx.args.entry))
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

/// An atom of ld-prime's, for -why_live: a subsection, a symbol in a
/// section of an object without MH_SUBSECTIONS_VIA_SYMBOLS that does
/// not name the section, a dylib's symbol, or one of the linker's own
/// (those of a "boundary-file"). Such a section is one atom, named by
/// the symbol at its start, and each other symbol in it an atom of its
/// own that references that one.
#[derive(Clone, Copy)]
enum Atom {
    Isec(usize),
    Label(SymbolId),
    Import(SymbolId),
    Boundary(&'static str),
}

/// The names of the mach header, which ld-prime's boundary-file defines
/// as atoms that reference the start of __TEXT (itself an atom there).
const HEADER_NAMES: [&str; 5] = [
    "__mh_execute_header",
    "__mh_dylib_header",
    "__mh_bundle_header",
    "__mh_dylinker_header",
    "___dso_handle",
];
const TEXT_START: &str = "segment$start$__TEXT";

/// A step of the walk: an atom, what it references, how many of those
/// have been followed, and whether reports are off (see walk_from).
struct Frame {
    atom: Atom,
    edges: Vec<(Atom, bool)>,
    next: usize,
    quiet: bool,
}

/// -why_live prints, for each atom whose name matches a -why_live
/// pattern ("*" wildcards), every chain of references that keeps it
/// alive, on stderr, as ld-prime does. Its walk goes from each root in
/// turn - the -u symbols and the entry point, then every root atom in
/// input order - and when a reference reaches a matching atom, prints
/// "name from file" and the referencing atoms back to the root, one to
/// a line and indented a step further each; for a root itself, why it
/// is one. An atom already reached is not walked again. An initializer
/// pointer's atom is a "mod-init-ptr", and an arm64 assembler label
/// (ltmpN) that is an atom of its own goes by "none" in a chain. Only
/// meaningful under -dead_strip, like ld64's option of the same name.
struct WhyLive<'a, E: Target> {
    ctx: &'a Context<E>,
    redirects: &'a [usize],
    /// The symbol naming each subsection's atom, if one does.
    names: Vec<Option<SymbolId>>,
    /// Why each subsection is a root for a symbol it defines.
    roots: Vec<Option<Root>>,
    /// The other symbols of each section of an object without
    /// subsections, in address order.
    labels: HashMap<usize, Vec<SymbolId>>,
    /// The label and import atoms reached so far.
    live_syms: HashSet<SymbolId>,
    live_boundaries: HashSet<&'static str>,
    /// The file each import a dylib merged from a library it re-exports
    /// comes from (libSystem's from its libsystem_* stubs).
    providers: HashMap<SymbolId, &'a std::path::Path>,
}

impl<'a, E: Target> WhyLive<'a, E> {
    fn new(ctx: &'a Context<E>, redirects: &'a [usize]) -> Self {
        let mut names: Vec<Option<SymbolId>> = vec![None; ctx.isecs.len()];
        let mut roots: Vec<Option<Root>> = vec![None; ctx.isecs.len()];
        let mut labels: HashMap<usize, Vec<SymbolId>> = HashMap::new();
        // A name at the start: an extern one first, an assembler label
        // last.
        let rank = |sym: &Symbol| (sym.name().starts_with("ltmp"), !sym.is_extern());
        for (id, sym) in ctx.symbols.syms.iter().enumerate() {
            let id = id as SymbolId;
            let Some(isec) = sym.input_section().map(|i| redirects[i as usize]) else {
                continue;
            };
            if roots[isec].is_none() {
                roots[isec] = symbol_root(ctx, sym);
            }
            if !matches!(sym.file(), Some(FileId::Obj(_))) || sym.name().is_empty() {
                continue;
            }
            if !ctx.objs[ctx.isecs[isec].file as usize].subsections_via_symbols {
                labels.entry(isec).or_default().push(id);
            }
            if sym.value == 0 && names[isec].is_none_or(|name| rank(sym) < rank(&ctx.symbols[name]))
            {
                names[isec] = Some(id);
            }
        }
        for (&isec, syms) in &mut labels {
            syms.retain(|&id| Some(id) != names[isec]);
            syms.sort_by_key(|&id| (ctx.symbols[id].value, rank(&ctx.symbols[id]).0));
        }
        let mut by_name: HashMap<&str, &std::path::Path> = HashMap::new();
        for dylib in &ctx.dylibs {
            for file in &dylib.merged_files {
                for &name in &file.exports {
                    by_name.entry(name).or_insert(&file.path);
                }
            }
        }
        let providers = (0..ctx.symbols.syms.len() as SymbolId)
            .filter(|&id| matches!(ctx.symbols[id].file(), Some(FileId::Dylib(_))))
            .filter_map(|id| Some((id, *by_name.get(ctx.symbols[id].name())?)))
            .collect();
        let (live_syms, live_boundaries) = (HashSet::new(), HashSet::new());
        Self { ctx, redirects, names, roots, labels, live_syms, live_boundaries, providers }
    }

    /// Walks from every root, marking what it reaches live.
    fn walk(&mut self) {
        let ctx = self.ctx;
        for id in initial_undefines(ctx) {
            if let Some(atom) = self.atom_of(id) {
                self.walk_from(atom, Root::InitialUndef);
            }
        }
        for (id, isec) in ctx.isecs.iter().enumerate() {
            if !ctx.objs[isec.file as usize].is_alive {
                continue;
            }
            // An initializer pointer ld-prime keeps, even converted to
            // __init_offsets (which leaves its section dead).
            let init_pointers = ctx.hdr_of(isec).section_type() == S_MOD_INIT_FUNC_POINTERS;
            if !isec.is_alive() && !init_pointers {
                continue;
            }
            let keep = should_keep(ctx, isec) || init_pointers;
            let why = match self.roots[id] {
                _ if keep => Root::DontDeadStrip,
                Some(why) if self.redirects[id] == id => why,
                _ => continue,
            };
            let id = self.redirects[id];
            self.walk_from(Atom::Isec(id), why);
            if keep && let Some(labels) = self.labels.get(&id) {
                for label in labels.clone() {
                    self.walk_from(Atom::Label(label), Root::DontDeadStrip);
                }
            }
        }
        // An executable's mach header is a root, last.
        if ctx.args.output_type == MH_EXECUTE && !ctx.args.preload {
            self.walk_from(Atom::Boundary(HEADER_NAMES[0]), Root::DontDeadStrip);
        }
    }

    /// The atom a symbol names, if a file or the linker defines it.
    fn atom_of(&self, sym: SymbolId) -> Option<Atom> {
        let symbol = &self.ctx.symbols[sym];
        if matches!(symbol.file(), Some(FileId::Dylib(_))) {
            return Some(Atom::Import(sym));
        }
        let Some(isec) = symbol.input_section() else {
            let name = HEADER_NAMES.into_iter().find(|&name| name == symbol.name())?;
            return symbol.is_defined().then_some(Atom::Boundary(name));
        };
        let isec = self.redirects[isec as usize];
        let whole = !self.ctx.objs[self.ctx.isecs[isec].file as usize].subsections_via_symbols;
        if whole && self.names[isec] != Some(sym) {
            Some(Atom::Label(sym))
        } else {
            Some(Atom::Isec(isec))
        }
    }

    /// Marks an atom live, and says whether it was not before.
    fn mark(&mut self, atom: Atom) -> bool {
        match atom {
            Atom::Isec(id) => self.ctx.isecs[id].mark_visited(),
            Atom::Label(sym) | Atom::Import(sym) => self.live_syms.insert(sym),
            Atom::Boundary(name) => self.live_boundaries.insert(name),
        }
    }

    /// What an atom references, in the order of the places it does (a
    /// label, its section's atom), and whether ld-prime reports the
    /// reference. It follows an arm64 instruction pair referring to a
    /// symbol by its ADRP alone, and reports no LSDA or personality
    /// routine; those references come last, unreported, so that the
    /// walk marks all mold's does.
    fn edges(&self, atom: Atom) -> Vec<(Atom, bool)> {
        let ctx = self.ctx;
        let id = match atom {
            Atom::Label(sym) => {
                let isec = ctx.symbols[sym].input_section().unwrap() as usize;
                return vec![(Atom::Isec(self.redirects[isec]), true)];
            }
            Atom::Import(_) | Atom::Boundary(TEXT_START) => return Vec::new(),
            Atom::Boundary(_) => return vec![(Atom::Boundary(TEXT_START), true)],
            Atom::Isec(id) => id,
        };
        let file = &ctx.objs[ctx.isecs[id].file as usize];
        let mut relocs: Vec<_> = ctx.isec_relocs(id).iter().collect();
        let second = |rel: &Reloc| {
            E::CPUTYPE == CPU_TYPE_ARM64
                && matches!(
                    rel.r_type,
                    ARM64_RELOC_PAGEOFF12
                        | ARM64_RELOC_GOT_LOAD_PAGEOFF12
                        | ARM64_RELOC_TLVP_LOAD_PAGEOFF12
                )
        };
        relocs.sort_by_key(|rel| (second(rel), rel.offset));
        let mut edges = Vec::new();
        for rel in relocs {
            let atom = match rel.target() {
                RelocTarget::Sym(idx) => self.atom_of(file.symbols[idx as usize]),
                RelocTarget::Section(target) => Some(Atom::Isec(self.redirects[target as usize])),
            };
            edges.extend(atom.map(|atom| (atom, !second(rel))));
        }
        for_each_unwind_edge(ctx, id, |target, sym| {
            let atom = match sym {
                Some(sym) => self.atom_of(sym),
                None => Some(Atom::Isec(self.redirects[target])),
            };
            edges.extend(atom.map(|atom| (atom, false)));
        });
        edges
    }

    /// Walks from a root, depth first as ld-prime does. What only an
    /// unreported reference reaches is reported on no further.
    fn walk_from(&mut self, root: Atom, why: Root) {
        self.report(root, &[], Some(why));
        if !self.mark(root) {
            return;
        }
        let mut stack = vec![Frame { atom: root, edges: self.edges(root), next: 0, quiet: false }];
        while let Some(frame) = stack.last_mut() {
            let Some(&(target, reported)) = frame.edges.get(frame.next) else {
                stack.pop();
                continue;
            };
            frame.next += 1;
            let quiet = frame.quiet || !reported;
            if !quiet {
                self.report(target, &stack, None);
            }
            if self.mark(target) {
                let edges = self.edges(target);
                stack.push(Frame { atom: target, edges, next: 0, quiet });
            }
        }
    }

    /// Prints the chain that reaches an atom if the atom matches.
    fn report(&self, atom: Atom, stack: &[Frame], why: Option<Root>) {
        let Some(name) = self.name(atom, false) else { return };
        if self.ctx.args.why_live.find(name.as_bytes()) == -1 {
            return;
        }
        crate::error::notice(format_args!("{}", self.describe(atom, name)));
        if let Some(why) = why {
            crate::error::notice(format_args!("  {}", why.name()));
        }
        for (depth, frame) in stack.iter().rev().enumerate() {
            let referrer = frame.atom;
            let name =
                self.name(referrer, true).map_or_else(|| self.section_name(referrer), String::from);
            crate::error::notice(format_args!(
                "{:indent$}{}",
                "",
                self.describe(referrer, &name),
                indent = depth * 2 + 2
            ));
        }
    }

    /// An atom's name, if it has one, as a match or in a chain: an
    /// initializer pointer is a "mod-init-ptr" there, and an assembler
    /// label that is an atom of its own "none". Literals have none.
    fn name(&self, atom: Atom, in_chain: bool) -> Option<&str> {
        let ctx = self.ctx;
        match atom {
            Atom::Label(sym) => {
                let name = ctx.symbols[sym].name();
                Some(if in_chain && name.starts_with("ltmp") { "none" } else { name })
            }
            Atom::Import(sym) => Some(ctx.symbols[sym].name()),
            Atom::Boundary(name) => Some(name),
            Atom::Isec(id) => {
                let hdr = ctx.hdr_of(&ctx.isecs[id]);
                if in_chain && hdr.section_type() == S_MOD_INIT_FUNC_POINTERS {
                    return Some("mod-init-ptr");
                }
                if is_literal_section(hdr) {
                    return None;
                }
                self.names[id].map(|sym| ctx.symbols[sym].name())
            }
        }
    }

    fn section_name(&self, atom: Atom) -> String {
        let Atom::Isec(id) = atom else { unreachable!() };
        let hdr = self.ctx.hdr_of(&self.ctx.isecs[id]);
        format!("{},{}", hdr.segname(), hdr.sectname())
    }

    /// "name from file", but the linker's own sections have no file. A
    /// dylib goes by its real path, as ld-prime reports the files it
    /// loads.
    fn describe(&self, atom: Atom, name: &str) -> String {
        let ctx = self.ctx;
        let isec = match atom {
            Atom::Isec(id) => id,
            Atom::Label(sym) => ctx.symbols[sym].input_section().unwrap() as usize,
            Atom::Boundary(_) => return format!("{name} from boundary-file"),
            Atom::Import(sym) => {
                let Some(FileId::Dylib(i)) = ctx.symbols[sym].file() else { unreachable!() };
                let path = self.providers.get(&sym).copied();
                let path = path.unwrap_or(&ctx.dylibs[i as usize].path);
                let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
                return format!("{name} from {}", real.display());
            }
        };
        let file = ctx.isecs[isec].file as usize;
        if ctx.is_internal(file) {
            return name.to_string();
        }
        format!("{name} from {}", crate::passes::resolved_file_name(ctx.objs[file].mf))
    }
}
