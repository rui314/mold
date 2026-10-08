//! The linker passes, in the order the driver runs them.

use std::ops::Range;
use std::path::Path;

use portable_atomic::AtomicU64;
use rayon::prelude::*;

use crate::arch::Target;
use crate::chunks::init_offsets::InitFunc;
use crate::chunks::{self, ChunkHeader, ChunkId, OutputSegment, mach_header_size};
use crate::cmdline::Treatment;
use crate::context::Context;
use crate::error;
use crate::error::RawPath;
use crate::error::raw;
use crate::fatal;
use crate::input_files;
use crate::input_files::{FileId, ObjcImageInfo, SymbolSlots};
use crate::input_sections::{InputSection, NO_REPLACEMENT, RelocTarget};
use crate::macho::*;
use crate::mapped_file::MappedFile;
use crate::output_sections::header_segment;
use crate::symbol::{NEEDS_GOT, NEEDS_STUB, Symbol, SymbolId};
use crate::util::{align_to, path_bytes, split_once};

/// Adds the object that owns what the linker synthesizes: the
/// sections standing for merged Objective-C records, folded class
/// references or tentative definitions, and symbols such as
/// __mh_execute_header. It takes part in every pass like an input
/// object with no symbol table of its own, so no pass has to treat
/// synthesized sections and symbols as fileless. mold's
/// create_internal_file.
pub fn create_internal_file<E: Target>(ctx: &mut Context<E>) {
    ctx.internal_obj = Some(ctx.objs.len());
    ctx.objs.push(input_files::ObjectFile::internal());
}

/// Resolves all symbols, following mold's model: every input including
/// each archive member has been parsed already, and resolution ranks
/// competing definitions (strong > weak > a lazy archive member's or a
/// dylib's strong > their weak > common), breaking ties by input order
/// (see ObjectFile::definition_rank). A liveness walk then marks the
/// archive members whose definitions are actually referenced (see
/// mark_live_objects), and the live objects' auto-link options load the
/// libraries they name; objects among those, or a library that changes
/// what an earlier one stands for, have resolution and the walk start
/// over. A final round restricted to live files settles the owners, as
/// in mold's resolve_symbols.
pub fn resolve_symbols<E: Target>(ctx: &mut Context<E>) {
    intern_command_line_symbols(ctx);
    // The final round ranks the dylibs as they were before the last
    // auto-linked ones came: those claim only what is left undefined
    // (see claim_new_dylibs).
    let (ranking, num_dylibs) = loop {
        clear_symbols(ctx);
        let ranking = DylibRanking::new(&ctx.dylibs);
        resolve_symbols_pass(ctx, false, &ranking);
        mark_live_objects(ctx);
        let num_dylibs = ctx.dylibs.len();
        if !crate::reader::load_autolink_deps(ctx) {
            break (ranking, num_dylibs);
        }
    };

    // Now that we know the exact set of input files that are to be
    // included in the output file, redo symbol resolution.
    clear_symbols(ctx);
    resolve_symbols_pass(ctx, true, &ranking);
    claim_locals(ctx);
    if ctx.dylibs.len() > num_dylibs {
        claim_new_dylibs(ctx, num_dylibs);
    }
}

/// Symbols the command line names exist even when no object mentions
/// them, so that a dylib export can claim them: an app extension's
/// entry point, _NSExtensionMain, lives in Foundation and nothing in the
/// extension references it.
fn intern_command_line_symbols<E: Target>(ctx: &mut Context<E>) {
    let new: Vec<&'static [u8]> = ctx
        .args
        .command_line_symbols()
        .filter(|name| ctx.symbols.get(name).is_none())
        .map(|name| crate::util::leak_bytes(name.to_vec()))
        .collect();
    for name in new {
        ctx.symbols.intern(name);
    }
}

/// Non-external symbols are private to their object and never compete:
/// each gets its definition directly. Relocations reference them by
/// symbol index just like externals, so they need locations too.
fn claim_locals<E: Target>(ctx: &mut Context<E>) {
    let _t = ctx.timer("claim_locals");
    // A local symbol belongs to exactly one object (locals get fresh
    // slots, never interned), so the per-object claims write disjoint
    // symbols and the objects proceed in parallel.
    let syms = SymbolSlots::new(&mut ctx.symbols.syms);
    let isecs = &ctx.isecs;
    ctx.objs.par_iter().enumerate().for_each(|(obj_idx, obj)| {
        for i in obj.local_range() {
            let msym = &obj.mach_syms[i];
            if msym.is_stab() || msym.is_extern() {
                continue;
            }
            // SAFETY: disjoint per object, as above.
            let sym = unsafe { syms.get(obj.symbols[i]) };
            let file = FileId::Obj(obj_idx as u32);
            match msym.ty() {
                N_ABS => {
                    sym.set_file(file);
                    sym.set_input_section(None);
                    sym.value = msym.value;
                }
                N_SECT => {
                    if let Some((isec, off)) = obj.symbol_subsec(isecs, i) {
                        sym.set_file(file);
                        sym.set_input_section(Some(isec as u32));
                        sym.value = off;
                        sym.set_no_dead_strip(
                            msym.desc & (N_NO_DEAD_STRIP | REFERENCED_DYNAMICALLY) != 0,
                        );
                        sym.set_is_alt_entry(msym.desc & N_ALT_ENTRY != 0);
                    }
                }
                _ => {}
            }
        }
    });
}

/// Resets the resolution of every symbol a file claimed, and of every
/// common one, for a resolution round to start over. Which imports are
/// weak is decided afresh too: the final round counts only the live
/// files' references, as ld-prime ignores those of an archive member it
/// doesn't load (a strong one there would make a weak import strong).
fn clear_symbols<E: Target>(ctx: &mut Context<E>) {
    let _t = ctx.timer("clear_symbols");
    ctx.symbols.syms.par_iter_mut().for_each(|sym| {
        sym.set_is_weak_ref(false);
        sym.set_is_strong_ref(false);
        if matches!(sym.file(), Some(FileId::Obj(_)) | Some(FileId::Dylib(_))) || sym.is_common() {
            sym.clear_file();
            sym.set_input_section(None);
            sym.value = 0;
            sym.set_is_weak_def(false);
            sym.set_is_private_extern(false);
            sym.set_is_imported(false);
            sym.set_is_common(false);
            sym.common_p2align = 0;
            sym.set_no_dead_strip(false);
            sym.set_is_referenced_dynamically(false);
            sym.set_is_alt_entry(false);
        }
    });
}

/// One resolution round over the objects - all of them, or with
/// `only_alive` just the live ones: definitions race for each symbol by
/// rank and the winners claim it, common symbols merge, and dylib
/// exports claim what the objects leave undefined, the dylibs ranked as
/// `ranking` has them.
fn resolve_symbols_pass<E: Target>(ctx: &mut Context<E>, only_alive: bool, ranking: &DylibRanking) {
    use std::sync::atomic::Ordering;
    let _t = ctx.timer("resolve_symbols_pass");

    let refs = collect_references(ctx, only_alive);
    let commons = live_common_symbols(ctx);
    let best = race_definitions(ctx, only_alive);
    claim_definitions(ctx, only_alive, &best);
    merge_common_symbols(ctx, &commons, &best);

    // Record the references seen this round. A symbol is a weak import
    // only if every reference to it is weak: one strong reference
    // anywhere makes it strong (ld64's default, -weak_reference_
    // mismatches non-weak), and so binds it non-weakly and keeps its
    // dylib loaded non-weakly. -weak_reference_mismatches weak has one
    // weak reference make it weak instead, in a final image.
    let weak_wins = ctx.args.weak_reference_mismatches == crate::cmdline::WeakRefMismatches::Weak
        && !ctx.args.relocatable;
    ctx.symbols.syms.par_iter_mut().enumerate().for_each(|(i, sym)| {
        if weak_wins && refs.weak[i].load(Ordering::Relaxed) {
            sym.set_is_weak_ref(true);
        } else if refs.strong[i].load(Ordering::Relaxed) {
            sym.set_is_strong_ref(true);
            sym.set_is_weak_ref(false);
        } else if refs.weak[i].load(Ordering::Relaxed) && !sym.is_strong_ref() {
            sym.set_is_weak_ref(true);
        }
    });

    // A relocatable link keeps every reference undefined rather than
    // binding it to a dylib.
    if !ctx.args.relocatable {
        claim_dylib_exports(ctx, ranking, &refs.used, &best, &commons);
    }

    // Record the final usage set for downstream passes.
    let used = ctx.symbols.syms.par_iter_mut().zip(&refs.used);
    used.for_each(|(sym, used)| sym.set_is_used(used.load(Ordering::Relaxed)));
}

/// Which symbols the objects considered in a round reference, and how.
struct References {
    used: Vec<std::sync::atomic::AtomicBool>,
    weak: Vec<std::sync::atomic::AtomicBool>,
    strong: Vec<std::sync::atomic::AtomicBool>,
}

/// Which symbols the files considered this round actually reference.
/// References from dead archive members must not count: they would
/// otherwise demand definitions nothing live needs. What the command
/// line names (-u, -alias, and -e or the default _main of an image that
/// has an entry point) counts as referenced.
fn collect_references<E: Target>(ctx: &Context<E>, only_alive: bool) -> References {
    use std::sync::atomic::{AtomicBool, Ordering};
    let n = ctx.symbols.syms.len();
    let refs = References {
        used: (0..n).map(|_| AtomicBool::new(false)).collect(),
        weak: (0..n).map(|_| AtomicBool::new(false)).collect(),
        strong: (0..n).map(|_| AtomicBool::new(false)).collect(),
    };
    ctx.objs.par_iter().filter(|obj| !only_alive || obj.is_alive).for_each(|obj| {
        let r = obj.global_range();
        for (msym, &sym_id) in obj.mach_syms[r.clone()].iter().zip(&obj.symbols[r]) {
            if msym.is_undef() {
                refs.used[sym_id as usize].store(true, Ordering::Relaxed);
                if msym.desc & N_WEAK_REF != 0 {
                    refs.weak[sym_id as usize].store(true, Ordering::Relaxed);
                } else {
                    refs.strong[sym_id as usize].store(true, Ordering::Relaxed);
                }
            }
        }
    });

    for name in ctx.args.command_line_symbols() {
        if let Some(id) = ctx.symbols.get(name) {
            refs.used[id as usize].store(true, Ordering::Relaxed);
        }
    }
    // So are the classes the hook for those of mergeable libraries
    // binds to (see bundle_hook).
    for id in ctx.bundle_hook.imports() {
        refs.used[id as usize].store(true, Ordering::Relaxed);
        refs.strong[id as usize].store(true, Ordering::Relaxed);
    }
    refs
}

/// The best definition rank of each symbol. Ranks race into it with an
/// atomic minimum, as in mold: the race is order-free because the
/// winner is the same whatever the interleaving, and since each object
/// has a unique priority, exactly one object ends up owning each
/// symbol.
fn race_definitions<E: Target>(ctx: &Context<E>, only_alive: bool) -> Vec<AtomicU64> {
    use std::sync::atomic::Ordering;
    let best: Vec<AtomicU64> =
        (0..ctx.symbols.syms.len()).map(|_| AtomicU64::new(u64::MAX)).collect();
    ctx.objs.par_iter().filter(|obj| !only_alive || obj.is_alive).for_each(|obj| {
        for i in obj.global_range() {
            let sym_id = obj.symbols[i];
            let rank = obj.definition_rank(&ctx.isecs, i, ctx.autolink_priority);
            if let Some(rank) = rank {
                best[sym_id as usize].fetch_min(rank, Ordering::Relaxed);
            }
        }
    });
    best
}

/// Each object writes the symbols whose race it won. Ranks are unique
/// per object, so every symbol has exactly one writer and the parallel
/// writes are disjoint.
fn claim_definitions<E: Target>(ctx: &mut Context<E>, only_alive: bool, best: &[AtomicU64]) {
    use std::sync::atomic::Ordering;
    let syms = SymbolSlots::new(&mut ctx.symbols.syms);
    let isecs = &ctx.isecs;
    let autolink_priority = ctx.autolink_priority;
    let objs = ctx.objs.par_iter().enumerate().filter(|(_, obj)| !only_alive || obj.is_alive);
    objs.for_each(|(obj_idx, obj)| {
        for i in obj.global_range() {
            let sym_id = obj.symbols[i];
            let rank = obj.definition_rank(isecs, i, autolink_priority);
            let Some(rank) = rank else { continue };
            if best[sym_id as usize].load(Ordering::Relaxed) != rank {
                continue;
            }
            // SAFETY: this object holds the unique minimum rank for
            // sym_id, so no other thread writes this slot.
            let sym = unsafe { syms.get(sym_id) };
            if !obj.claim_definition(sym, obj_idx, i, isecs) {
                best[sym_id as usize].store(u64::MAX, Ordering::Relaxed);
            }
        }
    });
}

/// The tentative definitions of live objects, in input order: (symbol,
/// size, log2 of the alignment, whether a private external).
fn live_common_symbols<E: Target>(ctx: &Context<E>) -> Vec<(SymbolId, u64, u8, bool)> {
    ctx.objs
        .par_iter()
        .filter(|obj| obj.is_alive)
        .flat_map_iter(|obj| {
            let r = obj.global_range();
            obj.mach_syms[r.clone()].iter().zip(&obj.symbols[r]).filter_map(|(msym, &sym_id)| {
                if !msym.is_stab() && msym.is_extern() && msym.ty() == N_UNDF && msym.is_common() {
                    let p2align = msym.common_p2align();
                    let pext = msym.n_type & N_PEXT != 0 || obj.hidden;
                    Some((sym_id, msym.value, p2align, pext))
                } else {
                    None
                }
            })
        })
        .collect()
}

/// Common symbols merge as in mold, from every common claim once the
/// class-4 winners are known: into the largest size and the greatest
/// alignment of them all, and a private external if any is one (mold's
/// most restrictive visibility), whatever the input order.
fn merge_common_symbols<E: Target>(
    ctx: &mut Context<E>,
    commons: &[(SymbolId, u64, u8, bool)],
    best: &[AtomicU64],
) {
    use std::sync::atomic::Ordering;
    for &(sym_id, size, p2align, pext) in commons {
        if best[sym_id as usize].load(Ordering::Relaxed) >> 40 != 4 {
            continue;
        }
        let sym = &mut ctx.symbols[sym_id];
        sym.value = sym.value.max(size);
        sym.common_p2align = sym.common_p2align.max(p2align);
        if pext {
            sym.set_is_private_extern(true);
        }
    }
}

/// Dylib exports claim the referenced symbols that no object defines,
/// or that only a lazy archive member does; as among archive members,
/// a strong definition beats a weak one, and of two equally strong an
/// earlier dylib beats a later archive member and vice versa (see
/// dylib_ranks). Of the dylibs `ranking` ranks that export a symbol,
/// the first in search order that exports it strong claims it, or if
/// none does, the first.
fn claim_dylib_exports<E: Target>(
    ctx: &mut Context<E>,
    ranking: &DylibRanking,
    used: &[std::sync::atomic::AtomicBool],
    best: &[AtomicU64],
    commons: &[(SymbolId, u64, u8, bool)],
) {
    use std::sync::atomic::Ordering;
    collect_dylib_symbols(ctx);
    let dylibs = &ctx.dylibs;
    let DylibRanking { ranks, providers } = ranking;
    let order = dylib_search_order(ranks, 0);
    let first = first_exporters(dylibs, &ctx.symbols.syms, &order);
    // A live tentative definition (a common symbol) hides a dylib's
    // definition but under -commons use_dylibs, though not an archive
    // member's real one, which beats it.
    let use_dylibs = ctx.args.commons == crate::cmdline::CommonsMode::UseDylibs;
    let live_commons: hashbrown::HashSet<SymbolId> =
        commons.iter().filter(|_| !use_dylibs).map(|c| c.0).collect();
    ctx.symbols.syms.par_iter_mut().enumerate().for_each(|(i, sym)| {
        let key = first[i].load(Ordering::Relaxed);
        if key == u32::MAX || !used[i].load(Ordering::Relaxed) {
            return;
        }
        let won = best[i].load(Ordering::Relaxed);
        if won >> 40 < 2 || live_commons.contains(&(i as SymbolId)) {
            return;
        }
        // A DTrace symbol binds to no dylib, whatever exports one (see
        // dtrace).
        if crate::dtrace::is_dtrace_symbol(sym.name()) {
            return;
        }
        let dylib_idx = order[(key & !WEAK_EXPORT) as usize];
        // A weak export ranks as a lazy member's weak definition does.
        let weak = if key & WEAK_EXPORT != 0 { 1 << 40 } else { 0 };
        if ranks[dylib_idx] + weak >= won {
            return;
        }
        if sym.is_common() {
            sym.set_is_common(false);
            sym.value = 0;
            sym.common_p2align = 0;
        }
        let owner = import_from_dylib(sym, dylibs, providers, dylib_idx);
        // -weak_framework / -weak_library / -weak-l: every import from
        // the library is a weak import (ld64 binds it weak-import and
        // marks it N_WEAK_REF), whatever the references say.
        if dylibs[owner].is_weak {
            sym.set_is_weak_ref(true);
        }
    });
}

/// Brings each dylib's symbols (DylibFile::symbols) up to date with the
/// symbol table: a dylib new to resolution looks up each of its
/// exports, and one that collected its symbols before checks the global
/// symbols interned since. mold interns a shared library's symbols as
/// it reads the library; a Mach-O dylib exports far more than a link
/// uses (an SDK framework's stub tens of thousands of symbols), so only
/// the ones the inputs name are kept.
fn collect_dylib_symbols<E: Target>(ctx: &mut Context<E>) {
    let num_syms = ctx.symbols.syms.len();
    let symbols = &ctx.symbols;
    let seen = ctx.dylibs.iter().filter_map(|d| d.symbols_seen).min().unwrap_or(num_syms);
    let interned: Vec<SymbolId> = (seen as SymbolId..num_syms as SymbolId)
        .into_par_iter()
        .filter(|&id| symbols.get(symbols[id].name()) == Some(id))
        .collect();
    ctx.dylibs.par_iter_mut().for_each(|dylib| {
        match dylib.symbols_seen {
            // An SDK framework's stub can export a hundred thousand
            // names, so they are looked up in parallel too.
            None => {
                let names: Vec<&[u8]> = dylib.exports.iter().copied().collect();
                dylib.symbols = names.par_iter().filter_map(|n| symbols.get(n)).collect();
            }
            Some(seen) => {
                let new = interned.iter().filter(|&&id| id as usize >= seen);
                let exported = new.filter(|&&id| dylib.exports.contains(symbols[id].name()));
                dylib.symbols.extend(exported);
            }
        }
        dylib.symbols_seen = Some(num_syms);
    });
}

/// For each symbol, the place in `order` of the first of those dylibs
/// that exports it, u32::MAX if none does, a weak export's with
/// WEAK_EXPORT set: a dylib's weak definitions come after every strong
/// one, as in mold's ranks. Each dylib races the place into the symbols
/// it exports with an atomic minimum, as each of mold's shared
/// libraries resolves its own symbols by rank.
fn first_exporters(
    dylibs: &[input_files::DylibFile],
    syms: &[Symbol],
    order: &[usize],
) -> Vec<std::sync::atomic::AtomicU32> {
    use std::sync::atomic::{AtomicU32, Ordering};
    let first: Vec<AtomicU32> =
        (0..syms.len()).into_par_iter().map(|_| AtomicU32::new(u32::MAX)).collect();
    order.par_iter().enumerate().for_each(|(pos, &dylib_idx)| {
        let dylib = &dylibs[dylib_idx];
        for &id in &dylib.symbols {
            let weak = !dylib.weak_exports.is_empty()
                && dylib.weak_exports.contains(syms[id as usize].name());
            let key = if weak { pos as u32 | WEAK_EXPORT } else { pos as u32 };
            first[id as usize].fetch_min(key, Ordering::Relaxed);
        }
    });
    first
}

/// Marks a weak export in first_exporters' places.
const WEAK_EXPORT: u32 = 1 << 31;

/// How resolution ranks the dylibs of the link: the rank each claims
/// symbols with (see dylib_ranks), and those it takes the symbols of
/// its private re-exports from (see merged_providers).
struct DylibRanking {
    ranks: Vec<u64>,
    providers: Vec<Vec<usize>>,
}

impl DylibRanking {
    fn new(dylibs: &[input_files::DylibFile]) -> Self {
        Self { ranks: dylib_ranks(dylibs), providers: merged_providers(dylibs) }
    }
}

/// For each dylib, the dylibs of the link it merged as private
/// re-exports (see `providing_dylib`). An auto-linked library named by
/// an @rpath install name is none: ld-prime takes it for the one the
/// dylib re-exports, which it has loaded already (XCTest's
/// XCUIAutomation, which UI tests' objects auto-link), and binds its
/// symbols to the dylib.
fn merged_providers(dylibs: &[input_files::DylibFile]) -> Vec<Vec<usize>> {
    let by_name: hashbrown::HashMap<&[u8], usize> = (dylibs.iter().enumerate())
        .filter(|(_, d)| !(d.is_autolinked && d.install_name.starts_with(b"@rpath/")))
        .map(|(i, d)| (d.install_name.as_slice(), i))
        .collect();
    dylibs
        .iter()
        .map(|d| {
            d.merged_reexports.iter().filter_map(|n| by_name.get(n.as_slice()).copied()).collect()
        })
        .collect()
}

/// The dylib a symbol found in `dylibs[idx]`'s exports binds to. A
/// private re-export's exports count as the re-exporting dylib's
/// (libswiftDarwin's include libswift_Builtin_float's), but when the
/// library that defines the symbol is in the link itself - named or
/// auto-linked - ld-prime binds to it, whichever of the two comes first.
/// An export that an $ld$previous directive moves to an older library
/// for the target binds to that one.
fn providing_dylib(
    dylibs: &[input_files::DylibFile],
    providers: &[Vec<usize>],
    mut idx: usize,
    name: &[u8],
) -> usize {
    for _ in 0..dylibs.len() {
        match providers[idx].iter().find(|&&p| dylibs[p].exports.contains(name)) {
            Some(&p) => idx = p,
            None => break,
        }
    }
    dylibs[idx].moved_exports.get(name).copied().unwrap_or(idx)
}

/// Makes `sym`, found in `dylibs[idx]`'s exports, an import from the
/// dylib that provides it, and returns that dylib's index.
fn import_from_dylib(
    sym: &mut crate::symbol::Symbol,
    dylibs: &[input_files::DylibFile],
    providers: &[Vec<usize>],
    idx: usize,
) -> usize {
    let owner = providing_dylib(dylibs, providers, idx, sym.name());
    sym.set_file(FileId::Dylib(owner as u32));
    sym.set_is_imported(true);
    sym.set_is_extern(true);
    sym.set_input_section(None);
    sym.set_is_common(false);
    owner
}

/// Lets newly auto-linked dylibs claim still-unresolved symbols. They
/// carry later priorities than every file already resolved, so they
/// can steal nothing - a full re-resolution would reach exactly this
/// outcome, at many times the cost.
fn claim_new_dylibs<E: Target>(ctx: &mut Context<E>, first: usize) {
    use std::sync::atomic::Ordering;
    collect_dylib_symbols(ctx);
    let dylibs = &ctx.dylibs;
    let providers = merged_providers(dylibs);
    let order = dylib_search_order(&dylib_ranks(dylibs), first);
    let exporters = first_exporters(dylibs, &ctx.symbols.syms, &order);
    ctx.symbols.syms.par_iter_mut().enumerate().for_each(|(i, sym)| {
        let key = exporters[i].load(Ordering::Relaxed);
        if key == u32::MAX || !sym.is_used() || sym.is_defined() {
            return;
        }
        import_from_dylib(sym, dylibs, &providers, order[(key & !WEAK_EXPORT) as usize]);
    });
}

/// The rank with which each dylib's strong exports claim a symbol,
/// comparable with a lazy archive member's (see
/// ObjectFile::definition_rank), lower first; its weak exports rank one
/// class lower, as a member's weak definitions do. A dylib that stands
/// for a library exports moved to has none. ld-prime looks a symbol up
/// in the libraries the command line names, in their order among the
/// other inputs, and only then, after every archive, in the public
/// libraries they re-export: nearest first, a private library in
/// between counting as a step, and among the equally near by the
/// install name of the library that re-exports them, then by their own
/// (a breadth-first walk of each level sorted by name). Then come the
/// libraries auto-link options name, and the ones they re-export
/// likewise. So a symbol of both Foundation and CFNetwork that
/// `-framework Carbon -framework Foundation` finds binds to Foundation,
/// though Carbon re-exports CoreServices, which re-exports CFNetwork.
pub fn dylib_ranks(dylibs: &[input_files::DylibFile]) -> Vec<u64> {
    let phase = |phase: u64| (2 << 40) | (phase << 32);
    let mut ranks = vec![u64::MAX; dylibs.len()];
    for (i, d) in dylibs.iter().enumerate().filter(|(_, d)| !d.is_implicit) {
        // A library first loaded as a re-export takes the place of its
        // naming.
        let priority = d.named_at.unwrap_or(d.priority) as u64;
        ranks[i] = phase(if d.is_autolinked { 2 } else { 0 }) | priority;
    }
    for autolinked in [false, true] {
        // How near each re-exported library is, and the library that
        // re-exports it there.
        let mut key: Vec<Option<(u32, &[u8])>> = vec![None; dylibs.len()];
        let mut queue: Vec<(usize, u32)> = (0..dylibs.len())
            .filter(|&i| !dylibs[i].is_implicit && dylibs[i].is_autolinked == autolinked)
            .map(|i| (i, 0))
            .collect();
        while let Some((i, depth)) = queue.pop() {
            for edge in &dylibs[i].reexported {
                let (to, k) = (edge.dylib, (depth + edge.hops, edge.via.as_slice()));
                if dylibs[to].is_implicit
                    && ranks[to] == u64::MAX
                    && key[to].is_none_or(|old| k < old)
                {
                    key[to] = Some(k);
                    queue.push((to, k.0));
                }
            }
        }
        let mut reached: Vec<usize> = (0..dylibs.len()).filter(|&i| key[i].is_some()).collect();
        reached.sort_by_key(|&i| (key[i], &dylibs[i].install_name));
        for (n, i) in reached.into_iter().enumerate() {
            ranks[i] = phase(if autolinked { 3 } else { 1 }) | n as u64;
        }
    }
    // One that no named library reaches, if any, comes last.
    for (i, d) in dylibs.iter().enumerate() {
        if ranks[i] == u64::MAX && d.name_source != input_files::NameSource::Moved {
            ranks[i] = phase(4) | d.priority as u64;
        }
    }
    ranks
}

/// The dylibs from `first` on that have a rank, in rank order.
fn dylib_search_order(ranks: &[u64], first: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (first..ranks.len()).filter(|&i| ranks[i] != u64::MAX).collect();
    order.sort_by_key(|&i| ranks[i]);
    order
}

/// Marks archive members whose definitions live code references,
/// walking owner links to a fixed point. A live file's tentative
/// definition loads the member whose real definition beats it, but no
/// member for a tentative definition, which loses to it (mold's
/// common_ref).
fn mark_live_objects<E: Target>(ctx: &mut Context<E>) {
    let _t = ctx.timer("mark_live_objects");
    // Resolution runs in rounds (auto-linking, LTO), and a file live
    // after one stays so, with the -why_load reason it was loaded for.
    let mut queue: Vec<usize> = (0..ctx.objs.len()).filter(|&i| ctx.objs[i].is_alive).collect();

    // The entry point and -u symbols are roots too. A dylib or bundle
    // has no entry point: an archive member that defines _main stays
    // out of one (Lua's lua.o out of Hammerspoon's LuaSkin).
    let mut root_syms: Vec<&[u8]> =
        ctx.args.has_entry_point().then_some(ctx.args.entry.as_slice()).into_iter().collect();
    root_syms.extend(ctx.args.forced_undefined.iter().map(Vec::as_slice));
    for name in root_syms {
        if let Some(id) = ctx.symbols.get(name)
            && let Some(FileId::Obj(owner)) = ctx.symbols[id].file()
        {
            let owner = owner as usize;
            if !ctx.objs[owner].is_alive {
                ctx.objs[owner].is_alive = true;
                ctx.why_load.insert(owner, ctx.symbols[id].name());
                queue.push(owner);
            }
        }
    }

    while let Some(obj_idx) = queue.pop() {
        for i in ctx.objs[obj_idx].global_range() {
            let msym = ctx.objs[obj_idx].mach_syms[i];
            if !msym.is_undef() {
                continue;
            }
            let sym_id = ctx.objs[obj_idx].symbols[i];
            if msym.is_common() && ctx.symbols[sym_id].is_common() {
                continue;
            }
            load_owner(ctx, sym_id, &mut queue);
        }
    }
}

/// Makes live the archive member that defines a symbol something live
/// wants, if it is not yet.
fn load_owner<E: Target>(ctx: &mut Context<E>, sym_id: SymbolId, queue: &mut Vec<usize>) {
    if let Some(FileId::Obj(owner)) = ctx.symbols[sym_id].file() {
        let owner = owner as usize;
        if !ctx.objs[owner].is_alive {
            ctx.objs[owner].is_alive = true;
            ctx.why_load.insert(owner, ctx.symbols[sym_id].name());
            queue.push(owner);
        }
    }
}

/// The objects check_input_versions has checked, and the Objective-C
/// image info flags they merge to (see check_objc_flags).
#[derive(Default)]
pub struct CheckedInputs {
    objs: Vec<bool>,
    objc: Option<ObjcImageInfo>,
}

/// Warns if a dylib with install name `install_name` was built for an
/// OS version, `built_for`, newer than the link's.
fn warn_newer_dylib<E: Target>(ctx: &Context<E>, install_name: &[u8], built_for: u32) {
    let minos = ctx.args.platform_minos;
    if minos != 0 && built_for > minos {
        crate::warn!(
            "building for {}-{}, but linking with dylib '{}' which was built for newer version {}",
            platform_name(ctx.args.platform),
            format_version(minos),
            raw(install_name),
            format_version(built_for)
        );
    }
}

/// Checks the deployment target and the Objective-C image info of each
/// live object `checked` doesn't cover yet. Unused archive members must
/// not cause errors or warnings. The driver calls this before LTO, so
/// that bitcode built for another platform stops the link before it is
/// compiled, and again for the objects LTO made or pulled in.
pub fn check_input_versions<E: Target>(ctx: &Context<E>, checked: &mut CheckedInputs) {
    checked.objs.resize(ctx.objs.len(), false);
    for (i, obj) in ctx.objs.iter().enumerate() {
        // The hook for the classes of mergeable libraries is the
        // linker's, for any macOS.
        if !obj.is_alive || checked.objs[i] || ctx.is_bundle_hook(i) {
            continue;
        }
        checked.objs[i] = true;
        // A -r or -preload output for no platform takes any object.
        if ctx.args.platform != 0 {
            check_object_version(ctx, i);
        }
        if let Some(info) = obj.objc_image_info {
            checked.objc = Some(check_objc_flags(ctx, checked.objc, info, obj.mf));
        }
    }
}

/// Warns of each dylib the link names that was built for a newer OS
/// version than the link's, but not one only re-exported, nor one of
/// the SDK, built for newer OS versions as a matter of course. (A
/// dylib built for another platform is refused as it is read; see
/// input_files::check_dylib_platforms.)
pub fn warn_newer_dylibs<E: Target>(ctx: &Context<E>) {
    for dylib in ctx.dylibs.iter().filter(|d| !d.is_implicit && !d.in_sdk) {
        warn_newer_dylib(ctx, &dylib.install_name, dylib.minos);
    }
}

/// Checks the deployment target of object `i` (see
/// check_input_versions).
fn check_object_version<E: Target>(ctx: &Context<E>, i: usize) {
    let obj = &ctx.objs[i];
    let (platform, minos) = (ctx.args.platform, ctx.args.platform_minos);
    // An object may declare more than one platform; use the deployment
    // target for the platform being linked. ld-prime takes one with no
    // version command (an old one, or one assembled for no OS) for the
    // link's platform, with a warning but where it links firmware
    // (see Args::effective_platform), which takes code built for any
    // platform. The object the linker synthesizes has none either.
    let Some(first) = obj.platform_versions.first() else {
        let assumed = ctx.args.effective_platform();
        if assumed != PLATFORM_FIRMWARE && !ctx.is_internal(i) {
            crate::warn!(
                "no platform load command found in '{}', assuming: {}",
                obj.mf.name.raw(),
                platform_name(assumed)
            );
        }
        return;
    };
    let Some(version) = obj.platform_versions.iter().find(|v| v.platform == platform) else {
        // Firmware takes code built for any platform.
        if platform == crate::macho::PLATFORM_FIRMWARE {
            return;
        }
        fatal!(
            "building for '{}', but linking in object file ({}) built for '{}'",
            platform_name(platform),
            obj.mf.name.raw(),
            platform_name(first.platform)
        );
    };

    // A merged mergeable dylib is a dylib to this check.
    let merged = ctx.merged_libraries.iter().find(|lib| std::ptr::eq(lib.obj, obj.mf));
    if let Some(lib) = merged {
        warn_newer_dylib(ctx, &lib.install_name, lib.minos);
        return;
    }

    // The SDK version used to compile an input does not constrain its
    // use. -deployment_target_mismatches error makes the first object
    // for a newer OS fail the link, and suppress keeps quiet.
    if minos != 0 && version.minos > minos {
        let msg = format_args!(
            "object file ({}) was built for newer '{}' version ({}) than being linked ({})",
            obj.mf.name.raw(),
            platform_name(version.platform),
            format_version(version.minos),
            format_version(minos)
        );
        match ctx.args.deployment_target_mismatches {
            Treatment::Warning => crate::warn!("{msg}"),
            Treatment::Error => fatal!("{msg}"),
            Treatment::Suppress => {}
        }
    }
}

/// Merges the Objective-C image info of an object, `info`, into that of
/// the objects checked before it, `merged`, with ld-prime's
/// diagnostics. The first Swift ABI version stays: another one
/// fails the link (or with $LD_WARN_ON_SWIFT_ABI_VERSION_MISMATCHES
/// draws a warning). An object that has category class properties
/// where those before don't, or lacks them where those before have
/// them, draws a warning - each one that differs from the merged flags,
/// which lose the bit at the first. So does one that differs from the
/// objects with classes before it in signing class_ro_t pointers, if
/// it has classes or signs (an error with -objc_class_ro_signing_mismatch
/// error).
fn check_objc_flags<E: Target>(
    ctx: &Context<E>,
    merged: Option<ObjcImageInfo>,
    info: ObjcImageInfo,
    mf: &MappedFile,
) -> ObjcImageInfo {
    let Some(merged) = merged else { return info };
    let (first, abi) = ((merged.flags >> 8) & 0xff, (info.flags >> 8) & 0xff);
    if first != 0 && abi != 0 && abi != first {
        let (first, file) = (swift_abi_name(first), mf.name.raw());
        if ctx.args.warn_swift_abi_mismatches {
            crate::warn!(
                "{file} compiled with a different Swift ABI version ({}), than previous files \
                 ({first})",
                swift_abi_name(abi)
            );
        } else {
            fatal!(
                "not all .o files built with the same Swift ABI version. Started with ({first}), \
                 now found ({}) in {file}",
                swift_abi_name(abi)
            );
        }
    }
    let cat = info.flags & OBJC_HAS_CATEGORY_CLASS_PROPERTIES;
    if cat != merged.flags & OBJC_HAS_CATEGORY_CLASS_PROPERTIES {
        crate::warn!(
            "mixed ObjC ABI, {} compiled {} category class properties",
            mf.name.raw(),
            if cat != 0 { "with" } else { "without" }
        );
    }
    let signed = info.flags & OBJC_SIGNED_CLASS_RO != 0;
    if merged.classes
        && (info.classes || signed)
        && signed != (merged.flags & OBJC_SIGNED_CLASS_RO != 0)
    {
        let msg = format!(
            "'{}' {} built with class_ro_t pointer signing enabled, but previous .o file {}",
            mf.name.raw(),
            if signed { "was" } else { "was not" },
            if signed { "was not" } else { "was" }
        );
        match ctx.args.objc_class_ro_signing_mismatch {
            Treatment::Error => fatal!("{msg}"),
            _ => crate::warn!("{msg}"),
        }
    }
    chunks::objc_imageinfo::merge_objc_info(merged, info)
}

/// A Swift ABI version, the byte of __objc_imageinfo's flags that holds
/// it, as ld-prime names it.
fn swift_abi_name(v: u32) -> String {
    let name = match v {
        1 => "1.0",
        2 => "1.1",
        3 => "2.0",
        4 => "3.0",
        5 => "4.0",
        6 => "4.1/4.2",
        7 => "5 or later",
        _ => return format!("unknown ABI version 0x{v:02X}"),
    };
    name.to_string()
}

/// Whether a -r link takes in bitcode and nothing else, none of it
/// built for ThinLTO: ld-prime then writes the modules merged into one
/// bitcode file rather than an object, so that the final link still
/// optimizes them as a whole. A Mach-O object of any content, a
/// ThinLTO module or -flto-codegen-only makes it compile them instead.
pub fn links_only_bitcode<E: Target>(ctx: &Context<E>) -> bool {
    let mut modules = crate::lto::live_bitcode_modules(ctx).peekable();
    modules.peek().is_some()
        && !ctx.args.lto_codegen_only
        && modules.all(|module| !module.is_thin)
        && ctx
            .objs
            .iter()
            .enumerate()
            .all(|(i, obj)| !obj.is_alive || obj.lto_module.is_some() || ctx.is_internal(i))
}

/// Writes a -r link of bitcode alone as one merged bitcode file (see
/// links_only_bitcode and lto::write_merged_bitcode).
pub fn write_merged_bitcode<E: Target>(ctx: &Context<E>) {
    crate::lto::write_merged_bitcode(ctx);
}

/// Whether a live file is bitcode, for LTO to compile.
pub fn has_lto_obj<E: Target>(ctx: &Context<E>) -> bool {
    crate::lto::live_bitcode_modules(ctx).next().is_some()
}

/// Has libLTO compile the live bitcode modules to Mach-O objects (see
/// lto::run_plugin) and resolves symbols again with the compiled
/// objects in place of the bitcode files.
pub fn do_lto<E: Target>(ctx: &mut Context<E>) {
    let objects = crate::lto::run_plugin(ctx);
    retire_bitcode_placeholders(ctx);

    let first = ctx.objs.len();
    for crate::lto::LtoObject { name, mtime, data } in objects {
        let data = Vec::leak(data);
        let mf = crate::mapped_file::MappedFile { name, data, parent: None, mtime };
        // An output of x86_64h bitcode is an x86_64h object: ld-prime
        // warns of it in an x86_64 link as of an input.
        let mf = Box::leak(Box::new(mf));
        if !crate::reader::is_foreign(ctx, mf) {
            input_files::parse_object(ctx, mf, true);
        }
    }
    ctx.lto_objs = first..ctx.objs.len();

    // Redo name resolution.
    resolve_symbols(ctx);
}

/// Retires the bitcode files' placeholder objects once LTO compiled
/// them: the compiled objects provide the real definitions, so they
/// must neither claim nor reference anything in the next resolution
/// round. What diagnostics still need to know of the files compiled is
/// kept aside.
fn retire_bitcode_placeholders<E: Target>(ctx: &mut Context<E>) {
    for module in std::mem::take(&mut ctx.lto_modules) {
        let obj_idx = module.obj;
        let obj = &ctx.objs[obj_idx];
        if obj.is_alive {
            let won = obj
                .symbols
                .iter()
                .filter(|&&id| ctx.symbols[id].file() == Some(FileId::Obj(obj_idx as u32)))
                .map(|&id| ctx.symbols[id].name())
                .collect();
            let defined = module.defined;
            let input = crate::lto::LtoInput { obj: obj_idx, defined, won };
            ctx.lto_inputs.push(input);
        }
        let ids = ctx.objs[obj_idx].symbols.clone();
        for id in ids {
            let sym = &mut ctx.symbols[id];
            if sym.file() == Some(FileId::Obj(obj_idx as u32)) {
                sym.clear_file();
                sym.set_input_section(None);
                sym.value = 0;
                sym.set_is_weak_def(false);
            }
        }
        let obj = &mut ctx.objs[obj_idx];
        obj.is_alive = false;
        obj.mach_syms = std::borrow::Cow::Borrowed(&[]);
        obj.symbols.clear();
    }
}

/// Hides the subsections of archive members that resolution left
/// dead, so nothing of theirs reaches the output.
pub fn remove_unreachable_files<E: Target>(ctx: &mut Context<E>) {
    for isec in ctx.isecs.iter_mut() {
        if !ctx.objs[isec.file as usize].is_alive {
            isec.set_alive(false);
        }
    }

    // Unwind records and FDEs of dead files go too.
    input_files::remove_dead_unwind_info(ctx);
}

/// -remove_swift_reflection_metadata_sections: drops the Swift
/// reflection metadata from a final image and a -r output alike, as
/// ld-prime drops its subsections as it reads them, before anything can
/// keep them alive. What still refers to them is an error (see
/// check_removed_swift_metadata_refs).
pub fn remove_swift_reflection_metadata<E: Target>(ctx: &mut Context<E>) {
    if !ctx.args.remove_swift_reflection_metadata_sections {
        return;
    }
    let removed: Vec<usize> = (0..ctx.isecs.len())
        .filter(|&i| input_files::is_swift_reflection_section(ctx.hdr_of(&ctx.isecs[i])))
        .collect();
    for i in removed {
        ctx.isecs[i].set_alive(false);
    }
}

/// Reports each live reference to the Swift reflection metadata that
/// -remove_swift_reflection_metadata_sections dropped - such as a type
/// descriptor's to its field descriptor, which every Swift type has -
/// whose target so has no address. (ld-prime fails a final link at the
/// first by address as it writes it, and crashes on a pointer or in a
/// -r link.)
pub fn check_removed_swift_metadata_refs<E: Target>(ctx: &Context<E>) {
    if !ctx.args.remove_swift_reflection_metadata_sections {
        return;
    }
    let removed = |isec: usize| {
        let isec = &ctx.isecs[ctx.resolve_isec(isec)];
        !isec.is_alive() && input_files::is_swift_reflection_section(ctx.hdr_of(isec))
    };
    for (i, isec) in ctx.isecs.iter().enumerate().filter(|(_, isec)| isec.is_emitted()) {
        for rel in ctx.isec_relocs(i) {
            let file = isec.file as usize;
            let target = match ctx.reloc_target_sym(file, rel) {
                Some(id) => ctx.symbols[id].input_section().map(|t| t as usize),
                None => ctx.reloc_target_isec(file, rel),
            };
            if target.is_some_and(removed) {
                let target = ctx.reloc_target_name(file, rel);
                let msg = format_args!("target '{}' does not have address", raw(&target));
                ctx.fixup_error(i, rel.offset, msg);
            }
        }
    }
}

/// With -init_offsets (which chained fixups imply, see
/// Args::init_offsets), replaces __mod_init_func's absolute pointers
/// (which each need a rebase) with 32-bit image-relative offsets in a
/// __TEXT,__init_offsets section (type S_INIT_FUNC_OFFSETS), which
/// dyld runs the same way but never has to fix up.
pub fn convert_init_offsets<E: Target>(ctx: &mut Context<E>) {
    let init = init_function(ctx);
    if !ctx.args.init_offsets {
        // ld-prime runs an -init function only from __init_offsets and
        // drops it here. ld64 named it in LC_ROUTINES_64, which dyld
        // runs before the image's other initializers; so do we, for an
        // image dyld loads, if the function is its own.
        if !ctx.args.without_dyld() {
            ctx.init_routine = init.filter(|&id| ctx.symbols[id].input_section().is_some());
        }
        return;
    }
    // ld-prime makes it the first of the initializer offsets.
    if let Some(id) = init {
        let func = init_func(ctx, id);
        ctx.init_offsets.init_funcs.push(func);
    }
    // ld-prime runs the hook for the classes of mergeable libraries (see
    // bundle_hook) last, though its object comes first.
    let mut pointers: Vec<usize> = (0..ctx.isecs.len())
        .filter(|&i| {
            ctx.hdr_of(&ctx.isecs[i]).section_type() == S_MOD_INIT_FUNC_POINTERS
                && ctx.isecs[i].is_alive()
        })
        .collect();
    pointers.sort_by_key(|&i| ctx.is_bundle_hook(ctx.isecs[i].file as usize));
    for i in pointers {
        let obj = ctx.isecs[i].file as usize;
        for rel in initializer_relocs(ctx, i) {
            let func = match rel.target() {
                RelocTarget::Sym(idx) => init_func(ctx, ctx.objs[obj].symbols[idx as usize]),
                RelocTarget::Section(isec) => {
                    InitFunc::Local(ctx.resolve_isec(isec as usize), rel.addend as u64)
                }
            };
            ctx.init_offsets.init_funcs.push(func);
        }
        ctx.isecs[i].set_alive(false);
    }
}

/// The initializer symbol `id` is. One dyld binds, or an absolute
/// one, has no offset in the image: the link fails as it is written
/// (see init_offsets::copy_buf).
fn init_func<E: Target>(ctx: &Context<E>, id: crate::symbol::SymbolId) -> InitFunc {
    let sym = &ctx.symbols[id];
    match sym.input_section() {
        Some(isec) => InitFunc::Local(ctx.resolve_isec(isec as usize), sym.value),
        None => InitFunc::Imported(id),
    }
}

/// The function -init names, if it is defined. An undefined one is
/// reported with the other initial undefines.
fn init_function<E: Target>(ctx: &Context<E>) -> Option<crate::symbol::SymbolId> {
    let id = ctx.symbols.get(ctx.args.init.as_deref()?)?;
    ctx.symbols[id].is_defined().then_some(id)
}

/// The relocations naming the functions of the initializer pointers
/// subsection `i` holds, in slot order. A pointer the difference of two
/// symbols makes (a SUBTRACTOR and an UNSIGNED relocation) names the
/// function it adds, as in ld-prime, not the one it subtracts too.
fn initializer_relocs<E: Target>(ctx: &Context<E>, i: usize) -> Vec<crate::input_sections::Reloc> {
    let mut relocs: Vec<_> =
        ctx.isec_relocs(i).iter().filter(|r| r.ty != E::RELOC_SUBTRACTOR).copied().collect();
    relocs.sort_by_key(|r| r.offset);
    relocs
}

/// ld-prime's diagnostics for static initializers: a warning for each
/// in a dylib bound for the dyld shared cache, where every process
/// would run it, unless -no_warn_inits; with -no_inits, an error listing
/// them all. A build for profiling has initializers by design: an
/// object with an __llvm_prf_ section (clang's -fprofile-instr-generate
/// counters and records) turns both off.
pub fn check_initializers<E: Target>(ctx: &Context<E>) {
    let args = &ctx.args;
    let warn = args.shared_region && args.output_type == MH_DYLIB && !args.no_warn_inits;
    if !warn && !args.no_inits {
        return;
    }
    let profiling = |obj: &input_files::ObjectFile| {
        obj.sect_hdrs.iter().any(|hdr| hdr.sectname().starts_with(b"__llvm_prf_"))
    };
    if ctx.objs.iter().any(|obj| obj.is_alive && profiling(obj)) {
        return;
    }
    let inits = initializers(ctx);
    if args.no_inits {
        if !inits.is_empty() {
            let list: Vec<u8> = (inits.iter())
                .flat_map(|(name, file)| error::render(format_args!("{} in {file}\n", raw(name))))
                .collect();
            error!("Static initializers:\n{}", raw(&list));
        }
        return;
    }
    for (name, file) in inits {
        let name = raw(name);
        crate::warn!(
            "static initializer '{name}' found in '{file}'. Use -no_inits to make this an \
             error.  Use -no_warn_inits to suppress warning"
        );
    }
}

/// The functions the inputs' __mod_init_func sections point at, by
/// name, with the files that hold the pointers.
fn initializers<E: Target>(ctx: &Context<E>) -> Vec<(&[u8], error::Raw<'_>)> {
    let mut vec = Vec::new();
    for (i, isec) in ctx.isecs.iter().enumerate() {
        if !isec.is_alive() || ctx.hdr_of(isec).section_type() != S_MOD_INIT_FUNC_POINTERS {
            continue;
        }
        let obj = &ctx.objs[isec.file as usize];
        for rel in initializer_relocs(ctx, i) {
            let name = match rel.target() {
                RelocTarget::Sym(idx) => ctx.symbols[obj.symbols[idx as usize]].name(),
                RelocTarget::Section(target) => {
                    let target = ctx.resolve_isec(target as usize) as u32;
                    obj.symbols
                        .iter()
                        .map(|&id| &ctx.symbols[id])
                        .find(|s| s.input_section() == Some(target) && s.value == rel.addend as u64)
                        .map_or(&b""[..], |s| s.name())
                }
            };
            vec.push((name, obj.mf.name.raw()));
        }
    }
    vec
}

/// The tentative definitions (common symbols) no definition replaced,
/// in the order the objects' symbol tables first declare them.
fn common_symbols_in_order<E: Target>(ctx: &Context<E>) -> Vec<SymbolId> {
    let mut seen = hashbrown::HashSet::new();
    let mut out = Vec::new();
    for obj in ctx.objs.iter().filter(|obj| obj.is_alive) {
        let r = obj.global_range();
        for (msym, &id) in obj.mach_syms[r.clone()].iter().zip(&obj.symbols[r]) {
            let sym = &ctx.symbols[id];
            if msym.is_common() && sym.is_common() && !sym.is_defined() && seen.insert(id) {
                out.push(id);
            }
        }
    }
    out
}

/// Converts surviving tentative definitions (common symbols) into real
/// definitions in a synthetic __DATA,__common zero-fill section.
pub fn convert_common_symbols<E: Target>(ctx: &mut Context<E>) {
    let internal = ctx.internal_obj.expect("internal object not created yet") as u32;
    for i in common_symbols_in_order(ctx) {
        let sym = &ctx.symbols[i];
        let size = sym.value;
        // An alignment the object gave (.comm's third operand) is kept;
        // without one, ld64 aligns the symbol to its size rounded up to
        // a power of two, at most -max_default_common_align's (a
        // 100000-byte array asks for 32KB by default, which the page
        // then caps with a warning).
        let p2align = if sym.common_p2align != 0 || size == 0 {
            sym.common_p2align
        } else {
            (size.next_power_of_two().trailing_zeros() as u8).min(ctx.args.max_default_common_align)
        };

        let (file, shndx) = ctx.add_synthetic_section(MachSection {
            sectname: bytes_to_name(b"__common"),
            segname: bytes_to_name(b"__DATA"),
            size,
            p2align: p2align as u32,
            flags: S_ZEROFILL,
            ..Default::default()
        });
        ctx.isecs.push(InputSection::new(file, shndx, p2align, size as u32, &[]));

        let sym = &mut ctx.symbols[i];
        sym.set_file(FileId::Obj(internal));
        sym.set_input_section(Some((ctx.isecs.len() - 1) as u32));
        sym.value = 0;
        sym.set_is_common(false);
        sym.set_is_extern(true);
    }
}

/// Marks the literal records a symbol names, other than a temporary
/// (L-prefixed, which the arm64 assembler keeps for relocations to
/// name) or linker-private (l-prefixed, the assembler's ltmpN labels
/// included) one: ld-prime keeps each such record a subsection of its
/// own, merged with no identical copy. So it keeps an __objc_superrefs
/// or __objc_protorefs entry any symbol names, even the ltmpN label of
/// its section's start (see coalesce_objc_refs), unless the section
/// is of the literal-pointer type (see is_class_or_protocol_ref).
fn mark_labeled_literals<E: Target>(ctx: &Context<E>) {
    ctx.symbols.syms.par_iter().for_each(|sym| {
        if let Some(i) = sym.input_section()
            && !sym.name().is_empty()
        {
            let isec = &ctx.isecs[i as usize];
            let hdr = ctx.hdr_of(isec);
            let labeled = match hdr.section_type() {
                S_CSTRING_LITERALS | S_4BYTE_LITERALS | S_8BYTE_LITERALS | S_16BYTE_LITERALS => {
                    !sym.name().starts_with(b"l") && !sym.name().starts_with(b"L")
                }
                S_LITERAL_POINTERS => false,
                _ => input_files::is_class_or_protocol_ref(hdr),
            };
            if labeled {
                isec.mark_labeled();
            }
        }
    });
}

/// Merges identical literal elements across all live inputs: the most
/// aligned live copy wins, the first of equals, and the rest redirect
/// to it. Only the elements ld-prime merges take part (see
/// is_mergeable_literal); a labeled record stays apart, and so do
/// copies in sections of different names, as in ld-prime: a class
/// named "Foo" keeps its name in __objc_classname though __cstring has
/// a "Foo" too.
///
/// A C string keeps its input offset modulo its section's alignment,
/// as any subsection does (see InputSection::align_offset), and like
/// ld64 ld-prime keeps the copy that alignment favors most (see
/// InputSection::p2align_at): Swift pads the strings of its 16-aligned
/// __objc_methname so that many start at a multiple of 16, and a copy
/// from Swift then wins over clang's, which has no alignment.
pub fn merge_literals<E: Target>(ctx: &mut Context<E>) {
    mark_labeled_literals(ctx);
    // Deduplication follows the symbol table's sharded shape: every
    // element's content hash is computed in parallel, elements bin by
    // hash, and the shards resolve independently, each meeting its
    // copies in input order.
    let literals: Vec<Literal> = ctx
        .isecs
        .par_iter()
        .enumerate()
        .filter_map(|(i, isec)| {
            if !isec.is_emitted() || isec.is_labeled() {
                return None;
            }
            let hdr = ctx.hdr_of(isec);
            if !input_files::is_mergeable_literal(hdr, isec) {
                return None;
            }
            Some((xxhash_rust::xxh3::xxh3_64(isec.data()), hdr, i as u32))
        })
        .collect();

    const NUM_SHARDS: usize = 64;
    let mut shards: Vec<Vec<Literal>> = vec![Vec::new(); NUM_SHARDS];
    for &lit in &literals {
        shards[(lit.0 % NUM_SHARDS as u64) as usize].push(lit);
    }
    let isecs = &ctx.isecs;
    let folds: Vec<Vec<(u32, u32)>> =
        shards.into_par_iter().map(|shard| merge_shard(isecs, shard)).collect();

    // The winner keeps its own alignment: a loser's is no stricter.
    for (loser, winner) in folds.into_iter().flatten() {
        ctx.isecs[loser as usize].replacement = winner;
    }
    input_files::redirect_symbols_to_replacements(ctx);
}

/// A literal element to merge: its content hash, its section's header
/// and its subsection.
type Literal<'a> = (u64, &'a MachSection, u32);

/// Merges the identical elements of a shard, met in input order: the
/// same bytes in a section of the same name and type. Of each group the
/// most aligned copy wins, the first of equals. Returns each losing
/// copy with the winning one.
fn merge_shard(isecs: &[InputSection], shard: Vec<Literal>) -> Vec<(u32, u32)> {
    // Keyed by the content hash already computed, each group's first
    // copy and its index in `best`, which holds its winner so far.
    let mut table: hashbrown::HashTable<(u64, &MachSection, u32, u32)> =
        hashbrown::HashTable::new();
    let mut best: Vec<u32> = Vec::new();
    let mut losers: Vec<(u32, u32)> = Vec::new();
    let p2align = |i: u32| isecs[i as usize].p2align_at(isecs[i as usize].input_addr as u64);
    for (hash, hdr, i) in shard {
        let data = isecs[i as usize].data();
        let same = |&(h, other, j, _): &(u64, &MachSection, u32, u32)| {
            h == hash
                && other.segname == hdr.segname
                && other.sectname == hdr.sectname
                && other.section_type() == hdr.section_type()
                && isecs[j as usize].data() == data
        };
        match table.entry(hash, same, |e| e.0) {
            hashbrown::hash_table::Entry::Occupied(e) => {
                let group = e.get().3;
                let winner = &mut best[group as usize];
                if p2align(i) > p2align(*winner) {
                    losers.push((*winner, group));
                    *winner = i;
                } else {
                    losers.push((i, group));
                }
            }
            hashbrown::hash_table::Entry::Vacant(e) => {
                e.insert((hash, hdr, i, best.len() as u32));
                best.push(i);
            }
        }
    }
    losers.into_iter().map(|(i, group)| (i, best[group as usize])).collect()
}

/// Auto-hides eligible weak definitions. Compilers mark a weak
/// definition whose address is never observed with
/// .weak_def_can_be_hidden (a MachSym's desc carries N_WEAK_DEF and
/// N_WEAK_REF together): no one can tell which image's copy they use,
/// so ld64 demotes such symbols to non-external in every kind of
/// output - executables, dylibs and bundles alike - gone from the
/// export trie and the external symbol table, and referenced directly
/// rather than through weak-lookup binds (ld-prime's NetNewsWire
/// dylib hides PLCrashReporter's template constructors this way).
/// The scopes of coalesced copies merge: one plain .weak_definition
/// among them pins the symbol exported, and an -exported_symbols_list
/// naming it does too.
pub fn auto_hide_weak_defs<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.relocatable {
        return;
    }

    // For each symbol, "seen a live weak def" and "every live weak def
    // may be hidden". A C++ debug link has millions of weak-def MachSyms
    // (every inline and template instance), so this reduces over them
    // in parallel into a dense array keyed by the symbol's id - mold's
    // pattern - rather than a serial fold into a hash map. Bit 0 marks
    // a symbol seen; bit 1 marks it as having a def that cannot hide.
    // Both bits are monotonic (only ever set), so racing relaxed
    // stores are safe.
    use std::sync::atomic::{AtomicU8, Ordering};
    const SEEN: u8 = 1;
    const NOT_HIDABLE: u8 = 2;
    let flags: Vec<AtomicU8> = (0..ctx.symbols.syms.len()).map(|_| AtomicU8::new(0)).collect();
    ctx.objs.par_iter().for_each(|obj| {
        if !obj.is_alive {
            return;
        }
        let r = obj.global_range();
        for (msym, &sym_id) in obj.mach_syms[r.clone()].iter().zip(&obj.symbols[r]) {
            if !msym.is_weak_def() {
                continue;
            }
            let bits = if msym.desc & N_WEAK_REF != 0 { SEEN } else { SEEN | NOT_HIDABLE };
            flags[sym_id as usize].fetch_or(bits, Ordering::Relaxed);
        }
    });

    let exported = ctx.args.exported_symbols.as_ref();
    ctx.symbols.syms.par_iter_mut().zip(&flags).for_each(|(sym, f)| {
        let f = f.load(Ordering::Relaxed);
        if f & SEEN != 0
            && f & NOT_HIDABLE == 0
            && sym.is_weak_def()
            && sym.is_extern()
            && matches!(sym.file(), Some(FileId::Obj(_)))
            && !exported.is_some_and(|exported| exported.find(sym.name()) != -1)
        {
            sym.set_is_private_extern(true);
        }
    });
}

/// Hide definitions before dead stripping and relocation scanning so
/// they neither keep otherwise unused code alive nor bind as exports.
pub fn hide_all_exports<E: Target>(ctx: &mut Context<E>) {
    if !ctx.args.no_exported_symbols {
        return;
    }
    ctx.symbols.syms.par_iter_mut().for_each(|sym| {
        if matches!(sym.file(), Some(FileId::Obj(_))) {
            sym.set_is_private_extern(true);
        }
    });
}

/// -exported_symbol(s_list) and -unexported_symbol(s_list) narrow the
/// exports by scope, as -no_exported_symbols does: ld64 turns every
/// definition they leave out into a private extern, in executables,
/// dylibs, bundles and -r outputs alike. Such a symbol is then a local
/// in the symbol table, not a dead-strip root of a dylib, not bound by
/// weak lookup and not counted toward MH_WEAK_DEFINES. The exports
/// -reexported_symbols_list adds are created afterwards, so the lists
/// never hide them. Ported from sold's handle_exported_symbols_list
/// and handle_unexported_symbols_list.
pub fn handle_exported_symbols_list<E: Target>(ctx: &mut Context<E>) {
    let Some(exported) = &ctx.args.exported_symbols else {
        return;
    };
    ctx.symbols.syms.par_iter_mut().for_each(|sym| {
        if matches!(sym.file(), Some(FileId::Obj(_)))
            && sym.is_extern()
            && exported.find(sym.name()) == -1
        {
            sym.set_is_private_extern(true);
        }
    });
}

pub fn handle_unexported_symbols_list<E: Target>(ctx: &mut Context<E>) {
    let unexported = &ctx.args.unexported_symbols;
    if unexported.is_empty() {
        return;
    }
    ctx.symbols.syms.par_iter_mut().for_each(|sym| {
        if matches!(sym.file(), Some(FileId::Obj(_)))
            && sym.is_extern()
            && unexported.find(sym.name()) != -1
        {
            sym.set_is_private_extern(true);
        }
    });
}

/// -force_symbols_weak_list and -force_symbols_not_weak_list make the
/// exported definitions they name weak or not (the weak list winning),
/// which dyld then coalesces, or not, at load time: the image calls
/// and points to a forced-weak one as to any weak definition. A hidden
/// one stays as it is, with a warning, by name, if it would change.
pub fn force_symbol_weakness<E: Target>(ctx: &mut Context<E>) {
    let (weak, not_weak) = (&ctx.args.force_weak, &ctx.args.force_not_weak);
    if weak.is_empty() && not_weak.is_empty() {
        return;
    }
    let mut hidden: Vec<(&[u8], bool)> = ctx
        .symbols
        .syms
        .par_iter_mut()
        .filter_map(|sym| {
            if !matches!(sym.file(), Some(FileId::Obj(_))) || sym.input_section().is_none() {
                return None;
            }
            let name = sym.name();
            let force = if weak.find(name) != -1 {
                true
            } else if not_weak.find(name) != -1 {
                false
            } else {
                return None;
            };
            if sym.is_weak_def() == force {
                return None;
            }
            if sym.is_extern() && !sym.is_private_extern() {
                sym.set_is_weak_def(force);
                return None;
            }
            Some((name, force))
        })
        .collect();
    hidden.par_sort_unstable();
    for (name, weak) in hidden {
        let kind = if weak { "weak" } else { "not-weak" };
        crate::warn!("cannot force to be {kind}, non-external symbol {}", raw(name));
    }
}

/// Discards the losing copies of coalesced weak definitions. Symbol
/// resolution picks one definition per weak symbol, but the losing
/// objects' subsections still hold the duplicate bodies - a C++-heavy
/// link would otherwise ship every object's copy of every template
/// instantiation as anonymous dead weight (12MB of clang's 80MB
/// __text). Each losing subsection is redirected to the winner's, the
/// same replacement mechanism literal merging and ICF use, so
/// section-target relocations into a loser resolve into the winning
/// copy. The kept copy stands for all of them whatever their sizes, as
/// in ld64: an inline function compiled at different optimization
/// levels, or a Swift __swift5_typeref string with or without a pad
/// byte, still has one definition, and a loser's bytes, relocations,
/// unwind info and data-in-code go with it (see weak_def_losers for
/// the copies that stay).
pub fn coalesce_weak_defs<E: Target>(ctx: &mut Context<E>) {
    // A C++ debug link has millions of weak-def MachSyms (every inline
    // and template instance), so the losing copies are found in
    // parallel, object by object. A later loser may resolve through an
    // earlier one, so the replacements are made serially, in object
    // order.
    let losers: Vec<Vec<(usize, usize)>> =
        (0..ctx.objs.len()).into_par_iter().map(|i| weak_def_losers(ctx, i)).collect();
    for (loser, winner) in losers.into_iter().flatten() {
        let winner = ctx.resolve_isec(winner);
        let loser = ctx.resolve_isec(loser);
        if loser != winner && ctx.isecs[loser].replacement == NO_REPLACEMENT {
            ctx.isecs[loser].replacement = winner as u32;
        }
    }
}

/// The subsections of object `obj_idx` that hold a losing copy of a
/// weak definition, each with the winning copy's subsection, in MachSym
/// order. The definition must be at the same offset in both copies, and
/// the losing subsection hold no other symbol: an object without
/// subsections-via-symbols has one subsection per section, and folding
/// it away would take every other symbol's bytes with it. ld64 splits
/// at symbols regardless; we keep such a copy.
fn weak_def_losers<E: Target>(ctx: &Context<E>, obj_idx: usize) -> Vec<(usize, usize)> {
    let obj = &ctx.objs[obj_idx];
    let mut out = Vec::new();
    if !obj.is_alive {
        return out;
    }
    // The addresses of the object's symbols, sorted, once there is a
    // losing copy to check.
    let mut values: Option<Vec<u64>> = None;
    for i in obj.global_range() {
        let (msym, sym_id) = (&obj.mach_syms[i], obj.symbols[i]);
        if !msym.is_weak_def() {
            continue;
        }
        let sym = &ctx.symbols[sym_id];
        let Some(FileId::Obj(owner)) = sym.file() else { continue };
        if owner as usize == obj_idx {
            continue;
        }
        let Some(winner) = sym.input_section() else { continue };
        let Some((loser, off)) = obj.symbol_subsec(&ctx.isecs, i) else { continue };
        if off != sym.value {
            continue;
        }
        let values = values.get_or_insert_with(|| {
            let mut v: Vec<u64> = (obj.mach_syms.iter())
                .filter(|n| !n.is_stab() && n.ty() == N_SECT)
                .map(|n| n.value)
                .collect();
            v.sort_unstable();
            v.dedup();
            v
        });
        let l = &ctx.isecs[loser];
        let (start, end) = (l.input_addr as u64, l.input_addr as u64 + l.size as u64);
        let lo = values.partition_point(|&v| v < start);
        let hi = values.partition_point(|&v| v < end);
        if values[lo..hi].iter().all(|&v| v == msym.value) {
            out.push((loser, winner as usize));
        }
    }
    out
}

/// Reports the symbols live objects define strongly more than once,
/// once resolution settles (and, in a final link, no symbol is
/// undefined), sorted by name: mold's check_duplicate_symbols.
/// Resolution keeps the first strong definition it meets; a weak or
/// common one yields quietly. As in ld-prime, under -dead_strip only a
/// symbol whose kept definition is live is an error, and
/// -allow_dead_duplicates lets one stay whose other definitions are all
/// dead.
pub fn check_duplicate_symbols<E: Target>(ctx: &Context<E>) {
    report_duplicates(ctx, duplicate_symbols(ctx, false));
}

/// Reports, before LTO, the symbols two bitcode files define strongly,
/// whose modules libLTO could not merge. A duplicate between bitcode and
/// a Mach-O object is left to the check after LTO, which finds it in a
/// compiled object (see lto::lto_roots).
pub fn check_bitcode_duplicates<E: Target>(ctx: &Context<E>) {
    if ctx.lto_modules.len() > 1 {
        report_duplicates(ctx, duplicate_symbols(ctx, true));
    }
}

/// A symbol defined strongly more than once: the file whose definition
/// won, the files whose definitions lost to it, and whether any of the
/// latter is live and the winner is.
struct Duplicate {
    sym: SymbolId,
    winner: usize,
    losers: Vec<usize>,
    any_live: bool,
    winner_live: bool,
}

/// The symbols defined strongly more than once - with `among_bitcode`,
/// by two bitcode files or more.
fn duplicate_symbols<E: Target>(ctx: &Context<E>, among_bitcode: bool) -> Vec<Duplicate> {
    // Each losing definition, and whether it is live.
    let mut losers: Vec<(SymbolId, usize, bool)> = ctx
        .objs
        .par_iter()
        .enumerate()
        .filter(|(_, obj)| obj.is_alive)
        .flat_map_iter(|(obj_idx, obj)| {
            obj.global_range().filter_map(move |i| {
                let (msym, sym_id) = (&obj.mach_syms[i], obj.symbols[i]);
                if msym.is_stab()
                    || !msym.is_extern()
                    || !matches!(msym.ty(), N_SECT | N_ABS)
                    || msym.desc & N_WEAK_DEF != 0
                    || !matches!(ctx.symbols[sym_id].file(), Some(FileId::Obj(owner)) if owner as usize != obj_idx)
                {
                    return None;
                }
                let isec = obj.symbol_subsec(&ctx.isecs, i);
                let live = isec.is_none_or(|(isec, _)| ctx.isecs[isec].is_alive());
                Some((sym_id, obj_idx, live))
            })
        })
        .collect();
    losers.sort_by_key(|&(sym_id, obj_idx, _)| (ctx.symbols[sym_id].name(), obj_idx));
    losers.dedup();

    let is_bitcode = |obj: usize| ctx.objs[obj].lto_module.is_some();
    let mut dups = Vec::new();
    for group in losers.chunk_by(|a, b| a.0 == b.0) {
        let id = group[0].0;
        let sym = &ctx.symbols[id];
        let Some(FileId::Obj(winner)) = sym.file() else { continue };
        let winner = winner as usize;
        let mut losers: Vec<usize> = group.iter().map(|&(_, obj, _)| obj).collect();
        let bitcode = losers.iter().chain([&winner]).filter(|&&obj| is_bitcode(obj)).count();
        if among_bitcode && bitcode < 2 {
            continue;
        }
        losers.sort_by_key(|&obj| ctx.objs[obj].priority);
        dups.push(Duplicate {
            sym: id,
            winner,
            losers,
            any_live: group.iter().any(|&(_, _, live)| live),
            winner_live: sym.input_section().is_none_or(|isec| ctx.isecs[isec as usize].is_alive()),
        });
    }
    dups
}

/// Reports duplicate symbols as mold does, an error for each definition
/// that lost: "duplicate symbol: <its file>: <the winner's file>:
/// <name>". A symbol whose kept definition -dead_strip left dead is no
/// error, nor, with -allow_dead_duplicates, one whose losing
/// definitions are all dead.
fn report_duplicates<E: Target>(ctx: &Context<E>, dups: Vec<Duplicate>) {
    let name = |obj: usize| -> error::RawBuf { ctx.objs[obj].mf.name.as_path().into() };
    for dup in dups {
        if !dup.winner_live || (ctx.args.allow_dead_duplicates && !dup.any_live) {
            continue;
        }
        for &loser in &dup.losers {
            error!(
                "duplicate symbol: {}: {}: {}",
                name(loser),
                name(dup.winner),
                ctx.symbols[dup.sym]
            );
        }
    }
}

/// -poison_symbol and -poison_symbols_list fail the link on any live
/// reference to a symbol they name, defined in the link or not: each
/// such symbol is listed, by name, with each subsection referring to
/// it, once. A SUBTRACTOR's symbol, subtracted, is no reference.
pub fn check_poisoned_symbols<E: Target>(ctx: &Context<E>) {
    let poisoned = &ctx.args.poisoned;
    if poisoned.is_empty() {
        return;
    }
    let mut refs: Vec<(SymbolId, usize)> = ctx
        .objs
        .par_iter()
        .filter(|obj| obj.is_alive)
        .flat_map_iter(|obj| obj.subsecs.iter().map(|&id| id as usize))
        .filter(|&isec| ctx.isecs[isec].is_alive())
        .flat_map_iter(|isec| {
            let file = ctx.isecs[isec].file as usize;
            let rels = input_files::isec_relocs_of(&ctx.objs, &ctx.isecs[isec]);
            (rels.iter())
                .filter(|rel| rel.ty != E::RELOC_SUBTRACTOR)
                .filter_map(move |rel| ctx.reloc_target_sym(file, rel))
                .filter(|&id| poisoned.find(ctx.symbols[id].name()) != -1)
                .map(move |id| (id, isec))
        })
        .collect();
    if refs.is_empty() {
        return;
    }
    // (A stable sort keeps each symbol's references in object order.)
    refs.sort_by_key(|&(id, _)| ctx.symbols[id].name());
    refs.dedup();
    let mut msg = b"Use of poisoned symbols:\n".to_vec();
    for group in refs.chunk_by(|a, b| a.0 == b.0) {
        let sym = &ctx.symbols[group[0].0];
        msg.extend(error::render(format_args!("  {sym}, referenced from:\n")));
        for &(_, isec) in group {
            let file = ctx.objs[ctx.isecs[isec].file as usize].mf.name.raw();
            let subsec = ctx.subsec_name(isec);
            let subsec = crate::util::demangle::display_name(&subsec);
            msg.extend(error::render(format_args!("      {subsec} in {file}\n")));
        }
    }
    error!("{}", raw(&msg));
}

/// -warn_commons warns of each tentative definition (common symbol)
/// the link keeps over a dylib's definition of the name (by default; a
/// missing `extern` in a header makes one), once for each such dylib,
/// and -commons error refuses one.
pub fn check_common_conflicts<E: Target>(ctx: &Context<E>) {
    use crate::cmdline::CommonsMode;
    let error = ctx.args.commons == CommonsMode::Error;
    let warn = ctx.args.warn_commons && ctx.args.commons == CommonsMode::IgnoreDylibs;
    if !warn && !error {
        return;
    }
    let mut commons: Vec<SymbolId> = (0..ctx.symbols.syms.len() as u32)
        .into_par_iter()
        .filter(|&id| ctx.symbols[id].is_common() && !ctx.symbols[id].is_defined())
        .collect();
    commons.par_sort_unstable_by_key(|&id| ctx.symbols[id].name());
    for id in commons {
        let sym = &ctx.symbols[id];
        let mut dylibs = ctx.dylibs.iter().filter(|d| d.exports.contains(sym.name())).peekable();
        if dylibs.peek().is_none() {
            continue;
        }
        // The first object declaring it.
        let declares = |obj: &&input_files::ObjectFile| {
            obj.is_alive
                && (obj.mach_syms.iter().zip(&obj.symbols)).any(|(n, &s)| s == id && n.is_common())
        };
        let Some(obj) = ctx.objs.iter().find(declares) else { continue };
        let (name, obj) = (raw(sym.name()), obj.mf.name.raw());
        for dylib in dylibs {
            let dylib = dylib.path.raw();
            if error {
                error!(
                    "common symbol '{name}' ({obj}) conflicts with definition from dylib '{name}' ({dylib})"
                );
            } else {
                crate::warn!(
                    "using common symbol '{name}' ({obj}) and ignoring definition from dylib '{name}' ({dylib})"
                );
            }
        }
    }
}

/// The imports an object references, each once, and whether weakly
/// (its undefined symbol is N_WEAK_REF).
fn import_references<E: Target>(ctx: &Context<E>, obj_idx: usize) -> Vec<(SymbolId, bool)> {
    let obj = &ctx.objs[obj_idx];
    let mut seen = hashbrown::HashSet::new();
    let mut out = Vec::new();
    for &id in &obj.subsecs {
        for rel in input_files::isec_relocs_of(&ctx.objs, &ctx.isecs[id as usize]) {
            let RelocTarget::Sym(idx) = rel.target() else { continue };
            let sym_id = obj.symbols[idx as usize];
            if ctx.symbols[sym_id].is_imported() && seen.insert(sym_id) {
                out.push((sym_id, obj.mach_syms[idx as usize].desc & N_WEAK_REF != 0));
            }
        }
    }
    out
}

/// -no_weak_imports and -weak_reference_mismatches error go through each
/// object's imports (see import_references): -no_weak_imports names
/// each one an object references weakly, and -weak_reference_mismatches
/// error names the object that references one otherwise than the
/// objects before it did (where any strong reference makes a strong
/// one).
pub fn check_weak_imports<E: Target>(ctx: &Context<E>) {
    use crate::cmdline::WeakRefMismatches;
    let mismatches = ctx.args.weak_reference_mismatches == WeakRefMismatches::Error;
    if (!ctx.args.no_weak_imports && !mismatches) || ctx.args.relocatable {
        return;
    }
    let refs: Vec<Vec<(SymbolId, bool)>> = (0..ctx.objs.len())
        .into_par_iter()
        .map(|i| match ctx.objs[i].is_alive {
            true => import_references(ctx, i),
            false => Vec::new(),
        })
        .collect();
    let mut weak_so_far: hashbrown::HashMap<SymbolId, bool> = hashbrown::HashMap::new();
    let (mut weak_found, mut mismatch_found) = (false, false);
    for (obj, refs) in ctx.objs.iter().zip(refs) {
        for (id, weak) in refs {
            let name = raw(ctx.symbols[id].name());
            if weak && ctx.args.no_weak_imports {
                crate::error::notice(format_args!(
                    "weak import of symbol '{name}' not supported because of option: -no_weak_imports"
                ));
                weak_found = true;
            }
            let all_weak = weak_so_far.entry(id).or_insert(weak);
            if mismatches && *all_weak != weak {
                let kind = if weak { "weak" } else { "non-weak" };
                crate::error::notice(format_args!(
                    "mismatching weak references for symbol: {name}, found {kind} import in {}",
                    obj.mf.name.raw()
                ));
                mismatch_found = true;
            }
            *all_weak &= weak;
        }
    }
    if weak_found {
        error!("weak imports not allowed");
    } else if mismatch_found {
        error!("weak import mismatches found");
    }
}

/// -warn_weak_exports names, by name, each weak definition the output
/// exports and each definition that overrides a dylib's weak one -
/// what makes dyld coalesce symbols at launch (MH_WEAK_DEFINES) - and
/// -no_weak_exports refuses a final image with any. ld-prime looks
/// once it has found no undefined or duplicate symbol.
pub fn check_weak_exports<E: Target>(ctx: &Context<E>) {
    if !ctx.args.warn_weak_exports && !ctx.args.no_weak_exports {
        return;
    }
    let mut found: Vec<(&[u8], bool)> = (0..ctx.symbols.syms.len() as u32)
        .into_par_iter()
        .filter_map(|id| {
            let defined_here = matches!(ctx.symbols[id].file(), Some(FileId::Obj(_)));
            if defined_here && ctx.exports_weak_def(id) {
                Some((ctx.symbols[id].name(), false))
            } else if ctx.overrides_weak_export(id) {
                Some((ctx.symbols[id].name(), true))
            } else {
                None
            }
        })
        .collect();
    found.par_sort_unstable();
    if ctx.args.warn_weak_exports {
        for (name, overrides) in &found {
            let name = raw(name);
            match overrides {
                true => crate::warn!("overrides weak external symbol: {name}"),
                false => crate::warn!("weak external symbol: {name}"),
            }
        }
    }
    if ctx.args.no_weak_exports && !found.is_empty() && !ctx.args.relocatable {
        error!("output has external weak-def symbols, but -no_weak_exports used");
    }
}

/// The symbols the output refers to that nothing defines. A DTrace
/// symbol is never defined (see dtrace).
fn unresolved_symbols<E: Target>(ctx: &Context<E>) -> Vec<SymbolId> {
    (0..ctx.symbols.syms.len() as SymbolId)
        .into_par_iter()
        .filter(|&id| {
            let sym = &ctx.symbols[id];
            sym.is_used() && !sym.is_defined() && !crate::dtrace::is_dtrace_symbol(sym.name())
        })
        .collect()
}

/// With `-undefined dynamic_lookup` (or -U naming one), references to
/// symbols that are still unresolved become flat-namespace imports that
/// dyld resolves against any loaded image at run time.
///
/// An alive object may name a symbol undefined that nothing refers to -
/// a .globl with neither a definition nor a relocation, as XNU declares
/// `SleepToken` under !WITH_CLASSIC_S2R. ld-prime drops such a name
/// without a word: no error (see report_undef_errors), and no import
/// under -undefined dynamic_lookup. Most links have no undefined symbol
/// at all, so the relocations are looked at only when there is one.
pub fn claim_unresolved_symbols<E: Target>(ctx: &mut Context<E>) {
    let undef = unresolved_symbols(ctx);
    if undef.is_empty() {
        return;
    }
    let referenced = referenced_symbols(ctx);
    // A name the command line insists on must resolve, even under
    // -undefined dynamic_lookup or -U: -u, the entry point, a name an
    // export list gives without wildcards, an -alias base. The alias
    // itself counts as defined.
    let initial: hashbrown::HashSet<SymbolId> = crate::dead_strip::initial_undefines(ctx).collect();
    let aliases = alias_symbols(ctx);

    // A -static image has no dyld to look a symbol up at run time, so
    // ld-prime lets none stay undefined, whatever -undefined or -U say.
    let args = &ctx.args;
    let may_look_up = |id: SymbolId| {
        !args.static_link
            && (args.undefined_dynamic_lookup
                || args.allowed_undefined.iter().any(|n| n.as_slice() == ctx.symbols[id].name()))
            && !initial.contains(&id)
    };
    let imports: Vec<SymbolId> = (undef.into_iter())
        .filter(|&id| {
            referenced[id as usize].load(std::sync::atomic::Ordering::Relaxed)
                && !aliases.contains(&id)
                && may_look_up(id)
        })
        .collect();
    for id in imports {
        let sym = &mut ctx.symbols[id];
        sym.set_file(FileId::Dylib(u32::MAX));
        sym.set_is_imported(true);
        sym.set_is_extern(true);
    }
}

/// The symbols -alias names, which count as defined.
fn alias_symbols<E: Target>(ctx: &Context<E>) -> hashbrown::HashSet<SymbolId> {
    ctx.args.aliases.iter().filter_map(|(_, alias)| ctx.symbols.get(alias)).collect()
}

/// Reports references to symbols that are still unresolved, those
/// claim_unresolved_symbols left. ld-prime reports them before
/// duplicate definitions, which it then leaves unreported.
pub fn report_undef_errors<E: Target>(ctx: &mut Context<E>) {
    let undef = unresolved_symbols(ctx);
    if undef.is_empty() {
        return;
    }
    let referenced = referenced_symbols(ctx);
    let initial: hashbrown::HashSet<SymbolId> = crate::dead_strip::initial_undefines(ctx).collect();
    let aliases = alias_symbols(ctx);
    let mut errors: Vec<SymbolId> = (undef.into_iter())
        .filter(|&id| {
            referenced[id as usize].load(std::sync::atomic::Ordering::Relaxed)
                && !aliases.contains(&id)
        })
        .collect();
    if errors.is_empty() {
        return;
    }

    // ld-prime points at the auto-linked libraries it could not find
    // first, and then reports the symbols by name, each with a file that
    // wants it.
    for msg in std::mem::take(&mut ctx.autolink_misses) {
        crate::warn!("{}", raw(&msg));
    }
    errors.par_sort_unstable_by_key(|&id| ctx.symbols[id].name());
    let referencers = first_referencers(ctx);
    for id in errors {
        let file: error::RawBuf = match referencers.get(&id) {
            Some(&obj) => ctx.objs[obj].mf.name.as_path().into(),
            None if initial.contains(&id) => "the command line".into(),
            None => "<synthesized>".into(),
        };
        error!("undefined symbol: {}: {}", file, ctx.symbols[id]);
    }
}

/// The first live object that names each symbol undefined.
fn first_referencers<E: Target>(ctx: &Context<E>) -> hashbrown::HashMap<SymbolId, usize> {
    let mut map = hashbrown::HashMap::new();
    for (obj_idx, obj) in ctx.objs.iter().enumerate().filter(|(_, obj)| obj.is_alive) {
        let r = obj.global_range();
        for (msym, &sym_id) in obj.mach_syms[r.clone()].iter().zip(&obj.symbols[r]) {
            if !msym.is_stab() && msym.ty() == N_UNDF && !msym.is_common() {
                map.entry(sym_id).or_insert(obj_idx);
            }
        }
    }
    map
}

/// The symbols something in the output refers to: the target of a live
/// relocation, the personality of a live function's unwind record or of
/// its FDE's CIE, an initializer __init_offsets names (whose pointer is
/// gone), or a name -u, -e or -alias insists on.
fn referenced_symbols<E: Target>(ctx: &Context<E>) -> Vec<std::sync::atomic::AtomicBool> {
    use std::sync::atomic::{AtomicBool, Ordering};
    let referenced: Vec<AtomicBool> =
        (0..ctx.symbols.syms.len()).map(|_| AtomicBool::new(false)).collect();
    ctx.isecs.par_iter().filter(|isec| isec.is_alive()).for_each(|isec| {
        for rel in input_files::isec_relocs_of(&ctx.objs, isec) {
            if let Some(id) = ctx.reloc_target_sym(isec.file as usize, rel) {
                referenced[id as usize].store(true, Ordering::Relaxed);
            }
        }
    });
    // A personality is named by an unwind record or a CIE rather than by
    // a relocation of a subsection, and an undefined one is looked up at
    // run time under -undefined dynamic_lookup like any other import.
    let alive = |isec: u32| ctx.isecs[isec as usize].is_alive();
    let personalities = ctx
        .unwind_records
        .iter()
        .filter(|rec| alive(rec.isec))
        .filter_map(|rec| rec.personality())
        .chain(
            ctx.fdes
                .iter()
                .filter(|fde| alive(fde.isec))
                .filter_map(|fde| ctx.cies[fde.cie as usize].personality),
        );
    for id in personalities {
        referenced[id as usize].store(true, Ordering::Relaxed);
    }
    for &func in &ctx.init_offsets.init_funcs {
        if let InitFunc::Imported(id) = func {
            referenced[id as usize].store(true, Ordering::Relaxed);
        }
    }
    for id in crate::dead_strip::initial_undefines(ctx) {
        referenced[id as usize].store(true, Ordering::Relaxed);
    }
    referenced
}

/// --print-dependencies prints, for every undefined symbol of every
/// object, which file's definition satisfied it - a line per edge:
/// "referencer<TAB>provider<TAB>u<TAB>symbol". Xcode's newer ld
/// grew this for build-graph auditing; it makes questions like "why
/// is this archive member in my binary" one grep.
pub fn print_dependencies<E: Target>(ctx: &Context<E>) {
    if !ctx.args.print_dependencies {
        return;
    }
    for (obj_idx, obj) in ctx.objs.iter().enumerate().filter(|(_, obj)| obj.is_alive) {
        let r = obj.global_range();
        for (msym, &sym_id) in obj.mach_syms[r.clone()].iter().zip(&obj.symbols[r]) {
            if msym.is_stab() || msym.ty() != N_UNDF || msym.is_common() {
                continue;
            }
            let sym = &ctx.symbols[sym_id];
            let provider = match sym.file() {
                Some(FileId::Obj(idx)) => {
                    let idx = idx as usize;
                    if !ctx.objs[idx].is_alive || idx == obj_idx {
                        continue;
                    }
                    ctx.objs[idx].mf.name.raw()
                }
                Some(FileId::Dylib(idx)) if idx != u32::MAX => {
                    raw(&ctx.dylibs[idx as usize].install_name)
                }
                _ => continue,
            };
            let line =
                format_args!("{}\t{}\tu\t{}\n", obj.mf.name.raw(), provider, raw(sym.name()));
            let _ = std::io::Write::write_all(&mut std::io::stdout(), &error::render(line));
        }
    }
}

/// -t lists the files the link read, once each, in the order read:
/// objects and stubs by the path they were found at, every member of an
/// archive as archive(member), and each library a stub re-exports (by
/// its install name if the stub inlines it).
pub fn print_trace<E: Target>(ctx: &Context<E>) {
    if !ctx.args.trace {
        return;
    }
    let mut seen = hashbrown::HashSet::new();
    let mut out = Vec::new();
    for name in ctx.traced_files.iter().filter(|name| seen.insert(*name)) {
        out.extend_from_slice(name);
        out.push(b'\n');
    }
    let _ = std::io::Write::write_all(&mut std::io::stdout(), &out);
}

/// -why_load reports, on stderr, what dragged each archive member into
/// the link, in input order: "'_symbol' caused load of
/// archive.a(member.o)", or the option that loads it whole, -force_load
/// (or -all_load, which says -force_load) or -ObjC. A bitcode member
/// LTO compiled counts, though no longer live.
pub fn print_why_load<E: Target>(ctx: &Context<E>) {
    if !ctx.args.why_load {
        return;
    }
    let compiled: hashbrown::HashSet<usize> = ctx.lto_inputs.iter().map(|i| i.obj).collect();
    for (i, obj) in ctx.objs.iter().enumerate() {
        let Some(archive) = obj.mf.parent else { continue };
        if !obj.is_alive && !compiled.contains(&i) {
            continue;
        }
        let file = obj.mf.name.raw();
        match ctx.why_load.get(&i) {
            Some(name) => {
                crate::error::notice(format_args!("'{}' caused load of {file}", raw(name)))
            }
            None => {
                let option = if ctx.args.all_load || ctx.force_loaded.contains(&archive.name) {
                    "-force_load"
                } else {
                    "-ObjC"
                };
                crate::error::notice(format_args!("{option} caused load of {file}"));
            }
        }
    }
}

/// -trace_implicit_libraries prints on stdout the libraries the link
/// brings in on its own: each live object's auto-link hints, and each
/// library loaded as the re-export of another, with that library's
/// file. With -trace_implicit_library, only the lines about the
/// libraries whose names hold one of its names.
pub fn print_implicit_trace<E: Target>(ctx: &Context<E>) {
    let args = &ctx.args;
    if !args.trace_implicit_libraries && args.trace_implicit_library.is_empty() {
        return;
    }
    let traced = |name: &[u8]| {
        args.trace_implicit_libraries
            || args.trace_implicit_library.iter().any(|s| memchr::memmem::find(name, s).is_some())
    };
    let mut out = Vec::new();
    for obj in ctx.objs.iter().filter(|obj| obj.is_alive) {
        for opt in &obj.linker_options {
            let (kind, name) = match opt.as_slice() {
                [flag, name] if flag.ends_with(b"framework") => ("framework", name.as_slice()),
                [lib] => match ["-hidden-l", "-needed-l", "-lazy-l", "-l"]
                    .iter()
                    .find_map(|prefix| lib.strip_prefix(prefix.as_bytes()))
                {
                    Some(name) => ("library", name),
                    None => continue,
                },
                _ => continue,
            };
            if traced(name) {
                let (name, file) = (raw(name), obj.mf.name.raw());
                let line = format_args!("auto-linking {kind} hint '{name}' from file '{file}'\n");
                out.extend(error::render(line));
            }
        }
    }
    // A library re-exported from a private location is merged into the
    // dylib re-exporting it; one from a public location is a dylib of
    // its own, unless the link names it itself.
    let mut seen = hashbrown::HashSet::new();
    for parent in &ctx.dylibs {
        let public = (parent.reexported.iter())
            .map(|edge| &ctx.dylibs[edge.dylib])
            .filter(|dylib| dylib.is_implicit)
            .map(|dylib| dylib.install_name.as_slice());
        let private = parent.merged_reexports.iter().map(Vec::as_slice);
        for name in private.chain(public) {
            if traced(name) && seen.insert(name) {
                let (name, file) = (raw(name), parent.path.raw());
                let line = format_args!("indirect library '{name}' from file '{file}'\n");
                out.extend(error::render(line));
            }
        }
    }
    let _ = std::io::Write::write_all(&mut std::io::stdout(), &out);
}

/// -assert-weak-l and the like load a dylib weakly but leave its
/// imports as the references make them, where -weak-l makes them all
/// weak: the link is refused if one isn't weak, naming the first such
/// dylib, each symbol by name, and the files that refer to it strongly
/// from what the output keeps.
pub fn check_weak_assertions<E: Target>(ctx: &Context<E>) {
    let asserted = |id: SymbolId| match ctx.symbols[id].file() {
        Some(FileId::Dylib(d)) if d != u32::MAX => {
            let dylib = &ctx.dylibs[d as usize];
            (dylib.is_weak_asserted && !ctx.symbols[id].is_weak_ref()).then_some(dylib)
        }
        _ => None,
    };
    if !ctx.dylibs.iter().any(|d| d.is_weak_asserted) {
        return;
    }
    // The strong references, by the file making them.
    let refs: Vec<(SymbolId, u32)> = (0..ctx.isecs.len())
        .into_par_iter()
        .filter(|&i| {
            let isec = &ctx.isecs[i];
            isec.is_emitted()
        })
        .flat_map_iter(|i| {
            let file = ctx.isecs[i].file;
            let obj = &ctx.objs[file as usize];
            ctx.isec_relocs(i).iter().filter_map(move |r| {
                let RelocTarget::Sym(idx) = r.target() else {
                    return None;
                };
                let id = obj.symbols[idx as usize];
                let strong = obj.mach_syms[idx as usize].desc & N_WEAK_REF == 0;
                (strong && asserted(id).is_some()).then_some((id, file))
            })
        })
        .collect();
    let mut by_sym: std::collections::BTreeMap<&[u8], (SymbolId, std::collections::BTreeSet<u32>)> =
        Default::default();
    for (id, file) in refs {
        by_sym.entry(ctx.symbols[id].name()).or_insert((id, Default::default())).1.insert(file);
    }
    let Some(dylib) =
        by_sym.values().filter_map(|&(id, _)| asserted(id)).min_by_key(|d| d.dylib_idx)
    else {
        return;
    };
    let install_name = raw(&dylib.install_name);
    let mut msg = error::render(format_args!(
        "Found non-weak-imported symbol(s) preventing {install_name} from being weak-linked:"
    ));
    for (name, (id, files)) in &by_sym {
        if !std::ptr::eq(asserted(*id).unwrap(), dylib) {
            continue;
        }
        msg.extend(error::render(format_args!("\n  \"{}\" imported from:", raw(name))));
        for &file in files {
            let file = ctx.objs[file as usize].mf.name.raw();
            msg.extend(error::render(format_args!("\n      {file}")));
        }
    }
    error!("{}", raw(&msg));
}

/// An image bound for the dyld shared cache may link only libraries
/// that are in it too, since the cache builder binds every dependency
/// inside the cache. ld-prime rejects the first dylib in load-command
/// order installed anywhere else (@rpath, /usr/local, /Library, ...);
/// one that -dead_strip_dylibs drops doesn't count.
fn check_shared_cache_deps<E: Target>(ctx: &Context<E>) {
    if !ctx.args.shared_region {
        return;
    }
    if let Some(dylib) = ctx
        .dylibs
        .iter()
        .filter(|d| !d.is_bundle_loader && !d.is_lazy)
        .filter(|d| !crate::cmdline::in_shared_cache_path(&d.install_name, ctx.args.platform))
        .min_by_key(|d| d.dylib_idx)
    {
        error!(
            "Shared cache eligible dylib cannot link to ineligible dylib '{}'.  Remove link to \
             ineligible dylib, fix its eligibility, or opt out of the shared cache using the \
             build setting 'LD_SHARED_CACHE_ELIGIBLE=NO' (or linker flag \
             '-not_for_dyld_shared_cache')",
            raw(&dylib.install_name)
        );
    }
}

/// Warns about each dylib the command line links that nothing binds
/// to, under -warn_unused_dylibs, which a dylib bound for the dyld
/// shared cache gets by default (see Args::warn_unused_dylibs). A
/// -needed_* or -reexport_* library is linked on purpose, and
/// libSystem, libc++ and Foundation, which compiler drivers and project
/// templates link by habit, are let off - by the start of the install
/// name, so a simulator's shallow Foundation.framework/Foundation too
/// (and, as ld-prime has it, /usr/lib/libSystemX.dylib, but not
/// /usr/lib/libc++abi.dylib). ld-prime warns once the link has turned
/// out to be sound, before it warns of redundant re-exports and weak
/// exports.
pub fn warn_unused_dylibs<E: Target>(ctx: &Context<E>) {
    if !ctx.args.warn_unused_dylibs {
        return;
    }
    const EXEMPT: [&[u8]; 3] = [
        b"/usr/lib/libSystem",
        b"/usr/lib/libc++.",
        b"/System/Library/Frameworks/Foundation.framework/",
    ];
    let mut bound = vec![false; ctx.dylibs.len()];
    for sym in &ctx.symbols.syms {
        if let Some(FileId::Dylib(idx)) = sym.file()
            && idx != u32::MAX
        {
            bound[idx as usize] = true;
        }
    }
    for (i, dylib) in ctx.dylibs.iter().enumerate() {
        if !bound[i] && dylib.is_bundle_loader {
            crate::warn!(
                "linking with bundle loader ({}) but not using any symbols from it",
                dylib.path.raw()
            );
        } else if !bound[i]
            && !dylib.is_implicit
            && !dylib.is_autolinked
            && !dylib.is_needed
            && !dylib.is_reexported
            && !dylib.is_bundle_loader
            && !EXEMPT.iter().any(|prefix| dylib.install_name.starts_with(prefix))
        {
            crate::warn!(
                "linking with ({}) but not using any symbols from it",
                raw(&dylib.install_name)
            );
        }
    }
}

/// Makes the exports that moved from a weakly loaded dylib to an older
/// library weak imports, though the older one loads as its own imports
/// say: ld-prime weak-imports the 39 symbols iTerm2 binds to
/// libswiftNetwork, since Network loads weakly (its two direct imports
/// are weak), yet loads libswiftNetwork strongly.
fn weaken_moved_imports<E: Target>(ctx: &mut Context<E>) {
    for dylib in ctx.dylibs.iter().filter(|d| d.is_weak) {
        for (&name, &target) in &dylib.moved_exports {
            if let Some(id) = ctx.symbols.get(name)
                && ctx.symbols[id].file() == Some(FileId::Dylib(target as u32))
            {
                ctx.symbols[id].set_is_weak_ref(true);
            }
        }
    }
}

/// For each dylib with a load command that stands for a library exports
/// moved to (see add_moved_dylibs), the library of the link with its
/// install name and a load command too, which then speaks for both:
/// ld-prime binds AppKit's moved exports to libswiftAppKit 1.0.0 for
/// macOS 13, but to 2775.10.103 if -lswiftAppKit names the SDK's stub
/// as well, while an auto-link option's stub with nothing bound to it
/// has no load command and changes nothing. Failing that, the first one
/// with a load command that stands for the library too, exports of
/// another library having moved there at another version: SwiftUI's
/// load command is at DeveloperToolsSupport's version if exports of
/// both it and SwiftUICore that moved to SwiftUI bind, at SwiftUICore's
/// if only those of SwiftUICore do.
fn moved_dylib_twins<E: Target>(ctx: &Context<E>, used: &[bool]) -> Vec<Option<usize>> {
    use input_files::NameSource;
    let dylibs = &ctx.dylibs;
    let moved = |i: usize| dylibs[i].name_source == NameSource::Moved;
    let twin = |i: usize, moved_too: bool| {
        (0..dylibs.len()).find(|&j| {
            used[j] && moved(j) == moved_too && dylibs[j].install_name == dylibs[i].install_name
        })
    };
    (0..dylibs.len())
        .map(|i| {
            if !used[i] || !moved(i) {
                return None;
            }
            twin(i, false).or_else(|| twin(i, true).filter(|&j| j != i))
        })
        .collect()
}

/// Gives the dylibs their ordinals (and so their load commands) in the
/// order they were first named: where the command line or an auto-link
/// option names one, or else where the dylib that re-exports it, or that
/// its exports moved from ($ld$previous), was named. Returns them in
/// that order. A lazy dylib has none: its imports' desc names the
/// image itself (ordinal 0), as ld-prime writes it.
fn assign_dylib_ordinals<E: Target>(ctx: &mut Context<E>) -> Vec<usize> {
    for dylib in ctx.dylibs.iter_mut().filter(|d| d.is_lazy) {
        dylib.dylib_idx = 0;
    }
    let mut order: Vec<usize> = (0..ctx.dylibs.len())
        .filter(|&i| !ctx.dylibs[i].is_bundle_loader && !ctx.dylibs[i].is_lazy)
        .collect();
    order.sort_by_key(|&i| ctx.dylibs[i].named_at.unwrap_or(ctx.dylibs[i].priority));
    for (ordinal, &i) in order.iter().enumerate() {
        ctx.dylibs[i].dylib_idx = ordinal as i32 + 1;
    }
    order
}

/// Drops the dylibs no symbol binds to that the link may drop (any under
/// -dead_strip_dylibs, else those it brought in itself: auto-linked, or
/// loaded as another dylib's public re-export), makes those every
/// reference to which is weak load weakly, and gives the rest their
/// load commands' ordinals, by which bind records name them.
pub fn dead_strip_dylibs<E: Target>(ctx: &mut Context<E>) {
    // An auto-linked dylib is stripped even without -dead_strip_dylibs,
    // as ld64 treats its option as a hint: NetNewsWire's auto-link
    // options name 43 frameworks and Swift overlays nothing in it binds
    // to, and ld-prime lists none of them. (ld-prime ignores
    // MH_DEAD_STRIPPABLE_DYLIB, with which ld64 stripped a dylib too.)
    let strippable = |dylib: &input_files::DylibFile| {
        ctx.args.dead_strip_dylibs || dylib.is_autolinked || dylib.is_implicit
    };

    // libSystem stays whether or not anything binds to it: ld-prime
    // keeps it under -dead_strip_dylibs in an image that binds nothing
    // from it (dyld needs it to run anything), so a dylib exporting
    // only its own functions still lists it.
    let mut used = vec![false; ctx.dylibs.len()];
    for (i, dylib) in ctx.dylibs.iter().enumerate() {
        used[i] = dylib.is_needed
            || dylib.install_name == b"/usr/lib/libSystem.B.dylib"
            || !strippable(dylib) && (dylib.is_reexported || !dylib.exports_moved_away(ctx));
    }
    // A dylib every reference to which is a weak import loads weakly
    // (LC_LOAD_WEAK_DYLIB), as ld64 does: the Swift overlays a program
    // reaches only through their weak __swift_FORCE_LOAD_$_ symbols
    // come out weak, libswiftCore strong.
    let mut bound = vec![0u32; ctx.dylibs.len()];
    let mut weak = vec![0u32; ctx.dylibs.len()];
    for sym in &ctx.symbols.syms {
        if let Some(FileId::Dylib(idx)) = sym.file()
            && idx != u32::MAX
        {
            used[idx as usize] = true;
            if sym.is_used() {
                bound[idx as usize] += 1;
                weak[idx as usize] += sym.is_weak_ref() as u32;
            }
        }
    }
    let twins = moved_dylib_twins(ctx, &used);
    for (i, &twin) in twins.iter().enumerate() {
        if let Some(twin) = twin {
            used[i] = false;
            bound[twin] += bound[i];
            weak[twin] += weak[i];
        }
    }
    for (i, dylib) in ctx.dylibs.iter_mut().enumerate() {
        if bound[i] > 0 && weak[i] == bound[i] {
            dylib.is_weak = true;
        }
    }
    weaken_moved_imports(ctx);
    remove_unused_dylibs(ctx, &used, &twins);

    let order = assign_dylib_ordinals(ctx);
    // Only a dylib can have an upward dependency, one that depends on
    // it in turn: ld-prime loads the library as usual for anything else,
    // with a warning.
    if ctx.args.output_type != MH_DYLIB {
        for &i in &order {
            let dylib = &mut ctx.dylibs[i];
            if dylib.is_upward {
                let name = raw(&dylib.install_name);
                crate::warn!("ignoring upward dylib option for {name}");
                dylib.is_upward = false;
            }
        }
    }
    check_shared_cache_deps(ctx);
    check_libsystem_linked(ctx);
}

/// Drops the dylibs not `used`, and points what refers to a dylib - a
/// symbol it defines, an export moved to it - at its new index, or at
/// its twin's if it has one (see moved_dylib_twins).
fn remove_unused_dylibs<E: Target>(ctx: &mut Context<E>, used: &[bool], twins: &[Option<usize>]) {
    let mut remap = vec![usize::MAX; ctx.dylibs.len()];
    let old = std::mem::take(&mut ctx.dylibs);
    for (i, dylib) in old.into_iter().enumerate() {
        if used[i] {
            remap[i] = ctx.dylibs.len();
            ctx.dylibs.push(dylib);
        }
    }
    for (i, &twin) in twins.iter().enumerate() {
        if let Some(twin) = twin {
            remap[i] = remap[twin];
        }
    }

    for sym in &mut ctx.symbols.syms {
        if let Some(FileId::Dylib(idx)) = sym.file()
            && idx != u32::MAX
        {
            sym.set_file(FileId::Dylib(remap[idx as usize] as u32));
        }
    }
    for dylib in &mut ctx.dylibs {
        for target in dylib.moved_exports.values_mut() {
            *target = remap[*target];
        }
    }
}

/// Has the imports from the libraries this image re-exports from
/// locations that aren't public bind to the image itself, where there
/// are two or more such libraries, as ld64 and ld-prime do (see
/// DylibFile::binds_to_image). A public location is no such one under
/// -no_implicit_dylibs, as for the libraries a dylib re-exports (see
/// input_files::is_public_location).
pub fn bind_private_reexports_to_image<E: Target>(ctx: &mut Context<E>) {
    let no_implicit = ctx.args.no_implicit_dylibs;
    let private = |d: &input_files::DylibFile| {
        d.is_reexported && (no_implicit || !input_files::is_public_location(&d.install_name))
    };
    if ctx.dylibs.iter().filter(|d| private(d)).count() >= 2 {
        for dylib in ctx.dylibs.iter_mut().filter(|d| private(d)) {
            dylib.binds_to_image = true;
        }
    }
}

/// ld64 takes a dynamic image that would load no dylib at all for one
/// linked without libSystem by mistake (a stray -nostdlib) and refuses
/// it: an executable other than a -static one, or a dylib or bundle,
/// -static or not. Any dylib left after -dead_strip_dylibs, the bundle
/// loader included, will do, libSystem or not, but a lazy one, which
/// has no load command, won't. ld-prime does the same and, like ld64,
/// lets off libsystem_kernel, which libSystem is built on, and any link
/// with an exit-asm.o (a stopgap for rdar://39514191). Firmware has no
/// libSystem to link.
fn check_libsystem_linked<E: Target>(ctx: &Context<E>) {
    if ctx.args.platform == crate::macho::PLATFORM_FIRMWARE {
        return;
    }
    let dynamic = match ctx.args.output_type {
        MH_EXECUTE => !ctx.args.static_link,
        MH_DYLIB | MH_BUNDLE => true,
        _ => false,
    };
    if !dynamic || ctx.dylibs.iter().any(|d| !d.is_lazy) {
        return;
    }
    let is_exit_asm = |obj: &input_files::ObjectFile| {
        memchr::memmem::find(path_bytes(&obj.mf.name), b"exit-asm.o").is_some()
    };
    if ctx.args.install_name.as_deref() == Some(b"/usr/lib/system/libsystem_kernel.dylib")
        || ctx.objs.iter().any(is_exit_asm)
    {
        return;
    }
    fatal!("dynamic executables or dylibs must link with libSystem.dylib");
}

/// Sets the address-taken bit of every subsection whose address the
/// image may observe, which identical code folding must then keep
/// apart - mold's --icf=safe. Mach-O objects carry no address
/// significance table, so, as mold infers it without one, anything but
/// a branch takes the address of what it refers to: an adrp/add pair,
/// a GOT load, a pointer or a difference in data. ld-prime counts only
/// the references the output keeps, after dead stripping. An exported
/// symbol's address is observable by any image that imports it.
pub fn compute_address_significance<E: Target>(ctx: &mut Context<E>) {
    let ctx_ref: &Context<E> = ctx;
    ctx_ref.isecs.par_iter().filter(|isec| isec.is_emitted()).for_each(|isec| {
        for r in input_files::isec_relocs_of(&ctx_ref.objs, isec) {
            if !r.is_func_call::<E>()
                && let Some(dst) = ctx_ref.reloc_target_isec(isec.file as usize, r)
            {
                ctx_ref.isecs[ctx_ref.resolve_isec(dst)].set_address_taken();
            }
        }
    });

    ctx_ref.symbols.syms.par_iter().for_each(|sym| {
        if matches!(sym.file(), Some(FileId::Obj(_)))
            && sym.is_extern()
            && !sym.is_private_extern()
            && let Some(isec) = sym.input_section()
        {
            ctx_ref.isecs[ctx_ref.resolve_isec(isec as usize)].set_address_taken();
        }
    });
}

/// Decides which symbols need a stub or a GOT slot, from how relocations
/// refer to them, and creates them. Only the relocations of subsections
/// the output keeps count: not those of a copy merged into another, such
/// as a losing weak definition. Swift's symbolic type references are
/// weak, and the copy in the object defining the type refers to its
/// descriptor directly while every other object's goes through a GOT
/// slot.
pub fn scan_relocations<E: Target>(ctx: &mut Context<E>) {
    // Scan relocations to find the symbols that need stubs or GOT slots.
    {
        let ctx_ref: &Context<E> = ctx;
        ctx_ref.isecs.par_iter().for_each(|isec| {
            if isec.is_emitted() {
                E::scan_relocations(ctx_ref, isec);
            }
        });

        // Personality functions are referenced from __unwind_info, and
        // from __eh_frame's CIEs, through the GOT.
        let unwind = ctx_ref.unwind_records.par_iter().filter_map(|rec| rec.personality());
        let cies =
            ctx_ref.fdes.par_iter().filter_map(|fde| ctx_ref.cies[fde.cie as usize].personality);
        unwind.chain(cies).for_each(|id| ctx_ref.symbols[id].add_flags(NEEDS_GOT));
    }
    // Exit if a thread-local was referred to as regular data, or the
    // reverse.
    crate::error::checkpoint();

    // Create the stubs and GOT slots in the order of the files that own
    // the symbols: each live object's, in its symbol table's order. A
    // symbol can appear more than once; its flags are gone after the
    // first.
    let objs: Vec<Vec<SymbolId>> = {
        let ctx_ref: &Context<E> = ctx;
        ctx_ref
            .objs
            .par_iter()
            .enumerate()
            .filter(|(_, file)| file.is_alive)
            .map(|(i, file)| {
                let id = FileId::Obj(i as u32);
                file.symbols
                    .iter()
                    .copied()
                    .filter(|&s| {
                        let sym = &ctx_ref.symbols[s];
                        sym.file() == Some(id) && sym.flags() != 0
                    })
                    .collect()
            })
            .collect()
    };
    let mut syms: Vec<SymbolId> = objs.into_iter().flatten().collect();

    // Then the rest: the symbols the linker made, which no object
    // lists, then the dylibs', dylib by dylib, and last those dyld looks
    // up in whatever image has them. Each file's go by name, the order
    // in which a dylib lists its exports (its export trie or .tbd).
    let mut rest: Vec<SymbolId> = {
        let ctx_ref: &Context<E> = ctx;
        let listed: hashbrown::HashSet<SymbolId> = syms.iter().copied().collect();
        (0..ctx_ref.symbols.syms.len() as SymbolId)
            .into_par_iter()
            .filter(|&id| ctx_ref.symbols[id].flags() != 0 && !listed.contains(&id))
            .collect()
    };
    let num_objs = ctx.objs.len();
    rest.sort_by_key(|&id| {
        let sym = &ctx.symbols[id];
        let file = match sym.file() {
            Some(FileId::Obj(i)) => i as usize,
            Some(FileId::Dylib(d)) if d != u32::MAX => num_objs + d as usize,
            _ => usize::MAX,
        };
        (file, sym.name())
    });
    syms.extend(rest);

    for id in syms {
        let flags = ctx.symbols[id].flags();
        if flags & NEEDS_GOT != 0 {
            chunks::got::add_got_symbol(ctx, id);
        }
        if flags & NEEDS_STUB != 0 {
            chunks::stubs::add_symbol(ctx, id);
        }
        ctx.symbols[id].clear_flags();
    }
}

/// Settles which stubs jump through a lazy pointer (and so have a stub
/// helper entry), once the stubs are made: scan_relocations', in the
/// order of the files that own the symbols, then the ones the passes
/// after it make.
pub fn finish_stubs<E: Target>(ctx: &mut Context<E>) {
    let stubs = &ctx.stubs.symbols;
    let lazy = |id| !ctx.binds_weak_lookup(id);
    let lazy_stubs: Vec<u32> = match ctx.args.lazy_binding {
        true => (0..stubs.len() as u32).filter(|&i| lazy(stubs[i as usize])).collect(),
        false => Vec::new(),
    };
    ctx.stubs.lazy = lazy_stubs;
}

/// Publishes selected imports without reexporting their whole dylib:
/// those -reexported_symbols_list names, and those an export list
/// matches - an export list re-exports a dylib's symbol it names (a
/// plain name is an initial undefine, so it is always in the link) or a
/// pattern of it matches one the link imports anyway.
pub fn create_symbol_reexports<E: Target>(ctx: &mut Context<E>) {
    let exported = ctx.args.exported_symbols.as_ref();
    if ctx.args.reexported_symbols.is_empty() && exported.is_none() {
        return;
    }
    let targets: Vec<_> = ctx
        .symbols
        .syms
        .iter()
        .enumerate()
        .filter(|(_, sym)| {
            let name = sym.name();
            matches!(sym.file(), Some(FileId::Dylib(_)))
                && (ctx.args.reexported_symbols.find(name) != -1
                    || exported.is_some_and(|exported| exported.find(name) != -1))
        })
        .map(|(i, _)| i as u32)
        .collect();
    // A symbol of a library the image re-exports whole (to which it
    // binds, so not one only that library re-exports in turn from a
    // public location) is exported already.
    let (redundant, targets): (Vec<u32>, Vec<u32>) = targets.into_iter().partition(|&id| {
        matches!(ctx.symbols[id].file(),
            Some(FileId::Dylib(d)) if ctx.dylibs.get(d as usize).is_some_and(|d| d.is_reexported))
    });
    ctx.redundant_reexports = redundant;
    let internal = ctx.internal_obj.unwrap() as u32;
    for target in targets {
        let name = ctx.symbols[target].name();
        // Keep the import as the target of references within this
        // image, and add a separate, same-name N_INDR export. Apple
        // emits both entries too; the export has no local address.
        let alias = ctx.symbols.add_local(name);
        let sym = &mut ctx.symbols[alias];
        sym.set_file(FileId::Obj(internal));
        sym.set_is_extern(true);
        ctx.indirect_aliases.push((alias, target));
        ctx.args.forced_undefined.push(name.to_vec());
    }
}

/// ld-prime adds nothing to the exports for a symbol an export list
/// would re-export that a library the image re-exports whole exports
/// already, but warns of each, naming that library's file. The
/// warnings come by symbol name.
pub fn warn_redundant_reexports<E: Target>(ctx: &Context<E>) {
    let mut found: Vec<(&[u8], &Path)> = (ctx.redundant_reexports.iter())
        .filter_map(|&id| {
            let Some(FileId::Dylib(d)) = ctx.symbols[id].file() else { return None };
            Some((ctx.symbols[id].name(), ctx.dylibs[d as usize].path.as_path()))
        })
        .collect();
    found.sort();
    for (name, file) in found {
        let name = raw(name);
        crate::warn!(
            "explicit re-export for symbol '{name}' is redundant because it is already re-exported from dylib '{}'",
            file.raw()
        );
    }
}

/// Defines the symbols the linker itself provides: those of the mach
/// header, the -alias names and the layout-boundary symbols.
pub fn add_synthetic_symbols<E: Target>(ctx: &mut Context<E>) {
    let internal = ctx.internal_obj.expect("internal object not created yet") as u32;
    let header_addr = mach_header_addr(ctx);
    // An executable exports its mach header as __mh_execute_header,
    // unless it is a -preload image, whose header is in no segment. A
    // dylib, a bundle or dyld may find its own by a name for its kind,
    // which ld-prime defines as it does ___dso_handle, out of the symbol
    // table.
    let header_name = match ctx.args.output_type {
        MH_EXECUTE if !ctx.args.preload => Some((&b"__mh_execute_header"[..], true)),
        MH_DYLIB => Some((&b"__mh_dylib_header"[..], false)),
        MH_BUNDLE => Some((&b"__mh_bundle_header"[..], false)),
        MH_DYLINKER => Some((&b"__mh_dylinker_header"[..], false)),
        _ => None,
    };
    if let Some((name, is_extern)) = header_name {
        define_header_symbol(ctx, name, internal, header_addr, is_extern);
    }
    // ___dso_handle identifies the image; C++ static destructors pass it
    // to __cxa_atexit. It resolves to the mach header but is never
    // exported.
    define_header_symbol(ctx, b"___dso_handle", internal, header_addr, false);

    add_aliases(ctx, internal);
    claim_boundary_symbols(ctx, internal);
}

/// Defines `name` at the mach header unless an input does, as a symbol
/// of the internal object: an external one, or a local one, which the
/// symbol table leaves out.
fn define_header_symbol<E: Target>(
    ctx: &mut Context<E>,
    name: &'static [u8],
    internal: u32,
    addr: u64,
    is_extern: bool,
) {
    let id = ctx.symbols.intern(name);
    let sym = &mut ctx.symbols[id];
    if !sym.is_defined() {
        sym.set_file(FileId::Obj(internal));
        sym.value = addr;
        sym.set_is_extern(is_extern);
    }
}

/// -alias gives an existing definition a second name: the new symbol
/// shares the original's subsection and offset, so it lands at the same
/// address and is exported alongside it. Apple uses aliases to publish
/// compatibility names (e.g. libSystem's dozens of $VARIANT names)
/// without touching the source. An undefined base is reported with the
/// other undefined symbols. Dead stripping keeps the base (see
/// dead_strip::initial_undefines) but drops an alias nothing exports or
/// refers to.
fn add_aliases<E: Target>(ctx: &mut Context<E>, internal: u32) {
    let aliases = std::mem::take(&mut ctx.args.aliases);
    for (existing, new) in &aliases {
        let Some(src) = ctx.symbols.get(existing).filter(|&id| ctx.symbols[id].is_defined()) else {
            continue;
        };
        let referenced = ctx.symbols.get(new).is_some_and(|id| ctx.symbols[id].is_used());
        if ctx.strips_dead_code()
            && !referenced
            && !(crate::dead_strip::keeps_export(ctx, new)
                && ctx.args.unexported_symbols.find(new) == -1)
        {
            continue;
        }
        let dst = ctx.symbols.intern(crate::util::leak_bytes(new.clone()));
        if ctx.symbols[dst].is_defined() {
            continue;
        }
        if ctx.symbols[src].is_imported() {
            // An alias of a dylib symbol is an indirect symbol
            // (N_INDR) whose export trie entry re-exports the dylib's
            // symbol under the new name; nothing here has an address.
            // ld64 does this for Xcode's
            // `-alias _NSExtensionMain ___debug_main_executable_dylib_entry_point`.
            let sym = &mut ctx.symbols[dst];
            sym.set_file(FileId::Obj(internal));
            sym.set_input_section(None);
            sym.value = 0;
            sym.set_is_extern(true);
            ctx.indirect_aliases.push((dst, src));
        } else {
            let (file, isec, value) = {
                let s = &ctx.symbols[src];
                (s.file(), s.input_section(), s.value)
            };
            let sym = &mut ctx.symbols[dst];
            sym.set_file(file.expect("alias of a defined symbol"));
            sym.set_input_section(isec);
            sym.value = value;
            sym.set_is_extern(true);
        }
    }
    ctx.args.aliases = aliases;
}

/// Claims ld64's layout-boundary symbols: an undefined reference to
/// section$start$__SEG$__sect (or $end$, or segment$start$__SEG /
/// segment$end$__SEG) resolves to the boundary's final address, and
/// wills the named section into existence if nothing else creates it.
/// Their values can only be known after layout, so they are patched in
/// fix_synthetic_symbols.
fn claim_boundary_symbols<E: Target>(ctx: &mut Context<E>, internal: u32) {
    for id in 0..ctx.symbols.syms.len() {
        let sym = &ctx.symbols[id];
        if !sym.is_used() || sym.is_defined() {
            continue;
        }
        let parsed = if let Some(rest) = sym.name().strip_prefix(b"section$") {
            split_once(rest, b'$').and_then(|(which, rest)| {
                split_once(rest, b'$').map(|(seg, sect)| (which == b"start", seg, Some(sect)))
            })
        } else if let Some(rest) = sym.name().strip_prefix(b"segment$") {
            split_once(rest, b'$').map(|(which, seg)| (which == b"start", seg, None))
        } else {
            None
        };
        let Some((is_start, seg, sect)) = parsed else {
            continue;
        };
        let sym = &mut ctx.symbols[id];
        sym.set_file(FileId::Obj(internal));
        sym.set_is_extern(false);
        ctx.boundary_syms.push((id as u32, is_start, seg, sect));
    }
}

/// Fills in the boundary symbols' addresses once every chunk and
/// segment has one.
pub fn fix_synthetic_symbols<E: Target>(ctx: &mut Context<E>) {
    for i in 0..ctx.boundary_syms.len() {
        let (id, is_start, seg, sect) = ctx.boundary_syms[i];
        let value = match sect {
            Some(sect) => {
                let Some(hdr) = ctx
                    .chunks
                    .iter()
                    .map(|&id| ctx.chunk_header(id))
                    .find(|hdr| hdr.is_sect && hdr.segname == seg && hdr.sectname == sect)
                else {
                    fatal!("no section for boundary symbol: {}", ctx.symbols[id]);
                };
                if is_start { hdr.addr } else { hdr.addr + hdr.size }
            }
            None => {
                let Some(segment) = ctx.segments.iter().find(|s| s.name == seg) else {
                    fatal!("no segment for boundary symbol: {}", ctx.symbols[id]);
                };
                if is_start { segment.cmd.vmaddr } else { segment.cmd.vmaddr + segment.cmd.vmsize }
            }
        };
        ctx.symbols[id].value = value;
    }

    // A -preload image's mach header is in no segment; ___dso_handle
    // names the start of __TEXT, where the header would be, as in
    // ld-prime.
    if ctx.args.preload
        && let Some(text) = ctx.segments.iter().find(|s| s.name == b"__TEXT")
        && let Some(id) = ctx.symbols.get(b"___dso_handle")
        && ctx.symbols[id].input_section().is_none()
    {
        ctx.symbols[id].value = text.cmd.vmaddr;
    }
}

/// The mach header's address, the start of its segment: where -segaddr
/// pins that segment, or else the image base.
fn mach_header_addr<E: Target>(ctx: &Context<E>) -> u64 {
    ctx.args.segaddr(header_segment(ctx)).unwrap_or(ctx.image_base())
}

/// Lays out the output: each segment's contents in file order, and the
/// segments in the address space. Where ld-prime puts a segment can
/// depend on the size of any other one (place_segments), so a segment
/// is laid out where it would go after the ones before it first and
/// moved once all are sized - all but the mach header's segment
/// (__TEXT), whose address is known up front (mach_header_addr) and
/// whose __unwind_info encodes the final addresses of its functions
/// (and of the others once they are placed: finish_unwind_info).
/// __LINKEDIT comes last: its tables read every other address.
pub fn set_osec_offsets<E: Target>(ctx: &mut Context<E>) {
    let linkedit = ctx.segments.len() - 1;
    debug_assert_eq!(ctx.segments[linkedit].name, b"__LINKEDIT");

    // Range-extension thunks go in before the first placement, unless
    // only the placement can tell whether a branch may be out of reach;
    // the code is then placed again with them if it turns out so.
    let need_thunks = crate::thunks::need_thunks(ctx);
    if need_thunks == Some(true) {
        crate::thunks::create_range_extension_thunks(ctx);
    }
    let mut fileoff = lay_out_segments_with_unwind_info(ctx);
    if need_thunks.is_none() && crate::thunks::code_span(ctx) > E::BRANCH_RANGE / 2 {
        crate::thunks::create_range_extension_thunks(ctx);
        fileoff = lay_out_segments_with_unwind_info(ctx);
    }

    while !chunks::chain_starts::finish_chain_starts(ctx) {
        fileoff = lay_out_segments(ctx);
    }

    // The thunk entries' addresses are recorded on their symbols now
    // that the sections are placed.
    crate::thunks::gather_thunk_addresses(ctx);

    check_segments(ctx);
    check_tlv_template(ctx);
    crate::error::checkpoint();

    // The fixup builders leave a text relocation's alignment alone.
    ctx.text_reloc_ranges = text_reloc_ranges(ctx);
    build_linkedit_tables(ctx);
    ctx.output_size = layout_segment(ctx, linkedit, fileoff, 0);
    place_linkedit(ctx);

    // Thread pointers are relative to the start of the first
    // thread-local data section.
    ctx.tls_begin = ctx
        .chunks
        .iter()
        .map(|&id| ctx.chunk_header(id))
        .filter(|hdr| hdr.is_thread_local())
        .map(|hdr| hdr.addr)
        .min()
        .unwrap_or(0);
}

/// The address ranges of the segments mapped without write permission,
/// where a pointer that needs a fixup (a rebase or a bind) is a text
/// relocation: the loader would have to make the segment writable to
/// apply it. None if the output may have text relocations
/// (Args::text_relocs) or has no fixups at all: an image nothing
/// slides has no rebases, and only a -static one has no binds either.
fn text_reloc_ranges<E: Target>(ctx: &Context<E>) -> Vec<Range<u64>> {
    if ctx.args.text_relocs || (ctx.args.static_link && !ctx.args.pie) {
        return Vec::new();
    }
    ctx.segments
        .iter()
        .filter(|seg| {
            seg.name != b"__PAGEZERO"
                && seg.name != b"__LINKEDIT"
                && chunks::segment_prots(ctx, seg).1 & VM_PROT_WRITE == 0
        })
        .map(|seg| seg.cmd.vmaddr..seg.cmd.vmaddr + seg.cmd.vmsize)
        .collect()
}

/// Fails the link on the text relocations found applying relocations,
/// listed by address, and on the 32-bit pointers of an x86-64 image
/// dyld loads, which it could neither slide nor bind.
pub fn report_text_relocs<E: Target>(ctx: &Context<E>) {
    let mut found = std::mem::take(&mut *ctx.text_relocs.lock().unwrap());
    let addr = |isec: u32, off: u32| ctx.isec_addr(isec as usize) + off as u64;
    found.sort_unstable_by_key(|&(isec, i)| {
        addr(isec, ctx.isec_relocs(isec as usize)[i as usize].offset)
    });
    if !found.is_empty() {
        crate::error::notice(format_args!("Illegal text-relocations:"));
    }
    for &(id, i) in &found {
        let isec = &ctx.isecs[id as usize];
        let rel = &ctx.isec_relocs(id as usize)[i as usize];
        let target = ctx.reloc_target_name(isec.file as usize, rel);
        crate::error::notice(format_args!(
            "  text-relocation in {} to '{}'",
            raw(&ctx.subsec_ref(id as usize, rel.offset)),
            raw(&target)
        ));
    }
    if !found.is_empty() {
        error!("Found illegal text-relocations");
    }

    let mut pointers32 = std::mem::take(&mut *ctx.pointers32.lock().unwrap());
    pointers32.sort_unstable_by_key(|&(isec, off)| addr(isec, off));
    for (isec, off) in pointers32 {
        error!(
            "32-bit pointer used in 64-bit code in {}",
            raw(&ctx.subsec_ref(isec as usize, off))
        );
    }
}

/// Lays out every segment but __LINKEDIT and gives each its address.
/// Returns the file offset past them.
fn lay_out_segments<E: Target>(ctx: &mut Context<E>) -> u64 {
    let header_seg = in_place_segment(ctx);
    let header_addr = mach_header_addr(ctx);
    let mut fileoff = 0;
    // A -preload image's mach header and load commands fill the file's
    // first pages, ahead of the segments, whose base stands for the
    // image's address.
    if ctx.args.preload {
        ctx.mach_header.hdr.addr = ctx.image_base();
        ctx.mach_header.hdr.size = mach_header_size(ctx);
        fileoff = align_to(ctx.mach_header.hdr.size, ctx.args.segment_align);
    }
    // The other segments follow the header's (or the image base), each
    // on its first section's alignment where that exceeds a page (only
    // an image no dyld maps allows one); place_segments moves them where
    // they go. The file skips as many bytes as memory does, unless a
    // segment is pinned (ld-prime).
    let mirror_gaps = ctx.args.segaddrs.is_empty();
    let mut addr = ctx.image_base();
    for seg_idx in 0..ctx.segments.len() - 1 {
        let name = ctx.segments[seg_idx].name;
        if name == b"__PAGEZERO" {
            fileoff = layout_segment(ctx, seg_idx, fileoff, 0);
            continue;
        }
        let vmaddr = if Some(name) == header_seg {
            fileoff = layout_segment(ctx, seg_idx, fileoff, header_addr);
            header_addr
        } else {
            let vmaddr = align_to(addr, segment_start_align(ctx, seg_idx));
            let gap = if mirror_gaps { vmaddr - addr } else { 0 };
            fileoff = layout_segment(ctx, seg_idx, fileoff + gap, vmaddr);
            vmaddr
        };
        addr = vmaddr + segment_span(ctx, &ctx.segments[seg_idx]);
    }
    place_segments(ctx);
    check_segment_overlaps(ctx);
    crate::error::checkpoint();
    fileoff
}

/// Lays out every segment but __LINKEDIT as lay_out_segments does, and
/// again until __unwind_info fits the room __TEXT leaves it (see
/// finish_unwind_info). Returns the file offset past them.
fn lay_out_segments_with_unwind_info<E: Target>(ctx: &mut Context<E>) -> u64 {
    loop {
        let fileoff = lay_out_segments(ctx);
        if finish_unwind_info(ctx) {
            return fileoff;
        }
    }
}

/// The segment holding the mach header, laid out in place at the image
/// base or its -segaddr: none in a -preload image, whose header
/// precedes every segment in the file.
fn in_place_segment<E: Target>(ctx: &Context<E>) -> Option<&'static [u8]> {
    (!ctx.args.preload).then(|| header_segment(ctx))
}

/// The boundary the segment after a segment starts on, in memory and in
/// the file: its -seg_page_size, else the page.
fn seg_page_size<E: Target>(ctx: &Context<E>, segname: &[u8]) -> u64 {
    let sizes = &ctx.args.seg_page_sizes;
    sizes.iter().find(|(name, _)| name == segname).map_or(ctx.args.segment_align, |&(_, size)| size)
}

/// The room a segment takes from the segments after it: its size up to
/// its -seg_page_size, which ld-prime leaves out of the size itself
/// (the XNU x86-64 kernel starts the segment after __TEXT on a 2 MiB
/// boundary that way).
fn segment_span<E: Target>(ctx: &Context<E>, seg: &OutputSegment) -> u64 {
    align_to(seg.cmd.vmsize, seg_page_size(ctx, seg.name))
}

/// The alignment of a segment's address: a page, or its first section's
/// alignment if greater.
fn segment_start_align<E: Target>(ctx: &Context<E>, seg_idx: usize) -> u64 {
    let first = ctx.segments[seg_idx].chunks.first().map_or(0, |&id| ctx.chunk_header(id).p2align);
    ctx.args.segment_align.max(1 << first)
}

/// __unwind_info is encoded as __TEXT is laid out, when only __TEXT's
/// addresses are final. If it covers code or LSDAs in other segments
/// too, this encodes it again now every segment has its address.
/// Returns false if that encoding needs more room than __TEXT left the
/// section; the layout is then done again with that much room (a
/// smaller one leaves zeros after it).
fn finish_unwind_info<E: Target>(ctx: &mut Context<E>) -> bool {
    if !ctx.chunks.contains(&ChunkId::UnwindInfo)
        || !chunks::unwind_info::covers_other_segments(ctx)
    {
        return true;
    }
    let size = chunks::unwind_info::compute_size(ctx);
    if size > ctx.unwind_info.hdr.size {
        ctx.unwind_info.min_size = size;
        return false;
    }
    true
}

/// Lays out a segment's chunks from file offset `fileoff` and address
/// `vmaddr`, and returns the file offset past the segment.
fn layout_segment<E: Target>(
    ctx: &mut Context<E>,
    seg_idx: usize,
    fileoff: u64,
    vmaddr: u64,
) -> u64 {
    if ctx.segments[seg_idx].name == b"__PAGEZERO" {
        let seg = &mut ctx.segments[seg_idx];
        seg.cmd.vmaddr = 0;
        seg.cmd.vmsize = ctx.args.pagezero_size;
        return fileoff;
    }
    // The kernel maps a static executable's stack from nothing in the
    // file.
    if ctx.segments[seg_idx].name == b"__UNIXSTACK" {
        let seg = &mut ctx.segments[seg_idx];
        seg.cmd.vmaddr = vmaddr;
        seg.cmd.vmsize = ctx.args.stack_size;
        return fileoff;
    }

    let mut cursor = fileoff;
    let chunk_ids = ctx.segments[seg_idx].chunks.clone();
    let linkedit = ctx.segments[seg_idx].name == b"__LINKEDIT";

    // The chunks with file contents, in file order. Aligned is the
    // address; the file offset keeps its distance from it, which is no
    // multiple of the alignment where a -preload image's header pages
    // shift the file. __LINKEDIT's tables, which nothing addresses, are
    // aligned in the file.
    for &id in &chunk_ids {
        if ctx.chunk_header(id).is_zerofill() {
            continue;
        }
        let align = 1 << ctx.chunk_header(id).p2align;
        let addr = if linkedit {
            cursor = align_to(cursor, align);
            vmaddr + (cursor - fileoff)
        } else {
            align_to(vmaddr + (cursor - fileoff), align)
        };
        cursor = fileoff + (addr - vmaddr);
        let size = match id {
            ChunkId::MachHeader => mach_header_size(ctx),
            // Encoded once its segment's addresses are known, as it
            // embeds __TEXT offsets.
            ChunkId::UnwindInfo => {
                chunks::unwind_info::compute_size(ctx).max(ctx.unwind_info.min_size)
            }
            // It holds a hash of every page before it.
            ChunkId::CodeSignature => chunks::code_signature::size(ctx, cursor),
            _ => ctx.chunk_header(id).size,
        };
        let hdr = ctx.chunk_header_mut(id);
        hdr.fileoff = cursor;
        hdr.addr = addr;
        hdr.size = size;
        cursor += size;
    }

    let filesize = cursor - fileoff;
    let mut vm_end = vmaddr + filesize;

    // Zero-fill chunks occupy address space after the file-backed part
    // of the segment.
    for &id in &chunk_ids {
        let hdr = ctx.chunk_header_mut(id);
        if !hdr.is_zerofill() {
            continue;
        }
        vm_end = align_to(vm_end, 1 << hdr.p2align);
        hdr.addr = vm_end;
        hdr.fileoff = 0;
        vm_end += hdr.size;
    }

    // __LINKEDIT's file contents end exactly at the code signature;
    // other segments are padded to a page boundary in the file, and the
    // next one starts on the segment's -seg_page_size boundary (which
    // ld-prime counts in __LINKEDIT's size, there being no next one).
    let page = ctx.args.segment_align;
    let seg_page = seg_page_size(ctx, ctx.segments[seg_idx].name);
    let seg = &mut ctx.segments[seg_idx];
    seg.cmd.vmaddr = vmaddr;
    seg.cmd.fileoff = fileoff;
    if linkedit {
        seg.cmd.filesize = filesize;
        seg.cmd.vmsize = align_to(vm_end - vmaddr, seg_page).max(filesize);
        return fileoff + filesize;
    }
    seg.cmd.filesize = align_to(filesize, page);
    seg.cmd.vmsize = align_to(vm_end - vmaddr, page).max(seg.cmd.filesize);
    fileoff + align_to(seg.cmd.filesize, seg_page)
}

/// Gives every segment but __LINKEDIT its address, as ld-prime does:
///
/// - A segment -segaddr pins goes there.
/// - With -segment_order, the segments listed after a pinned one
///   follow it, one after another (ld64's
///   segmentOrderAfterFixedAddressSegment).
/// - The others go from the image base, in segment order, each to the
///   lowest address where it runs into no segment placed before it.
///   The segments the two rules above place count as placed only from
///   the first of them (the mach header's segment aside) on, so the
///   segments ahead of it are simply laid out one after another - into
///   a pinned one, if it is in their way. A pinned __LINKEDIT, not
///   sized yet, counts from the start, as an empty segment.
/// - Where a pinned __TEXT is no base the others float from (see
///   is_pin_no_base), every pinned segment counts as placed from the
///   start, and a segment may follow one below the base.
fn place_segments<E: Target>(ctx: &mut Context<E>) {
    let base = ctx.image_base();
    let header_seg = in_place_segment(ctx);
    let segs = &ctx.segments[..ctx.segments.len() - 1];
    let range = |i: usize, addr: u64| addr..addr + segment_span(ctx, &segs[i]);

    // __PAGEZERO and the mach header's segment are laid out in place
    // already.
    let in_place: Vec<bool> =
        segs.iter().map(|seg| seg.name == b"__PAGEZERO" || Some(seg.name) == header_seg).collect();
    let mut addrs: Vec<Option<u64>> = (0..segs.len())
        .map(
            |i| if in_place[i] { Some(segs[i].cmd.vmaddr) } else { ctx.args.segaddr(segs[i].name) },
        )
        .collect();
    for i in 1..segs.len() {
        if addrs[i].is_none()
            && follows_pinned_segment(ctx, segs[i].name)
            && let Some(prev) = addrs[i - 1]
        {
            let end = prev + segment_span(ctx, &segs[i - 1]);
            addrs[i] = Some(align_to(end, segment_start_align(ctx, i)));
        }
    }

    let fixed: Vec<usize> =
        (0..segs.len()).filter(|&i| !in_place[i] && addrs[i].is_some()).collect();
    let detached = header_seg.is_some_and(|seg| is_pin_no_base(ctx, seg));
    let first_pin = if detached { Some(0) } else { fixed.first().copied() };
    let floor = if detached { 0 } else { base };
    let header = segs.iter().position(|seg| Some(seg.name) == header_seg);
    let mut used: Vec<Range<u64>> =
        header.map(|i| range(i, segs[i].cmd.vmaddr)).into_iter().collect();
    if let Some(addr) = ctx.args.segaddr(b"__LINKEDIT") {
        used.push(addr..addr);
    }
    for i in 0..segs.len() {
        if first_pin == Some(i) {
            used.extend(fixed.iter().map(|&j| range(j, addrs[j].unwrap())));
        }
        if addrs[i].is_none() {
            let size = segment_span(ctx, &segs[i]);
            let align = segment_start_align(ctx, i);
            let span = lowest_free_span(base, floor, size, align, &used);
            addrs[i] = Some(span.end - size);
            used.push(span);
        }
    }

    for (i, addr) in addrs.into_iter().enumerate() {
        if !in_place[i] {
            move_segment(ctx, i, addr.unwrap());
        }
    }
}

/// Whether -segment_order lists a segment after one that -segaddr pins
/// (ld64's segmentOrderAfterFixedAddressSegment).
fn follows_pinned_segment<E: Target>(ctx: &Context<E>, segname: &[u8]) -> bool {
    let mut pinned = false;
    for name in &ctx.args.segment_order {
        if name == segname {
            return pinned;
        }
        pinned |= ctx.args.segaddr(name).is_some();
    }
    false
}

/// Where `size` bytes go at the lowest address where they run into none
/// of the `used` ranges: `base` itself or the end of a used range above
/// `floor`, rounded up to `align`. Returns the span from that address
/// before rounding to the end, which the rounding's padding is part of.
/// An empty segment is a point no other segment may straddle, and still
/// needs an address no segment covers.
fn lowest_free_span(
    base: u64,
    floor: u64,
    size: u64,
    align: u64,
    used: &[Range<u64>],
) -> Range<u64> {
    let is_free = |span: &Range<u64>| {
        used.iter().all(|r| r.end <= span.start || span.end.max(span.start + 1) <= r.start)
    };
    std::iter::once(base)
        .chain(used.iter().map(|r| r.end).filter(|&end| end > floor))
        .map(|start| start..align_to(start, align) + size)
        .filter(is_free)
        .min_by_key(|span| span.start)
        .unwrap()
}

/// Moves a laid-out segment to `addr`.
fn move_segment<E: Target>(ctx: &mut Context<E>, seg_idx: usize, addr: u64) {
    let delta = addr.wrapping_sub(ctx.segments[seg_idx].cmd.vmaddr);
    ctx.segments[seg_idx].cmd.vmaddr = addr;
    for i in 0..ctx.segments[seg_idx].chunks.len() {
        let id = ctx.segments[seg_idx].chunks[i];
        let hdr = ctx.chunk_header_mut(id);
        hdr.addr = hdr.addr.wrapping_add(delta);
    }
}

/// Refuses segments that overlap once placed (see place_segments),
/// which only -segaddr pins (or an -image_base inside __PAGEZERO) can
/// make. Left out are empty segments and __LINKEDIT, sized last.
fn check_segment_overlaps<E: Target>(ctx: &Context<E>) {
    let segs = &ctx.segments[..ctx.segments.len() - 1];
    let span = |i: usize| segs[i].cmd.vmaddr..segs[i].cmd.vmaddr + segs[i].cmd.vmsize;
    for i in 0..segs.len() {
        for j in i + 1..segs.len() {
            let (a, b) = (span(i), span(j));
            if !a.is_empty() && !b.is_empty() && a.start < b.end && b.start < a.end {
                error!(
                    "custom segments overlap: {}({:#x}-{:#x}) {}({:#x}-{:#x})",
                    raw(segs[i].name),
                    a.start,
                    a.end,
                    raw(segs[j].name),
                    b.start,
                    b.end
                );
                return;
            }
        }
    }
}

/// Checks the segments and their sections before __LINKEDIT is laid
/// out, reporting the first error. In an image dyld slides, a segment
/// must not be below the one before it, nor a pinned __LINKEDIT below
/// the last. A section's file offset is 32 bits, and so its segment's
/// end: a section that ends past that is refused - in a segment that
/// ends at 4 GiB, which a -segalign of 2 GiB gives one, and in any with
/// a -segalign of 0, which leaves every segment empty. And a section
/// named __thread_data or __thread_bss, in any segment, is part of the
/// template dyld copies for each thread, which the variables' offsets
/// count from: one its first member doesn't type as thread-local data
/// is refused, as is a section of the template that doesn't follow the
/// one before, as a rename or a symbol move to another segment leaves
/// them (dyld copies the template as one block).
fn check_segments<E: Target>(ctx: &Context<E>) {
    let slides = dyld_slides(ctx);
    let (linkedit, segs) = ctx.segments.split_last().unwrap();
    // The last section of the template seen, with its place in the walk.
    let mut template: Option<(usize, &ChunkHeader)> = None;
    let mut nsects = 0;
    for (i, seg) in segs.iter().enumerate() {
        if slides && i > 0 && seg.cmd.vmaddr < segs[i - 1].cmd.vmaddr {
            error!("segment {} address is out of order", raw(seg.name));
            return;
        }
        let seg_end = (seg.cmd.fileoff + seg.cmd.filesize) as u32;
        for hdr in seg.chunks.iter().map(|&id| ctx.chunk_header(id)).filter(|hdr| hdr.is_sect) {
            if !hdr.is_zerofill() && hdr.fileoff + hdr.size > seg_end as u64 {
                error!(
                    "section {},{} file end ({}) goes past the segment end ({seg_end}) ",
                    raw(hdr.segname),
                    raw(hdr.sectname),
                    hdr.fileoff + hdr.size
                );
                return;
            }
            if matches!(hdr.sectname, b"__thread_data" | b"__thread_bss") && !hdr.is_thread_local()
            {
                error!("Missing TLV section flags in {},{}", raw(hdr.segname), raw(hdr.sectname));
                return;
            }
            if hdr.is_thread_local() {
                if let Some((n, prev)) = template
                    && n + 1 != nsects
                {
                    error!(
                        "TLV sections must be contiguous, but {},{} - {},{} aren't",
                        raw(prev.segname),
                        raw(prev.sectname),
                        raw(hdr.segname),
                        raw(hdr.sectname)
                    );
                    return;
                }
                template = Some((nsects, hdr));
            }
            nsects += 1;
        }
    }
    if slides
        && let (Some(addr), Some(last)) = (ctx.args.segaddr(linkedit.name), segs.last())
        && addr < last.cmd.vmaddr
    {
        error!("segment {} address is out of order", raw(linkedit.name));
    }
}

/// Reports thread-local data (of input sections so typed) that a rename
/// put in a section of another type, no part of the template: the
/// offset from the template's start its variables' descriptors hold
/// falls outside it. ld-prime reports data before the template, whose
/// offset wraps past 4GB; mold also data after it, of which ld-prime
/// writes an image dyld refuses, and data with no template left, on
/// which ld-prime crashes.
fn check_tlv_template<E: Target>(ctx: &Context<E>) {
    if ctx.output_sections.iter().any(|osec| osec.has_tlv_data && !osec.hdr.is_thread_local()) {
        error!("thread-locals too large.  Max 4GB for 64-bit architectures");
    }
}

/// __LINKEDIT goes where -segaddr pins it. Otherwise, in an image dyld
/// slides, it goes above every other segment, and in one that stays
/// where it was linked to the lowest address from the image base where
/// it fits, as any other segment would - which, with no segment pinned,
/// is above them too (a gap a segment's alignment left is no room).
fn place_linkedit<E: Target>(ctx: &mut Context<E>) {
    let linkedit = ctx.segments.len() - 1;
    let others = &ctx.segments[..linkedit];
    let addr = if let Some(addr) = ctx.args.segaddr(b"__LINKEDIT") {
        addr
    } else if dyld_slides(ctx) || ctx.args.segaddrs.is_empty() {
        others.iter().map(|seg| seg.cmd.vmaddr + segment_span(ctx, seg)).max().unwrap_or(0)
    } else {
        let used: Vec<Range<u64>> = others
            .iter()
            .map(|seg| seg.cmd.vmaddr..seg.cmd.vmaddr + segment_span(ctx, seg))
            .collect();
        let size = ctx.segments[linkedit].cmd.vmsize;
        let base = ctx.image_base();
        lowest_free_span(base, base, size, ctx.args.segment_align, &used).start
    };
    move_segment(ctx, linkedit, addr);
}

/// Whether dyld loads the image wherever it likes: a PIE executable, a
/// dylib or a bundle, but not a -static image or a non-PIE executable.
fn dyld_slides<E: Target>(ctx: &Context<E>) -> bool {
    !ctx.args.static_link && (ctx.args.output_type != MH_EXECUTE || ctx.args.pie)
}

/// Whether ld-prime takes the -segaddr of the mach header's segment,
/// `segname`, for no base address the other segments float from: in an
/// image dyld slides, unless it is a dylib's or a bundle's preferred
/// address, which ld-prime honors without chained fixups (see
/// cmdline::resolve_image_base; a PIE's it ignores).
fn is_pin_no_base<E: Target>(ctx: &Context<E>, segname: &[u8]) -> bool {
    ctx.args.segaddr(segname).is_some()
        && dyld_slides(ctx)
        && (ctx.args.output_type == MH_EXECUTE || ctx.args.fixup_chains)
}

/// Builds the __LINKEDIT tables, once every other address is final
/// (the symbol table needs none at all). They are independent of one
/// another, so they build as one parallel task group; layout_segment
/// then places each by the size its contents give it.
fn build_linkedit_tables<E: Target>(ctx: &mut Context<E>) {
    let shared = &*ctx;
    let ((symtab, trie), (fixups, (starts, (dice, split)))) = rayon::join(
        || {
            let globals = sorted_globals(shared);
            rayon::join(
                || {
                    let _t = shared.timer("symtab");
                    chunks::symtab::create_output_symtab(shared, &globals)
                },
                || {
                    let _t = shared.timer("trie_encode");
                    chunks::export_trie::encode_export_trie(shared, &globals)
                },
            )
        },
        || {
            rayon::join(
                || build_fixups(shared),
                || {
                    rayon::join(
                        || {
                            let _t = shared.timer("function_starts");
                            chunks::function_starts::build(shared)
                        },
                        || {
                            let _t = shared.timer("data_in_code");
                            let dice = chunks::data_in_code::build(shared, |hdr| hdr.fileoff);
                            (dice, chunks::split_info::build(shared))
                        },
                    )
                },
            )
        },
    );

    ctx.symtab = symtab;
    ctx.symtab.hdr.size = (ctx.symtab.len() * size_of::<MachSym>()) as u64;
    ctx.strtab.hdr.size = ctx.symtab.strtab_size as u64;
    ctx.data_in_code.hdr.size = (dice.len() * 8) as u64;
    ctx.data_in_code.entries = dice;
    ctx.split_info.hdr.size = split.len() as u64;
    ctx.split_info.contents = split;
    match fixups {
        Fixups::Chained((contents, fixups, imports, ordinals)) => {
            let sec = &mut ctx.chained_fixups;
            sec.hdr.size = contents.len() as u64;
            sec.contents = contents;
            sec.fixups = fixups;
            sec.imports = imports;
            sec.ordinals = ordinals;
        }
        Fixups::Classic { rebase, bind, weak_bind, lazy_bind, lazy_offsets } => {
            // An image laid out for chains falls back to these.
            ctx.chained_fixups.disabled = ctx.args.fixup_chains;
            ctx.rebase_info.hdr.size = rebase.len() as u64;
            ctx.rebase_info.contents = rebase;
            ctx.bind_info.hdr.size = bind.len() as u64;
            ctx.bind_info.contents = bind;
            ctx.weak_bind_info.hdr.size = weak_bind.len() as u64;
            ctx.weak_bind_info.contents = weak_bind;
            ctx.lazy_bind_info.hdr.size = lazy_bind.len() as u64;
            ctx.lazy_bind_info.contents = lazy_bind;
            ctx.lazy_bind_info.offsets = lazy_offsets;
        }
    }
    ctx.function_starts.hdr.size = starts.len() as u64;
    ctx.function_starts.contents = starts;
    ctx.export_trie.hdr.size = trie.len() as u64;
    ctx.export_trie.contents = trie;
    // The relocations of an image no dyld loads (LC_DYSYMTAB's): a
    // -static -pie image's local ones, and a kext's local and external
    // ones.
    if ctx.chunks.contains(&ChunkId::LocalRelocs) {
        chunks::local_relocs::build(ctx);
    }
    if ctx.chunks.contains(&ChunkId::ExternRelocs) {
        chunks::extern_relocs::build(ctx);
    }
    if ctx.chunks.contains(&ChunkId::MergeableRecord) {
        let _t = ctx.timer("mergeable_record");
        crate::make_mergeable::build(ctx);
    }
}

/// The defined globals the output keeps, sorted by name, which both the
/// symbol table and the export trie list: sorted once for the two, as a
/// debug link has hundreds of thousands of long mangled names.
fn sorted_globals<E: Target>(ctx: &Context<E>) -> Vec<SymbolId> {
    let _t = ctx.timer("globals_sort");
    let mut globals: Vec<SymbolId> = (0..ctx.symbols.syms.len() as SymbolId)
        .into_par_iter()
        .filter(|&id| {
            let sym = &ctx.symbols[id];
            sym.is_extern()
                && !sym.is_private_extern()
                && matches!(sym.file(), Some(FileId::Obj(_)))
                && (sym.input_section())
                    .is_none_or(|isec| ctx.isecs[ctx.resolve_isec(isec as usize)].is_alive())
        })
        .collect();
    globals.par_sort_unstable_by_key(|&id| crate::util::name_sort_key(ctx.symbols[id].name()));
    globals
}

/// The fixups dyld applies, as __LINKEDIT encodes them: chained, or the
/// classic dyld opcodes (with the offsets of the lazy binds in theirs).
enum Fixups {
    Chained(chunks::chained_fixups::ChainedFixups),
    Classic {
        rebase: Vec<u8>,
        bind: Vec<u8>,
        weak_bind: Vec<u8>,
        lazy_bind: Vec<u8>,
        lazy_offsets: Vec<u32>,
    },
}

/// Encodes the fixups: chained if the image uses chained fixups and
/// every pointer suits them (see build_chained_fixups), else as the
/// classic dyld opcodes.
fn build_fixups<E: Target>(ctx: &Context<E>) -> Fixups {
    if ctx.use_chained_fixups() {
        let _t = ctx.timer("chained_fixups");
        if let Some(chained) = chunks::chained_fixups::build_chained_fixups(ctx) {
            return Fixups::Chained(chained);
        }
    } else {
        chunks::chained_fixups::check_classic_pointers(ctx);
    }
    let (rebase, bind) = rayon::join(
        || {
            let _t = ctx.timer("rebase_info");
            chunks::rebase_info::build(ctx)
        },
        || {
            let _t = ctx.timer("bind_info");
            chunks::bind_info::build(ctx)
        },
    );
    let (lazy_bind, lazy_offsets) = chunks::lazy_bind_info::build(ctx);
    let weak_bind = chunks::weak_bind_info::build(ctx);
    Fixups::Classic { rebase, bind, weak_bind, lazy_bind, lazy_offsets }
}

/// Resolves the entry point symbol.
pub fn resolve_entry<E: Target>(ctx: &mut Context<E>) {
    if !ctx.args.has_entry_point() {
        return;
    }
    match ctx.symbols.get(&ctx.args.entry) {
        // An entry point in a dylib (an app extension's
        // _NSExtensionMain): LC_MAIN must point into __TEXT, so it
        // names the symbol's stub, as ld64 does.
        Some(id) if ctx.symbols[id].is_imported() => ctx.entry_addr = ctx.sym_stub_addr(id),
        Some(id) if ctx.symbols[id].is_defined() => ctx.entry_addr = ctx.sym_addr(id),
        _ => {
            error!(
                "undefined symbol for entry point: {}",
                crate::util::demangle::display_name(&ctx.args.entry)
            )
        }
    }
}

/// Flags an entry point that resolved to a dylib export for the stub
/// that LC_MAIN will name, which scan_relocations makes with the stubs
/// of the branch targets.
pub fn add_entry_stub<E: Target>(ctx: &mut Context<E>) {
    if !ctx.args.has_entry_point() {
        return;
    }
    if let Some(id) = ctx.symbols.get(&ctx.args.entry)
        && ctx.symbols[id].is_imported()
    {
        ctx.symbols[id].add_flags(NEEDS_STUB);
    }
}

/// Finds the helper legacy LINKEDIT's stub helper entries jump to. It
/// binds no dyld_stub_binder: its entries go to dyld_stub_binding_helper,
/// which crt1.o, dylib1.o or bundle1.o defines; no dylib exports it.
/// (Otherwise dyld_stub_binder is bound once a stub needs it; see
/// chunks::stub_helper::ensure_stub_binder.)
pub fn resolve_stub_binder<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.legacy_linkedit {
        let id = ctx.symbols.get(b"dyld_stub_binding_helper");
        let id = id.filter(|&id| ctx.symbols[id].input_section().is_some());
        ctx.stub_helper.binding_helper = id;
    }
}

/// Computes the UUID that identifies the build and writes it into the
/// header's LC_UUID, and returns the SHA256 hashes of every 4KiB page
/// before the code signature at `sig_start`, which the signature is
/// made of. This is mold's write_build_id: a Mach-O build ID is its
/// UUID.
///
/// The UUID is derived from those page hashes rather than from a second
/// pass over the contents: the pages are hashed while the LC_UUID field
/// is still zero, the array is hashed once more and stamped as a
/// version-4 UUID, and only the pages the header spans are hashed again
/// for the signature. The circularity - the signature covers the
/// header, the header holds the UUID - is broken by the zeroed field,
/// the way ld64 hashes with the UUID zeroed. Like ld64's, the UUID
/// depends on the contents before the signature only, not on the
/// signature blob (whose identifier is the output's basename); unsigned
/// output hashes its pages the same way. A -random_uuid one goes in
/// before anything is hashed.
pub fn compute_uuid<E: Target>(
    ctx: &Context<E>,
    buf: &mut [u8],
    sig_start: usize,
) -> Vec<[u8; 32]> {
    let set_uuid = |uuid: &[u8], buf: &mut [u8]| {
        let mut uuid: [u8; 16] = uuid[..16].try_into().unwrap();
        uuid[6] = (uuid[6] & 0x0f) | 0x40; // version 4
        uuid[8] = (uuid[8] & 0x3f) | 0x80; // RFC 4122 variant
        *ctx.uuid.lock().unwrap() = uuid;
        chunks::write_uuid(ctx, buf);
    };
    let content_uuid = ctx.args.uuid && !ctx.args.random_uuid;
    if ctx.args.uuid && ctx.args.random_uuid {
        let mut uuid = [0; 16];
        crate::util::random_bytes(&mut uuid);
        set_uuid(&uuid, buf);
    }
    let mut hashes: Vec<[u8; 32]> = Vec::new();
    if content_uuid || ctx.args.adhoc_codesign {
        let _t = ctx.timer("page_hashes");
        hashes = chunks::code_signature::page_hashes(&buf[..sig_start]);
    }
    if content_uuid {
        let _t = ctx.timer("uuid");
        // The build system's salt goes in first (see Args::uuid_salt).
        let mut flat: Vec<u8> = ctx.args.uuid_salt.clone();
        flat.extend(hashes.concat());
        let mut hash = [0; 32];
        crate::util::sha256(&flat, &mut hash);
        set_uuid(&hash, buf);
        let hdr_end = ctx.mach_header.hdr.size as usize;
        chunks::code_signature::rehash_pages(&buf[..sig_start], &mut hashes, 0..hdr_end);
    }
    hashes
}
