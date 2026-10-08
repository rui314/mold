//! Dead-stripping (-dead_strip): mold's gc-sections for Mach-O.
//!
//! Subsections not reachable from the roots - the entry point,
//! exported symbols (for a dylib), initializers and everything the
//! format pins (no-dead-strip sections and symbols) - are removed.
//! Reachability follows relocations and unwind-info edges, so a live
//! function keeps its LSDA and personality. This is passes.rs's
//! liveness walk's section-level counterpart, and mirrors
//! gc_sections.rs in mold (dead-strip.cc in sold).

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};

use rayon::prelude::*;

use crate::arch::Target;
use crate::chunks::init_offsets::InitFunc;
use crate::context::Context;
use crate::error::{RawPath, notice, raw};
use crate::input_files::{FileId, is_literal_section};
use crate::input_sections::{InputSection, RelocTarget};
use crate::macho::*;
use crate::symbol::{Symbol, SymbolId};

/// Strips dead code (see Context::strips_dead_code) and refreshes which
/// symbols live code uses.
pub fn strip_dead_code<E: Target>(ctx: &mut Context<E>) {
    dead_strip(ctx);
    mark_live_references(ctx);
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
    // walk run on all cores; -why_live, which wants to know what made
    // each subsection live, walks serially instead.
    let mut why = (!ctx.args.why_live.is_empty()).then(|| vec![Why::Dead; ctx.isecs.len()]);
    let is_root = |_, sym: &Symbol| is_symbol_root(ctx, sym);
    let roots = collect_root_set(ctx, redirects, is_root, why.as_deref_mut());
    match why.as_deref_mut() {
        None => mark(ctx, redirects, &roots),
        Some(why) => walk(ctx, redirects, roots, Some(why)),
    }

    mark_live_support(ctx, redirects, why.as_deref_mut());
    sweep(ctx);
    if let Some(why) = &why {
        print_why_live(ctx, redirects, why);
    }
}

/// Why a subsection is a dead-strip root, as -why_live says.
#[derive(Clone, Copy)]
enum Root {
    /// It defines the entry point, a -u symbol or an -alias base.
    Undefined,
    /// It defines an exported symbol.
    Export,
    /// The format keeps it (see should_keep), or a no-dead-strip
    /// symbol it defines.
    Kept,
    /// An initializer __init_offsets runs.
    Initializer,
    /// A live-support subsection referring to live code (see
    /// mark_live_support).
    LiveSupport,
}

impl Root {
    fn name(self) -> &'static str {
        match self {
            Root::Undefined => "root: the entry point, -u or -alias",
            Root::Export => "root: exported",
            Root::Kept => "root: never dead-stripped",
            Root::Initializer => "root: an initializer",
            Root::LiveSupport => "root: live support of live code",
        }
    }
}

/// What made a subsection live, for -why_live: nothing yet, a root's
/// reason, or the subsection whose reference first reached it.
#[derive(Clone, Copy)]
enum Why {
    Dead,
    Root(Root),
    From(u32),
}

/// Whether a symbol makes its subsection a root: a no-dead-strip
/// symbol, or an export dead stripping keeps.
fn is_symbol_root<E: Target>(ctx: &Context<E>, sym: &Symbol) -> bool {
    sym.no_dead_strip()
        || (sym.is_extern()
            && !sym.is_private_extern()
            && sym.is_defined()
            && keeps_export(ctx, sym.name()))
}

/// The symbols the command line names, which the link must define and
/// dead stripping keeps: the -u ones (and those the options that add to
/// them name), the entry point and the bases of -alias - ld-prime's
/// initial undefines.
pub(crate) fn initial_undefines<E: Target>(ctx: &Context<E>) -> impl Iterator<Item = SymbolId> {
    let entry = ctx.args.has_entry_point().then_some(&ctx.args.entry);
    let aliased = ctx.args.aliases.iter().map(|(base, _)| base);
    ctx.args
        .forced_undefined
        .iter()
        .chain(entry)
        .chain(aliased)
        .filter_map(|name| ctx.symbols.lookup(name))
}

/// Sections the format keeps regardless of references: initializers
/// and terminators, and no-dead-strip sections (the ObjC image info
/// has no subsections: the link makes its own, see
/// input_files::is_objc_image_info; an __objc_imageinfo of another
/// segment than __DATA is any section to ld-prime, which strips it). So
/// is every section of an object without MH_SUBSECTIONS_VIA_SYMBOLS
/// that ld64 splits at symbols: it cannot tell where such a subsection
/// ends, so it treats the object as one huge subsection. Sections it
/// splits by content (literals, CFStrings, class references,
/// thread-local variable descriptors) are stripped as usual. ld-prime
/// strips a class reference nothing uses although clang marks
/// __objc_classrefs no-dead-strip (it keeps unused selector
/// references).
fn should_keep<E: Target>(ctx: &Context<E>, isec: &InputSection) -> bool {
    let hdr = ctx.hdr_of(isec);
    matches!(hdr.section_type(), S_MOD_INIT_FUNC_POINTERS | S_MOD_TERM_FUNC_POINTERS)
        || hdr.section_type() == S_INIT_FUNC_OFFSETS
        || (hdr.flags & S_ATTR_NO_DEAD_STRIP != 0
            && !(hdr.segname() == b"__DATA" && hdr.sectname() == b"__objc_classrefs"))
        || (!ctx.objs[isec.file as usize].subsections_via_symbols
            && !is_literal_section(hdr)
            && hdr.section_type() != S_THREAD_LOCAL_VARIABLES)
}

/// Marks the roots and returns them: the sections the format keeps,
/// those defining a symbol `is_root` takes (a no-dead-strip one or an
/// export), and those of the initializers and initial undefines. They
/// are found on all cores; marking stays serial (it is a handful of
/// sections). Under -why_live, `why` gets each root's reason.
fn collect_root_set<E: Target>(
    ctx: &Context<E>,
    redirects: &[usize],
    is_root: impl Fn(SymbolId, &Symbol) -> bool + Sync,
    mut why: Option<&mut [Why]>,
) -> Vec<usize> {
    let mut roots = Vec::new();
    // Liveness is marked in place, on the section's atomic visited bit
    // (mold's IS_VISITED), rather than in side arrays copied back at
    // the end.
    let mut enqueue = |id: usize, root: Root| {
        let id = redirects[id];
        if ctx.isecs[id].mark_visited() {
            roots.push(id);
            if let Some(why) = why.as_deref_mut() {
                why[id] = Why::Root(root);
            }
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
        enqueue(id, Root::Kept);
    }

    // Initializers converted to __init_offsets are roots; their source
    // sections are gone.
    for &func in &ctx.init_offsets.init_funcs {
        if let InitFunc::Local(isec, _) = func {
            enqueue(isec, Root::Initializer);
        }
    }

    // Sections defining a no-dead-strip or an exported symbol - in the
    // link: an archive member left unloaded keeps its local symbols
    // (`__attribute__((used))` ones are no-dead-strip), but none of its
    // sections.
    let syms: Vec<(usize, Root)> = ctx
        .symbols
        .syms
        .par_iter()
        .enumerate()
        .filter(|&(id, sym)| is_root(id as SymbolId, sym))
        .filter_map(|(_, sym)| {
            let root = if sym.no_dead_strip() { Root::Kept } else { Root::Export };
            let isec = sym.input_section()? as usize;
            ctx.isecs[isec].is_alive().then_some((isec, root))
        })
        .collect();
    for (id, root) in syms {
        enqueue(id, root);
    }

    // -u retains the symbol's subsection as well as extracting its
    // containing archive member. It may name a private external,
    // unlike an export root.
    for id in initial_undefines(ctx) {
        if let Some(isec) = ctx.symbols[id].input_section() {
            enqueue(isec as usize, Root::Undefined);
        }
    }

    // Legacy LINKEDIT's stub helper entries jump to crt1.o's
    // dyld_stub_binding_helper, which ld-prime keeps wherever imports
    // bind lazily, whether any stub needs it or not.
    if ctx.args.legacy_linkedit
        && ctx.args.lazy_binding
        && let Some(id) = ctx.symbols.lookup(b"dyld_stub_binding_helper")
        && let Some(isec) = ctx.symbols[id].input_section()
    {
        enqueue(isec as usize, Root::Kept);
    }
    roots
}

/// The symbols live Mach-O code refers to before LTO, as ld-prime finds
/// them to tell libLTO what to preserve: it walks the subsections as
/// dead stripping does - in any link with bitcode, -dead_strip or not -
/// from the same roots, each bitcode file's "internal" node among them,
/// which stands for its module's code and refers to every symbol the
/// module does. A bitcode definition is a node that refers to that
/// one. So native code that only bitcode, a root or another such
/// function reaches counts, and code nothing does (a hidden helper no
/// one calls) doesn't. The export lists have hidden nothing yet; the
/// roots go by them, as `exported` says (see lto::exported_before_lto).
pub fn native_refs_before_lto<E: Target>(
    ctx: &Context<E>,
    exported: impl Fn(SymbolId) -> bool + Sync,
) -> Vec<AtomicBool> {
    let redirects: Vec<usize> =
        (0..ctx.isecs.len()).into_par_iter().map(|i| ctx.resolve_isec(i)).collect();
    let is_root = |id: SymbolId, sym: &Symbol| sym.no_dead_strip() || exported(id);
    let mut roots = collect_root_set(ctx, &redirects, is_root, None);
    for module in &ctx.lto_modules {
        let obj = &ctx.objs[module.obj];
        if !obj.is_alive {
            continue;
        }
        for (msym, &id) in obj.mach_syms.iter().zip(&obj.symbols) {
            if msym.ty() == N_UNDF
                && let Some(isec) = ctx.symbols[id].input_section()
                && ctx.isecs[redirects[isec as usize]].mark_visited()
            {
                roots.push(redirects[isec as usize]);
            }
        }
    }
    mark(ctx, &redirects, &roots);
    mark_live_support(ctx, &redirects, None);

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

/// Whether dead stripping keeps an export named `name`: every one of a
/// dylib or bundle, and of an executable those an export list names, or
/// all its globals with -export_dynamic (but not a -preload image's,
/// which ld-prime still strips).
pub fn keeps_export<E: Target>(ctx: &Context<E>, name: &[u8]) -> bool {
    ctx.args.output_type != MH_EXECUTE
        || (ctx.args.export_dynamic && !ctx.args.preload)
        || (ctx.args.exported_symbols.as_ref()).is_some_and(|exported| exported.find(name) != -1)
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

/// The serial walk from the marked sections in `queue`, breadth first:
/// for what a live-support subsection keeps, and under -why_live for
/// everything, noting in `why` the subsection that first reaches each
/// one.
fn walk<E: Target>(
    ctx: &Context<E>,
    redirects: &[usize],
    queue: Vec<usize>,
    mut why: Option<&mut [Why]>,
) {
    let mut queue = VecDeque::from(queue);
    while let Some(id) = queue.pop_front() {
        for_each_edge(ctx, id, |target| {
            let target = redirects[target];
            if ctx.isecs[target].mark_visited() {
                if let Some(why) = why.as_deref_mut() {
                    why[target] = Why::From(id as u32);
                }
                queue.push_back(target);
            }
        });
    }
}

/// A live-support subsection (S_ATTR_LIVE_SUPPORT) lives only if it
/// references a live subsection, and then keeps what it references.
/// ld64 checks them once, in input order, after what the roots reach is
/// marked, so one that only a later live-support subsection would make
/// live stays dead.
fn mark_live_support<E: Target>(
    ctx: &Context<E>,
    redirects: &[usize],
    mut why: Option<&mut [Why]>,
) {
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
            if let Some(why) = why.as_deref_mut() {
                why[id] = Why::Root(Root::LiveSupport);
            }
            walk(ctx, redirects, vec![id], why.as_deref_mut());
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

    crate::input_files::remove_dead_unwind_info(ctx);
}

/// Refresh symbol usage after subsection liveness is known. Undefined
/// references in removed subsections must neither cause errors nor
/// become dynamic imports.
fn mark_live_references<E: Target>(ctx: &mut Context<E>) {
    ctx.symbols.syms.par_iter().for_each(|sym| sym.unmark());
    ctx.isecs.par_iter().filter(|isec| isec.is_emitted()).for_each(|isec| {
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
    crate::objc::drop_dead_objc_stubs(ctx);
    if let Some(id) = ctx.objc_stubs.msgsend_sym {
        ctx.symbols[id].mark();
    }
    for &func in &ctx.init_offsets.init_funcs {
        if let InitFunc::Imported(id) = func {
            ctx.symbols[id].mark();
        }
    }
    for id in initial_undefines(ctx) {
        ctx.symbols[id].mark();
    }
    ctx.symbols.syms.par_iter_mut().for_each(|sym| {
        sym.set_used(sym.is_marked());
        sym.unmark();
    });
}

/// -why_live prints, on stderr, for each live symbol whose name matches
/// a -why_live pattern ("*" wildcards), what keeps it alive: the symbol
/// and its file, then the subsection whose reference made its subsection
/// live, and that one's, each a step further indented, up to a root and
/// why it is one. An import is kept by the first live subsection that
/// refers to it. Only meaningful under -dead_strip, like ld64's option
/// of the same name.
fn print_why_live<E: Target>(ctx: &Context<E>, redirects: &[usize], why: &[Why]) {
    let matches = |sym: &Symbol| ctx.args.why_live.find(sym.name()) != -1;
    let print_chain = |mut step: Why| {
        for depth in 1.. {
            let indent = depth * 2;
            match step {
                Why::From(id) => {
                    let isec = &ctx.isecs[id as usize];
                    let name = ctx.subsec_name(id as usize);
                    let file = ctx.objs[isec.file as usize].mf.name.raw();
                    notice(format_args!("{:indent$}{} from {file}", "", raw(&name)));
                    step = why[id as usize];
                }
                Why::Root(root) => {
                    notice(format_args!("{:indent$}{}", "", root.name()));
                    return;
                }
                Why::Dead => return,
            }
        }
    };

    for (i, obj) in ctx.objs.iter().enumerate().filter(|(_, obj)| obj.is_alive) {
        for &id in &obj.symbols {
            let sym = &ctx.symbols[id];
            if sym.file() != Some(FileId::Obj(i as u32)) || !matches(sym) {
                continue;
            }
            let Some(isec) = sym.input_section().map(|isec| redirects[isec as usize]) else {
                continue;
            };
            if ctx.isecs[isec].is_alive() {
                notice(format_args!("{} from {}", raw(sym.name()), obj.mf.name.raw()));
                print_chain(why[isec]);
            }
        }
    }

    let mut seen = HashSet::new();
    for (id, isec) in ctx.isecs.iter().enumerate().filter(|(_, isec)| isec.is_alive()) {
        for rel in ctx.isec_relocs(id) {
            let Some(sym_id) = ctx.reloc_target_sym(isec.file as usize, rel) else { continue };
            let sym = &ctx.symbols[sym_id];
            let Some(FileId::Dylib(dylib)) = sym.file() else { continue };
            if dylib != u32::MAX && matches(sym) && seen.insert(sym_id) {
                let file = ctx.dylibs[dylib as usize].path.raw();
                notice(format_args!("{} from {file}", raw(sym.name())));
                print_chain(Why::From(id as u32));
            }
        }
    }
}
