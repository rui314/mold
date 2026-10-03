//! The linker passes, in the order the driver runs them.

use std::ops::Range;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::chunks::init_offsets::InitFunc;
use crate::chunks::{self, ChunkHeader, ChunkId, OutputSegment, mach_header_size};
use crate::cmdline::{Args, Treatment};
use crate::context::Context;
use crate::error;
use crate::error::RawPath;
use crate::error::raw;
use crate::fatal;
use crate::input_files;
use crate::input_files::FileId;
use crate::input_sections::{InputSection, NO_REPLACEMENT, RelocTarget};
use crate::macho::*;
use crate::mapped_file::MappedFile;
use crate::objc::{DataBlob, DataField};
use crate::output_sections::header_segment;
use crate::symbol::{NO_IDX, Symbol, SymbolId};
use crate::target::RelocClass;
use crate::target::Target;
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
/// competing definitions (strong > weak > lazy archive member or
/// dylib > common), breaking ties by input order. A liveness walk then
/// marks the archive members whose definitions are actually referenced
/// (see resolve_and_mark_live), and the live objects' auto-link options
/// load the libraries they name; objects among those, or a library that
/// changes what an earlier one stands for, have resolution and the walk
/// start over. A final round restricted to live files settles the
/// owners, as in mold's resolve_symbols.
pub fn resolve_symbols<E: Target>(ctx: &mut Context<E>) {
    intern_command_line_symbols(ctx);
    // The final round ranks the dylibs as they were before the last
    // auto-linked ones came: those claim only what is left undefined
    // (see claim_new_dylibs).
    let (ranking, num_dylibs) = loop {
        resolve_and_mark_live(ctx);
        let ranking = DylibRanking::new(&ctx.dylibs);
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

/// Resolves the symbols with every object taking part, the lazy archive
/// members too, and marks the live objects. A member the walk loads may
/// bring tentative definitions of symbols the round didn't rank as
/// such, which want what defines them as data (see definition_rank):
/// if a lazy member is what does, the two run again for the walk to
/// load it. Those symbols are the only ones another walk could load
/// anything for: every other symbol a live file needs, this walk has
/// left to a live file, a dylib or none.
fn resolve_and_mark_live<E: Target>(ctx: &mut Context<E>) {
    loop {
        clear_symbols(ctx);
        let ranking = DylibRanking::new(&ctx.dylibs);
        let tentative = resolve_symbols_pass(ctx, false, &ranking);
        let new_tentative = mark_live_objects(ctx, &tentative);
        if !member_overrides(ctx, &new_tentative) {
            break;
        }
    }
}

/// Whether a lazy archive member would own one of `tentative`, symbols
/// a live file now has a tentative definition of, in a round that ranks
/// them so: one that defines it as data where no live file defines it
/// (see definition_rank). The objects' definitions of those symbols
/// alone race for them here.
fn member_overrides<E: Target>(ctx: &Context<E>, tentative: &Tentative) -> bool {
    use std::sync::atomic::{AtomicU64, Ordering};
    if tentative.is_empty() {
        return false;
    }
    let index: hashbrown::HashMap<SymbolId, usize> =
        tentative.iter().enumerate().map(|(k, &id)| (id, k)).collect();
    let best: Vec<AtomicU64> = (0..index.len()).map(|_| AtomicU64::new(u64::MAX)).collect();
    ctx.objs.par_iter().for_each(|obj| {
        for i in obj.global_range() {
            let Some(&k) = index.get(&obj.symbols[i]) else { continue };
            let rank = definition_rank(&ctx.isecs, obj, i, !obj.is_alive, ctx.autolink_priority);
            if let Some(rank) = rank {
                best[k].fetch_min(rank, Ordering::Relaxed);
            }
        }
    });
    best.iter().any(|rank| rank.load(Ordering::Relaxed) >> 40 == 2)
}

/// The symbols the command line names, which count as referenced: the
/// -u ones, the entry point of an image that has one (not of a -r
/// output, whose output type is still the executable default: it would
/// carry a spurious undefined _main), and the -alias bases (Xcode
/// aliases an app extension's debug dylib entry point to Foundation's
/// _NSExtensionMain).
fn command_line_symbols(args: &Args) -> impl Iterator<Item = &[u8]> {
    let entry = args.has_entry_point().then_some(args.entry.as_slice());
    let aliased = args.aliases.iter().map(|(existing, _)| existing.as_slice());
    args.forced_undefined.iter().map(Vec::as_slice).chain(entry).chain(aliased)
}

/// Symbols the command line names exist even when no object mentions
/// them, so that a dylib export can claim them: an app extension's
/// entry point, _NSExtensionMain, lives in Foundation and nothing in the
/// extension references it. So do the runtime routines LTO may come to
/// call, so that a library can provide them.
fn intern_command_line_symbols<E: Target>(ctx: &mut Context<E>) {
    let softloaded = may_softload_runtime_routines(ctx).then_some(LTO_RUNTIME_ROUTINES);
    let new: Vec<&'static [u8]> = command_line_symbols(&ctx.args)
        .chain(softloaded.into_iter().flatten())
        .filter(|name| ctx.symbols.get(name).is_none())
        .map(|name| crate::util::leak_bytes(name.to_vec()))
        .collect();
    for name in new {
        ctx.symbols.intern(name);
    }
}

/// The runtime routines ld-prime "softloads" for LTO, whose code
/// generator may call them where no input did: it looks each up in the
/// libraries before LTO as if something referenced it, loading the
/// archive member or binding to the dylib that provides it first, but
/// takes none as missing. Its list is its own (libLTO's
/// lto_runtime_lib_symbols_list is longer and lacks _strcpy): these are
/// the names of compiler-rt's builtins, libm and libc it was found to
/// load, for x86-64 and arm64 alike.
pub const LTO_RUNTIME_ROUTINES: [&[u8]; 8] = [
    b"___divsi3",
    b"___gtdf2",
    b"___ltdf2",
    b"___muldi3",
    b"___udivdi3",
    b"___udivsi3",
    b"_memset",
    b"_strcpy",
];

/// Whether the link softloads LTO_RUNTIME_ROUTINES: once bitcode is in
/// it, with -lto_softload_runtime_symbols or in a -static or -preload
/// image (Args::lto_softload).
pub fn softloads_runtime_routines<E: Target>(ctx: &Context<E>) -> bool {
    ctx.args.lto_softload
        && (!ctx.lto_inputs.is_empty()
            || ctx.lto_modules.iter().any(|module| ctx.objs[module.obj].is_alive))
}

/// Whether the link may softload LTO_RUNTIME_ROUTINES: any bitcode file
/// is in it or could join it, live or not.
fn may_softload_runtime_routines<E: Target>(ctx: &Context<E>) -> bool {
    ctx.args.lto_softload && !(ctx.lto_modules.is_empty() && ctx.lto_inputs.is_empty())
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
            let nlist = &obj.nlists[i];
            if nlist.is_stab() || nlist.is_extern() {
                continue;
            }
            // SAFETY: disjoint per object, as above.
            let sym = unsafe { syms.get(obj.symbols[i]) };
            let file = FileId::Obj(obj_idx as u32);
            match nlist.n_type() {
                N_ABS => {
                    sym.set_file(file);
                    sym.set_input_section(None);
                    sym.value = nlist.n_value;
                }
                N_SECT => {
                    if let Some((isec, off)) = obj.symbol_subsec(isecs, i) {
                        sym.set_file(file);
                        sym.set_input_section(Some(isec as u32));
                        sym.value = off;
                        sym.set_no_dead_strip(
                            nlist.n_desc & (N_NO_DEAD_STRIP | REFERENCED_DYNAMICALLY) != 0,
                        );
                        sym.set_is_alt_entry(nlist.n_desc & N_ALT_ENTRY != 0);
                    }
                }
                _ => {}
            }
        }
    });
}

/// The symbol table's slots, for a parallel loop that writes each symbol
/// from one thread at most.
struct SymbolSlots<'a> {
    ptr: *mut Symbol,
    _marker: std::marker::PhantomData<&'a mut [Symbol]>,
}

unsafe impl Sync for SymbolSlots<'_> {}

impl<'a> SymbolSlots<'a> {
    fn new(syms: &'a mut [Symbol]) -> Self {
        Self { ptr: syms.as_mut_ptr(), _marker: std::marker::PhantomData }
    }

    /// Symbol `id`.
    ///
    /// # Safety
    ///
    /// No other thread may access the symbol while the result lives.
    #[allow(clippy::mut_from_ref)]
    unsafe fn get(&self, id: SymbolId) -> &mut Symbol {
        // SAFETY: the caller has the symbol to itself.
        unsafe { &mut *self.ptr.add(id as usize) }
    }
}

/// Resets the resolution of every symbol a file claimed, and of every
/// common one, for a resolution round to start over.
fn clear_symbols<E: Target>(ctx: &mut Context<E>) {
    let _t = ctx.timer("clear_symbols");
    ctx.symbols.syms.par_iter_mut().for_each(|sym| {
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
/// `ranking` has them. Returns the symbols a live object has a
/// tentative definition of.
fn resolve_symbols_pass<E: Target>(
    ctx: &mut Context<E>,
    only_alive: bool,
    ranking: &DylibRanking,
) -> Tentative {
    use std::sync::atomic::Ordering;
    let _t = ctx.timer("resolve_symbols_pass");

    let refs = collect_references(ctx, only_alive);
    let commons = live_common_symbols(ctx);
    let tentative: Tentative = commons.iter().map(|c| c.0).collect();
    let best = race_definitions(ctx, only_alive, &tentative);
    claim_definitions(ctx, only_alive, &best, &tentative);
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
        claim_dylib_exports(ctx, ranking, &refs.used, &best, &tentative);
    }

    // Record the final usage set for downstream passes.
    let used = ctx.symbols.syms.par_iter_mut().zip(&refs.used);
    used.for_each(|(sym, used)| sym.set_is_used(used.load(Ordering::Relaxed)));
    tentative
}

/// The symbols of which a live object has a tentative definition.
type Tentative = hashbrown::HashSet<SymbolId>;

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
        for (nlist, &sym_id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
            if !nlist.is_stab() && nlist.is_extern() && nlist.n_type() == N_UNDF {
                refs.used[sym_id as usize].store(true, Ordering::Relaxed);
                if nlist.n_desc & N_WEAK_REF != 0 {
                    refs.weak[sym_id as usize].store(true, Ordering::Relaxed);
                } else {
                    refs.strong[sym_id as usize].store(true, Ordering::Relaxed);
                }
            }
        }
    });

    for name in command_line_symbols(&ctx.args) {
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
    // A softloaded routine is wanted like these, so that a dylib that
    // comes before any archive defining it provides it - in the round
    // over all objects as soon as bitcode might be live.
    let softload = if only_alive {
        softloads_runtime_routines(ctx)
    } else {
        may_softload_runtime_routines(ctx)
    };
    if softload {
        for id in LTO_RUNTIME_ROUTINES.iter().filter_map(|name| ctx.symbols.get(name)) {
            refs.used[id as usize].store(true, Ordering::Relaxed);
        }
    }
    refs
}

/// The rank of a definition: (class << 40) | (weak term << 32) |
/// priority, lower is better. A live weak definition's rank carries
/// the order in which ld-prime, like ld64, prefers the copies of one
/// (see weak_definition_rank); the first copy wins only among equals.
/// A lazy archive member from an archive that an auto-link option named,
/// one of `autolink_priority` or later, comes after the libraries the
/// command line's dylibs re-export (see dylib_ranks).
///
/// A lazy member's tentative definition ranks as its definitions do: a
/// reference loads the member for it like for any other. But when a
/// live file has a tentative definition of the symbol (`tentative`),
/// only a member that defines it as data - in a section not of pure
/// instructions, what ld-prime takes, like ld64, for a "data
/// definition" - overrides that: neither a member's code nor its
/// tentative definition competes, nor, but under -commons use_dylibs,
/// a dylib (see claim_dylib_exports).
fn definition_rank(
    isecs: &[InputSection],
    obj: &crate::input_files::ObjectFile,
    i: usize,
    tentative: bool,
    autolink_priority: u32,
) -> Option<u64> {
    let nlist = &obj.nlists[i];
    if nlist.is_stab() || !nlist.is_extern() {
        return None;
    }
    let is_weak = nlist.n_desc & N_WEAK_DEF != 0;
    let is_code = || {
        let hdr = (nlist.n_sect as usize).checked_sub(1).and_then(|i| obj.sect_hdrs.get(i));
        hdr.is_some_and(|hdr| hdr.flags & S_ATTR_PURE_INSTRUCTIONS != 0)
    };
    let class: u64 = match nlist.n_type() {
        N_SECT | N_ABS if obj.is_alive && !is_weak => 0,
        N_SECT | N_ABS if obj.is_alive => 1,
        N_SECT if tentative && is_code() => return None,
        N_SECT | N_ABS => 2,
        N_UNDF if nlist.is_common() && obj.is_alive => 3,
        N_UNDF if nlist.is_common() && !tentative => 2,
        _ => return None,
    };
    let mut weak_term = 0u64;
    if class == 1
        && nlist.n_type() == N_SECT
        && let Some((isec, _)) = obj.symbol_subsec(isecs, i)
    {
        weak_term = weak_definition_rank(&isecs[isec], nlist, obj.hidden);
    }
    let phase = if class == 2 && obj.priority >= autolink_priority { 2 } else { 0 };
    Some((class << 40) | ((weak_term | phase) << 32) | obj.priority as u64)
}

/// How ld-prime, like ld64, orders the copies of a weak definition,
/// lower first: a copy that can't be auto-hidden before one that can
/// (.weak_def_can_be_hidden, a global's N_WEAK_DEF | N_WEAK_REF), then
/// a global before a private extern (unless both can be hidden), then
/// the more aligned. A subsection's alignment is its section's with
/// the subsection's address as the modulus, so a copy at 8 mod 16 is
/// 8-aligned: a Swift metadata record comes at 16 from one object and
/// at 8 from another, and the first copy wins only if equally aligned.
fn weak_definition_rank(isec: &InputSection, nlist: &NList, hidden: bool) -> u64 {
    let private = nlist.n_type & N_PEXT != 0 || hidden;
    let auto_hide = !private && nlist.n_desc & N_WEAK_REF != 0;
    let p2align = isec.p2align_at(nlist.n_value) as u64;
    ((auto_hide as u64) << 7) | ((private as u64) << 6) | (63 - p2align)
}

/// The best definition rank of each symbol. Ranks race into it with an
/// atomic minimum, as in mold: the race is order-free because the
/// winner is the same whatever the interleaving, and since each object
/// has a unique priority, exactly one object ends up owning each
/// symbol.
fn race_definitions<E: Target>(
    ctx: &Context<E>,
    only_alive: bool,
    tentative: &Tentative,
) -> Vec<std::sync::atomic::AtomicU64> {
    use std::sync::atomic::{AtomicU64, Ordering};
    let best: Vec<AtomicU64> =
        (0..ctx.symbols.syms.len()).map(|_| AtomicU64::new(u64::MAX)).collect();
    ctx.objs.par_iter().filter(|obj| !only_alive || obj.is_alive).for_each(|obj| {
        for i in obj.global_range() {
            let sym_id = obj.symbols[i];
            let tentative = overrides_tentative(obj, sym_id, tentative);
            let rank = definition_rank(&ctx.isecs, obj, i, tentative, ctx.autolink_priority);
            if let Some(rank) = rank {
                best[sym_id as usize].fetch_min(rank, Ordering::Relaxed);
            }
        }
    });
    best
}

/// Whether a definition in `obj` would override a live file's
/// tentative definition (see definition_rank).
fn overrides_tentative(
    obj: &crate::input_files::ObjectFile,
    sym_id: SymbolId,
    tentative: &Tentative,
) -> bool {
    !obj.is_alive && !tentative.is_empty() && tentative.contains(&sym_id)
}

/// Each object writes the symbols whose race it won. Ranks are unique
/// per object, so every symbol has exactly one writer and the parallel
/// writes are disjoint.
fn claim_definitions<E: Target>(
    ctx: &mut Context<E>,
    only_alive: bool,
    best: &[std::sync::atomic::AtomicU64],
    tentative: &Tentative,
) {
    use std::sync::atomic::Ordering;
    let syms = SymbolSlots::new(&mut ctx.symbols.syms);
    let isecs = &ctx.isecs;
    let autolink_priority = ctx.autolink_priority;
    let objs = ctx.objs.par_iter().enumerate().filter(|(_, obj)| !only_alive || obj.is_alive);
    objs.for_each(|(obj_idx, obj)| {
        for i in obj.global_range() {
            let sym_id = obj.symbols[i];
            let tentative = overrides_tentative(obj, sym_id, tentative);
            let rank = definition_rank(isecs, obj, i, tentative, autolink_priority);
            let Some(rank) = rank else { continue };
            if best[sym_id as usize].load(Ordering::Relaxed) != rank {
                continue;
            }
            // SAFETY: this object holds the unique minimum rank for
            // sym_id, so no other thread writes this slot.
            let sym = unsafe { syms.get(sym_id) };
            if !claim_definition(sym, obj_idx, obj, i, isecs) {
                best[sym_id as usize].store(u64::MAX, Ordering::Relaxed);
            }
        }
    });
}

/// Makes `sym` what nlist `i` of object `obj_idx`, the definition that
/// won the race for it, defines. Returns false for a symbol in a section
/// that was discarded (debug info), which resolves as if undefined.
fn claim_definition(
    sym: &mut Symbol,
    obj_idx: usize,
    obj: &input_files::ObjectFile,
    i: usize,
    isecs: &[InputSection],
) -> bool {
    let nlist = &obj.nlists[i];
    sym.set_is_extern(true);
    sym.set_is_imported(false);
    sym.set_is_common(false);
    sym.set_is_weak_def(nlist.n_desc & N_WEAK_DEF != 0);
    sym.set_is_private_extern(nlist.n_type & N_PEXT != 0 || obj.hidden);
    sym.set_no_dead_strip(nlist.n_desc & (N_NO_DEAD_STRIP | REFERENCED_DYNAMICALLY) != 0);
    sym.set_is_referenced_dynamically(
        nlist.n_type() == N_SECT
            && nlist.n_desc & (REFERENCED_DYNAMICALLY | N_WEAK_DEF) == REFERENCED_DYNAMICALLY,
    );
    sym.set_is_alt_entry(nlist.n_desc & N_ALT_ENTRY != 0);

    let file = FileId::Obj(obj_idx as u32);
    match nlist.n_type() {
        N_ABS => {
            sym.set_file(file);
            sym.set_input_section(None);
            sym.value = nlist.n_value;
        }
        N_SECT => {
            let Some((isec, off)) = obj.symbol_subsec(isecs, i) else {
                sym.clear_file();
                return false;
            };
            sym.set_file(file);
            sym.set_input_section(Some(isec as u32));
            sym.value = off;
        }
        // A lazy member's tentative definition claims the symbol for the
        // member, for the liveness walk to load it.
        N_UNDF if !obj.is_alive => {
            sym.set_file(file);
            sym.set_input_section(None);
            sym.value = 0;
        }
        // A live common symbol takes a tentative claim.
        N_UNDF => {
            sym.clear_file();
            sym.set_is_common(true);
            sym.value = nlist.n_value;
            sym.common_p2align = ((nlist.n_desc >> 8) & 0xf) as u8;
        }
        _ => unreachable!(),
    }
    true
}

/// The tentative definitions of live objects, in input order: (symbol,
/// size, log2 of the alignment, whether a private external).
fn live_common_symbols<E: Target>(ctx: &Context<E>) -> Vec<(SymbolId, u64, u8, bool)> {
    ctx.objs
        .par_iter()
        .filter(|obj| obj.is_alive)
        .flat_map_iter(|obj| {
            let r = obj.global_range();
            obj.nlists[r.clone()].iter().zip(&obj.symbols[r]).filter_map(|(nlist, &sym_id)| {
                if !nlist.is_stab()
                    && nlist.is_extern()
                    && nlist.n_type() == N_UNDF
                    && nlist.is_common()
                {
                    let p2align = ((nlist.n_desc >> 8) & 0xf) as u8;
                    let pext = nlist.n_type & N_PEXT != 0 || obj.hidden;
                    Some((sym_id, nlist.n_value, p2align, pext))
                } else {
                    None
                }
            })
        })
        .collect()
}

/// Common symbols merge as ld-prime merges them, from every common
/// claim once the class-3 winners are known: the largest tentative
/// definition wins whole - its size, its alignment, whatever the
/// others', and whether it is a private external - and of those of one
/// size the first claimed (the winner's to start with).
fn merge_common_symbols<E: Target>(
    ctx: &mut Context<E>,
    commons: &[(SymbolId, u64, u8, bool)],
    best: &[std::sync::atomic::AtomicU64],
) {
    use std::sync::atomic::Ordering;
    for &(sym_id, size, p2align, pext) in commons {
        if best[sym_id as usize].load(Ordering::Relaxed) >> 40 != 3 {
            continue;
        }
        let sym = &mut ctx.symbols[sym_id];
        if size > sym.value {
            sym.value = size;
            sym.common_p2align = p2align;
            sym.set_is_private_extern(pext);
        }
    }
}

/// Dylib exports claim the referenced symbols that no object defines,
/// or that only a lazy archive member does; an earlier dylib beats a
/// later archive member and vice versa (see dylib_ranks). Of the dylibs
/// `ranking` ranks that export a symbol, the first in search order
/// claims it.
fn claim_dylib_exports<E: Target>(
    ctx: &mut Context<E>,
    ranking: &DylibRanking,
    used: &[std::sync::atomic::AtomicBool],
    best: &[std::sync::atomic::AtomicU64],
    tentative: &Tentative,
) {
    use std::sync::atomic::Ordering;
    collect_dylib_symbols(ctx);
    let dylibs = &ctx.dylibs;
    let DylibRanking { ranks, providers } = ranking;
    let order = dylib_search_order(ranks, 0);
    let first = first_exporters(dylibs, ctx.symbols.syms.len(), &order);
    // A live tentative definition (a common symbol) beats a dylib's
    // but under -commons use_dylibs, even where it is an archive
    // member's that would override it.
    let use_dylibs = ctx.args.commons == crate::cmdline::CommonsMode::UseDylibs;
    ctx.symbols.syms.par_iter_mut().enumerate().for_each(|(i, sym)| {
        let pos = first[i].load(Ordering::Relaxed);
        if pos == u32::MAX || !used[i].load(Ordering::Relaxed) {
            return;
        }
        let won = best[i].load(Ordering::Relaxed);
        let has_tentative = !tentative.is_empty() && tentative.contains(&(i as SymbolId));
        if (has_tentative && !use_dylibs) || won >> 40 < 2 {
            return;
        }
        // A DTrace symbol binds to no dylib, whatever exports one (see
        // dtrace).
        if crate::dtrace::is_dtrace_symbol(sym.name()) {
            return;
        }
        let dylib_idx = order[pos as usize];
        if ranks[dylib_idx] >= won {
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
/// that exports it, u32::MAX if none does: each dylib races the place
/// into the symbols it exports with an atomic minimum, as each of
/// mold's shared libraries resolves its own symbols by rank.
fn first_exporters(
    dylibs: &[input_files::DylibFile],
    num_syms: usize,
    order: &[usize],
) -> Vec<std::sync::atomic::AtomicU32> {
    use std::sync::atomic::{AtomicU32, Ordering};
    let first: Vec<AtomicU32> =
        (0..num_syms).into_par_iter().map(|_| AtomicU32::new(u32::MAX)).collect();
    order.par_iter().enumerate().for_each(|(pos, &dylib_idx)| {
        for &id in &dylibs[dylib_idx].symbols {
            first[id as usize].fetch_min(pos as u32, Ordering::Relaxed);
        }
    });
    first
}

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
    let exporters = first_exporters(dylibs, ctx.symbols.syms.len(), &order);
    ctx.symbols.syms.par_iter_mut().enumerate().for_each(|(i, sym)| {
        let pos = exporters[i].load(Ordering::Relaxed);
        if pos == u32::MAX || !sym.is_used() || sym.is_defined() {
            return;
        }
        import_from_dylib(sym, dylibs, &providers, order[pos as usize]);
    });
}

/// The rank with which each dylib's exports claim a symbol, comparable
/// with a lazy archive member's (see definition_rank), lower first; a
/// dylib that stands for a library exports moved to has none. ld-prime
/// looks a symbol up in the libraries the command line names, in their
/// order among the other inputs, and only then, after every archive,
/// in the public libraries they re-export: nearest first, a private
/// library in between counting as a step, and among the equally near
/// by the install name of the library that re-exports them, then by
/// their own (a breadth-first walk of each level sorted by name). Then
/// come the libraries auto-link options name, and the ones they
/// re-export likewise. So a symbol of both Foundation and CFNetwork
/// that `-framework Carbon -framework Foundation` finds binds to
/// Foundation, though Carbon re-exports CoreServices, which re-exports
/// CFNetwork.
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
/// walking owner links to a fixed point. A tentative definition loads
/// the member that overrides it, if any (see definition_rank), but only
/// one of the `tentative` symbols, which the round ranked for: one that
/// a member this walk loads brings waits for the next round. Returns
/// the symbols of such ones.
fn mark_live_objects<E: Target>(ctx: &mut Context<E>, tentative: &Tentative) -> Tentative {
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

    // Once bitcode is live, the archive members that define a runtime
    // routine LTO may call are too (see LTO_RUNTIME_ROUTINES).
    let mut softloaded = false;
    let mut new_tentative = Tentative::new();
    loop {
        while let Some(obj_idx) = queue.pop() {
            for i in 0..ctx.objs[obj_idx].nlists.len() {
                let nlist = ctx.objs[obj_idx].nlists[i];
                if nlist.is_stab() || !nlist.is_extern() || nlist.n_type() != N_UNDF {
                    continue;
                }
                let sym_id = ctx.objs[obj_idx].symbols[i];
                if nlist.is_common() && !tentative.contains(&sym_id) {
                    new_tentative.insert(sym_id);
                    continue;
                }
                load_owner(ctx, sym_id, &mut queue);
            }
        }
        if softloaded || !softloads_runtime_routines(ctx) {
            break;
        }
        softloaded = true;
        for name in LTO_RUNTIME_ROUTINES {
            if let Some(id) = ctx.symbols.get(name) {
                load_owner(ctx, id, &mut queue);
            }
        }
    }
    new_tentative
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
    objc: Option<u32>,
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
        if let Some(flags) = obj.objc_image_info {
            checked.objc = Some(check_objc_flags(ctx, checked.objc, flags, obj.mf));
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
    // version command (an old one, or one assembled for no OS) for
    // macOS, with a warning in a macOS link. The object the linker
    // synthesizes has none either.
    let Some(first) = obj.platform_versions.first() else {
        if platform == crate::macho::PLATFORM_MACOS && !ctx.is_internal(i) {
            crate::warn!(
                "no platform load command found in '{}', assuming: macOS",
                obj.mf.name.raw()
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

/// __objc_imageinfo's flag of an image whose categories may have class
/// properties: every object's record has it, or the image's has not.
const OBJC_HAS_CATEGORY_CLASS_PROPERTIES: u32 = 0x40;

/// Merges the Objective-C image info `flags` of an object into those
/// of the objects checked before it, `merged`, with ld-prime's
/// diagnostics. The first Swift ABI version stays: another one
/// fails the link (or with $LD_WARN_ON_SWIFT_ABI_VERSION_MISMATCHES
/// draws a warning). And an object that has category class properties
/// where those before don't, or lacks them where those before have
/// them, draws a warning - each one that differs from the merged flags,
/// which lose the bit at the first.
fn check_objc_flags<E: Target>(
    ctx: &Context<E>,
    merged: Option<u32>,
    flags: u32,
    mf: &MappedFile,
) -> u32 {
    let Some(merged) = merged else { return flags };
    let (first, abi) = ((merged >> 8) & 0xff, (flags >> 8) & 0xff);
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
    let cat = flags & OBJC_HAS_CATEGORY_CLASS_PROPERTIES;
    if cat != merged & OBJC_HAS_CATEGORY_CLASS_PROPERTIES {
        crate::warn!(
            "mixed ObjC ABI, {} compiled {} category class properties",
            mf.name.raw(),
            if cat != 0 { "with" } else { "without" }
        );
    }
    merge_objc_flags(merged, flags)
}

/// The Objective-C image info flags of objects whose flags so far are
/// `merged`, and of an object with `flags`, as ld-prime merges them:
/// the first Swift ABI version given stays, the Swift language version
/// is the oldest given, and the image's categories may have class
/// properties if every object's may.
pub(crate) fn merge_objc_flags(merged: u32, flags: u32) -> u32 {
    let abi = if merged & 0xff00 != 0 { merged & 0xff00 } else { flags & 0xff00 };
    let lang = match (merged >> 16, flags >> 16) {
        (0, lang) | (lang, 0) => lang,
        (a, b) => a.min(b),
    };
    (lang << 16) | abi | (merged & flags & OBJC_HAS_CATEGORY_CLASS_PROPERTIES)
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
    let mut modules = live_bitcode_modules(ctx).peekable();
    modules.peek().is_some()
        && !ctx.args.lto_codegen_only
        && modules.all(|module| !module.is_thin)
        && ctx
            .objs
            .iter()
            .enumerate()
            .all(|(i, obj)| !obj.is_alive || obj.lto_module.is_some() || ctx.is_internal(i))
}

/// The bitcode modules of live files, in input order.
fn live_bitcode_modules<E: Target>(
    ctx: &Context<E>,
) -> impl Iterator<Item = &crate::lto::BitcodeModule> {
    ctx.lto_modules.iter().filter(|module| ctx.objs[module.obj].is_alive)
}

/// Writes a -r link of bitcode alone as one merged bitcode file (see
/// links_only_bitcode). ld-prime warns, then fails, if libLTO can't.
pub fn write_merged_bitcode<E: Target>(ctx: &Context<E>) {
    let plugin = ctx.lto_plugin.unwrap();
    let modules: Vec<_> = live_bitcode_modules(ctx).collect();
    let roots = lto_roots(ctx);
    // SAFETY: libLTO calls with handles created by the same library.
    unsafe {
        let cg = create_lto_codegen(ctx, &plugin, &modules, &roots);
        if let Err(msg) = crate::lto::write_merged_modules(&plugin, cg, &ctx.args.output) {
            crate::warn!("could not produce merged bitcode file");
            fatal!("LTO codegen error: {msg}");
        }
    }
}

/// Creates libLTO's code generator for the modules to merge, added in
/// input order, with the symbols that must survive LTO.
///
/// # Safety
///
/// The plugin must be the library the modules were created by.
unsafe fn create_lto_codegen<E: Target>(
    ctx: &Context<E>,
    plugin: &crate::lto::Plugin,
    modules: &[&crate::lto::BitcodeModule],
    roots: &[&[u8]],
) -> *mut std::ffi::c_void {
    // SAFETY: libLTO calls with handles created by the same library.
    unsafe {
        let cg = (plugin.codegen_create)();
        if cg.is_null() {
            fatal!("lto_codegen_create failed: {}", plugin.error_message());
        }
        (plugin.codegen_set_pic_model)(cg, crate::lto::LTO_CODEGEN_PIC_MODEL_DYNAMIC);
        crate::lto::set_debug_options(plugin, cg, &ctx.args.mllvm);
        for module in modules {
            if (plugin.codegen_add_module)(cg, module.handle as *mut _) {
                fatal!("lto_codegen_add_module failed: {}", plugin.error_message());
            }
        }
        for name in roots {
            if let Ok(name) = std::ffi::CString::new(*name) {
                (plugin.codegen_add_must_preserve_symbol)(cg, name.as_ptr());
            }
        }
        cg
    }
}

/// The symbols of the bitcode modules that must survive the LTO
/// internalizer, as ld-prime picks them - the same set for ThinLTO and
/// the merged module: the definitions the output exports (see
/// exported_before_lto), the entry point, -u symbols, -alias bases
/// (which the linker itself references), and those some code outside
/// the module defining them references: live Mach-O code (see
/// native_refs_before_lto), or a bitcode module that libLTO compiles
/// apart from it. A reference between two modules it
/// merges does not count, as libLTO resolves it itself (_times2,
/// called only from a bitcode main, goes local and is not exported),
/// but a ThinLTO module is compiled on its own, so a reference to or
/// from one does. A native common counts: when a bitcode definition
/// wins, the common's code addresses that definition's storage.
fn lto_roots<E: Target>(ctx: &Context<E>) -> Vec<&[u8]> {
    use std::sync::atomic::{AtomicU8, Ordering};

    // Who refers to each symbol: a ThinLTO module or a module to merge
    // (one whose copy of a weak definition another's replaced counts,
    // as mold's LTO plugin calls such a copy preempted: its code has to
    // reach the copy that won, or a C++ inline function's static local
    // would split in two); whether a Mach-O object defines it; and
    // whether it has a weak definition and one that can't be hidden.
    const THIN_REF: u8 = 1;
    const MERGED_REF: u8 = 2;
    const NATIVE_DEF: u8 = 4;
    const WEAK: u8 = 8;
    const NOT_HIDABLE: u8 = 16;
    let mut thin = vec![None; ctx.objs.len()];
    for module in &ctx.lto_modules {
        thin[module.obj] = Some(module.is_thin);
    }
    let flags: Vec<AtomicU8> = (0..ctx.symbols.syms.len()).map(|_| AtomicU8::new(0)).collect();
    ctx.objs.par_iter().enumerate().filter(|(_, obj)| obj.is_alive).for_each(|(i, obj)| {
        let r = obj.global_range();
        for (nlist, &sym_id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
            if nlist.is_stab() || !nlist.is_extern() {
                continue;
            }
            let defined = matches!(nlist.n_type(), N_SECT | N_ABS);
            let lost = || ctx.symbols[sym_id].file() != Some(FileId::Obj(i as u32));
            let mut flag = match (nlist.n_type(), thin[i]) {
                (N_UNDF, Some(true)) => THIN_REF,
                (N_UNDF, Some(false)) => MERGED_REF,
                (N_SECT | N_ABS, None) => NATIVE_DEF,
                (N_ABS, Some(true)) if lost() => THIN_REF,
                (N_ABS, Some(false)) if lost() => MERGED_REF,
                _ => 0,
            };
            if defined && nlist.n_desc & N_WEAK_DEF != 0 {
                flag |= if nlist.n_desc & N_WEAK_REF != 0 { WEAK } else { WEAK | NOT_HIDABLE };
            } else if defined {
                flag |= NOT_HIDABLE;
            }
            flags[sym_id as usize].fetch_or(flag, Ordering::Relaxed);
        }
    });
    let exported = |id: SymbolId| {
        let f = flags[id as usize].load(Ordering::Relaxed);
        let hidable = f & (WEAK | NOT_HIDABLE) == WEAK;
        crate::dead_strip::exported_before_lto(ctx, &ctx.symbols[id], hidable)
    };
    let native_refs = crate::dead_strip::native_refs_before_lto(ctx, exported);

    let mut roots = Vec::new();
    for (i, sym) in ctx.symbols.syms.iter().enumerate() {
        let Some(FileId::Obj(obj)) = sym.file() else { continue };
        let Some(is_thin) = thin[obj as usize] else { continue };
        if !ctx.objs[obj as usize].is_alive || !sym.is_extern() {
            continue;
        }
        let outside = THIN_REF | if is_thin { MERGED_REF } else { 0 };
        let name = sym.name();
        if native_refs[i].load(Ordering::Relaxed)
            || flags[i].load(Ordering::Relaxed) & outside != 0
            || exported(i as SymbolId)
            || command_line_symbols(&ctx.args).any(|named| named == name)
        {
            roots.push(name);
        }
    }

    // A bitcode definition a native object has one of too survives, as
    // ld64 keeps the LLVM definitions it coalesced away in favor of
    // Mach-O ones: left to libLTO, a weak one would be inlined into the
    // module's callers in place of the strong native definition that
    // wins, and a strong one would vanish rather than be reported as a
    // duplicate (ld-prime lists it in the compiled object).
    for module in live_bitcode_modules(ctx) {
        let obj = &ctx.objs[module.obj];
        for (nlist, &id) in obj.nlists.iter().zip(&obj.symbols) {
            if nlist.n_type() == N_ABS
                && flags[id as usize].load(Ordering::Relaxed) & NATIVE_DEF != 0
            {
                roots.push(ctx.symbols[id].name());
            }
        }
    }
    roots
}

/// An object LTO compiled, under the name ld-prime gives it (see
/// thin_lto and merged_lto) and the modification time its debug stab
/// gets if not the named file's.
struct LtoObject {
    name: PathBuf,
    mtime: Option<u64>,
    data: Vec<u8>,
}

/// Whether a live file is bitcode, for LTO to compile.
pub fn has_lto_obj<E: Target>(ctx: &Context<E>) -> bool {
    live_bitcode_modules(ctx).next().is_some()
}

/// Compiles the live bitcode modules to Mach-O objects as ld-prime
/// does - first the modules built for ThinLTO, an object each, then the
/// rest merged into one - and resolves symbols again with the compiled
/// objects in place of the bitcode files. Both compilations see the
/// same symbols to preserve. -flto-codegen-only has ThinLTO compile
/// every module, unoptimized.
pub fn do_lto<E: Target>(ctx: &mut Context<E>) {
    let plugin = ctx.lto_plugin.unwrap();

    let mut objects = Vec::new();
    {
        let roots = lto_roots(ctx);
        let (thin, merged): (Vec<_>, Vec<_>) = live_bitcode_modules(ctx)
            .partition(|module| module.is_thin || ctx.args.lto_codegen_only);
        if !thin.is_empty() {
            objects.extend(thin_lto(ctx, &plugin, &thin, &roots));
        }
        if !merged.is_empty() {
            objects.push(merged_lto(ctx, &plugin, &merged, &roots));
        }
    }
    retire_bitcode_placeholders(ctx);

    let first = ctx.objs.len();
    for LtoObject { name, mtime, data } in objects {
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
    keep_bitcode_imports(ctx);
}

/// Compiles the ThinLTO modules to an object each. libLTO tells the
/// modules apart by name: ld-prime gives each its file's real path -
/// an archive member's as archive[index](member) - followed by its
/// index among them. It names the objects after the files libLTO wrote
/// to the -object_path_lto directory, or else not at all: an empty
/// name in diagnostics, the map and the debug stab (whose modification
/// time is then 0).
fn thin_lto<E: Target>(
    ctx: &Context<E>,
    plugin: &crate::lto::Plugin,
    modules: &[&crate::lto::BitcodeModule],
    roots: &[&[u8]],
) -> Vec<LtoObject> {
    let thin_modules: Vec<crate::lto::ThinModule> = modules
        .iter()
        .enumerate()
        .map(|(i, module)| {
            let mf = ctx.objs[module.obj].mf;
            let mut id = path_bytes(&mf.name).to_vec();
            id.extend_from_slice(i.to_string().as_bytes());
            let id = std::ffi::CString::new(id).unwrap_or_default();
            crate::lto::ThinModule { id, data: mf.data() }
        })
        .collect();

    // What the modules refer to, defined anywhere, ThinLTO keeps too.
    let mut cross = Vec::new();
    for module in modules {
        let obj = &ctx.objs[module.obj];
        for (nlist, &id) in obj.nlists.iter().zip(&obj.symbols) {
            if nlist.n_type() == N_UNDF {
                cross.push(ctx.symbols[id].name());
            }
        }
    }

    let opts = crate::lto::ThinOptions {
        debug_options: &ctx.args.mllvm,
        cpu: ctx.args.lto_cpu.as_deref(),
        objects_dir: ctx.args.object_path_lto.as_deref(),
        cache: ctx.args.lto_cache_dir.as_deref().map(|dir| crate::lto::CacheOptions {
            dir,
            prune_interval: ctx.args.lto_cache_prune_interval,
            expiration: ctx.args.lto_cache_expiration,
            max_size: ctx.args.lto_cache_max_size,
        }),
        save_temps: ctx.args.save_temps.then_some(ctx.args.output.as_path()),
        codegen_only: ctx.args.lto_codegen_only,
    };
    // SAFETY: the plugin is the library that parsed the modules.
    let objects = unsafe { crate::lto::compile_thin(plugin, &thin_modules, roots, &cross, &opts) };
    objects
        .into_iter()
        .map(|obj| match obj.path {
            Some(name) => LtoObject { name, mtime: None, data: obj.data },
            None => LtoObject { name: PathBuf::new(), mtime: Some(0), data: obj.data },
        })
        .collect()
}

/// Merges the other modules into one and compiles it to one object.
/// -object_path_lto keeps that object: debug info stays in object files
/// on Mach-O (the executable only gets stabs pointing at them), and for
/// LTO code the object exists only inside the linker - Xcode passes a
/// path under the dSYM staging directory so dsymutil can find it
/// afterwards. ld-prime names the object after that file, or else after
/// a temporary file it never writes, in its diagnostics, the map and
/// the debug stabs - which give the latter modification time 0.
fn merged_lto<E: Target>(
    ctx: &Context<E>,
    plugin: &crate::lto::Plugin,
    modules: &[&crate::lto::BitcodeModule],
    roots: &[&[u8]],
) -> LtoObject {
    // SAFETY: libLTO calls with handles created by the same library.
    let data = unsafe {
        let cg = create_lto_codegen(ctx, plugin, modules, roots);
        let opts = crate::lto::CodegenOptions {
            cpu: ctx.args.lto_cpu.as_deref(),
            save_temps: ctx.args.save_temps.then_some(ctx.args.output.as_path()),
        };
        crate::lto::compile(plugin, cg, &opts)
    };
    match &ctx.args.object_path_lto {
        Some(path) => {
            let path = lto_object_path(path);
            // ld-prime keeps the object if it can, saying nothing
            // otherwise.
            let _ = std::fs::write(&path, &data);
            LtoObject { name: path, mtime: None, data }
        }
        None => LtoObject { name: PathBuf::from("/tmp/lto.o"), mtime: Some(0), data },
    }
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
            let imports = obj
                .nlists
                .iter()
                .zip(&obj.symbols)
                .filter(|(nlist, _)| nlist.n_type() == N_UNDF)
                .filter_map(|(_, &id)| match ctx.symbols[id].file() {
                    Some(FileId::Dylib(dylib)) => Some((id, dylib)),
                    _ => None,
                })
                .collect();
            let defined = module.defined;
            let input = crate::lto::LtoInput { obj: obj_idx, defined, won, imports };
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
        obj.nlists = std::borrow::Cow::Borrowed(&[]);
        obj.symbols.clear();
    }
}

/// Keeps the imports bitcode referred to that the code LTO compiled
/// doesn't: ld-prime resolved them before LTO, and keeps their dylibs
/// - unless -dead_strip drops what no live code refers to.
pub fn keep_bitcode_imports<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.dead_strip {
        return;
    }
    for input in &ctx.lto_inputs {
        for &(id, dylib) in &input.imports {
            let sym = &mut ctx.symbols[id];
            if sym.file().is_some() {
                continue;
            }
            sym.set_file(FileId::Dylib(dylib));
            sym.set_is_imported(true);
            sym.set_is_extern(true);
            if ctx.dylibs[dylib as usize].is_weak {
                sym.set_is_weak_ref(true);
            }
        }
    }
}

/// Where -object_path_lto has the merged LTO object written: the path
/// itself, or lto.o in it if it names a directory - as it does when
/// ThinLTO objects share it (ld-prime appends "/lto.o" to the path as
/// given, trailing slash or not).
fn lto_object_path(path: &Path) -> PathBuf {
    if !path.is_dir() {
        return path.to_path_buf();
    }
    let mut path = path.as_os_str().to_owned();
    path.push("/lto.o");
    PathBuf::from(path)
}

/// Hides the subsections of archive members that resolution left
/// dead, so nothing of theirs reaches the output.
pub fn remove_unreachable_files<E: Target>(ctx: &mut Context<E>) {
    for isec in ctx.isecs.iter_mut() {
        if !ctx.objs[isec.file as usize].is_alive {
            isec.set_alive(false);
        }
    }

    // Unwind records and FDEs of dead files go too, remapping the
    // record-to-FDE links around the removals.
    let mut fde_map = vec![usize::MAX; ctx.fdes.len()];
    let mut kept_fdes = Vec::new();
    let fdes = std::mem::take(&mut ctx.fdes);
    for (i, fde) in fdes.into_iter().enumerate() {
        if ctx.isecs[fde.isec].is_alive() {
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
            rec.fde_idx = map[rec.fde_idx as usize] as u32;
        }
        true
    });
    refresh_unwind_ranges(ctx);
}

/// Rebuilds each subsection's compact-unwind record range after the
/// records vector was compacted; the records stay grouped by
/// subsection, so one walk over runs restores every range.
pub fn refresh_unwind_ranges<E: Target>(ctx: &mut Context<E>) {
    let mut i = 0;
    while i < ctx.unwind_records.len() {
        let isec = ctx.unwind_records[i].isec;
        let start = i;
        while i < ctx.unwind_records.len() && ctx.unwind_records[i].isec == isec {
            i += 1;
        }
        ctx.isecs[isec as usize].unwind_offset = start as u32;
        ctx.isecs[isec as usize].nunwind = (i - start) as u32;
    }
}

/// Whether -remove_swift_reflection_metadata_sections drops an input
/// section: Swift's field descriptors, associated type records and the
/// names they give (but not the type references), in any segment.
pub(crate) fn is_swift_reflection_section(hdr: &MachSection) -> bool {
    matches!(hdr.sectname(), b"__swift5_fieldmd" | b"__swift5_assocty" | b"__swift5_reflstr")
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
        .filter(|&i| is_swift_reflection_section(ctx.hdr_of(&ctx.isecs[i])))
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
        !isec.is_alive() && is_swift_reflection_section(ctx.hdr_of(isec))
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
        ctx.isec_relocs(i).iter().filter(|r| r.r_type != E::RELOC_SUBTRACTOR).copied().collect();
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
        for (nlist, &id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
            let sym = &ctx.symbols[id];
            if nlist.is_common() && sym.is_common() && !sym.is_defined() && seen.insert(id) {
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
            if !is_mergeable_literal(hdr, isec) {
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
    redirect_symbols_to_replacements(ctx);
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

/// Whether ld-prime merges a literal element with identical ones: a C
/// string of a section of any name, but a fixed-size record only of the
/// standard pool of its size, __TEXT,__literal4, __literal8 or
/// __literal16 of that type - its records in a section of another name
/// or type stay, however many copies there are. Nor does an element
/// that carries a relocation merge, as identical bytes may point at
/// different targets (ld-prime merges a __literal8 record by its bytes,
/// making every copy point where the first does).
///
/// __TEXT,__ustring, which holds the UTF-16 strings of CFString
/// constants (and C's u"" literals), is a regular section that ld-prime
/// cuts at its symbols, like ld64, but merges each subsection with
/// identical ones whatever labels it: every object that spells @"é" has
/// its own copy, and so its own CFString, which merges only once the
/// strings have (iTerm2's debug dylib had 67 CFStrings too many).
fn is_mergeable_literal(hdr: &MachSection, isec: &InputSection) -> bool {
    if isec.nrels != 0 {
        return false;
    }
    match hdr.section_type() {
        S_CSTRING_LITERALS => true,
        S_4BYTE_LITERALS => hdr.segname_is(b"__TEXT") && hdr.sectname_is(b"__literal4"),
        S_8BYTE_LITERALS => hdr.segname_is(b"__TEXT") && hdr.sectname_is(b"__literal8"),
        S_16BYTE_LITERALS => hdr.segname_is(b"__TEXT") && hdr.sectname_is(b"__literal16"),
        S_REGULAR => hdr.segname_is(b"__TEXT") && hdr.sectname_is(b"__ustring"),
        _ => false,
    }
}

/// Points every symbol defined in a merged-away subsection at the
/// surviving one - mold makes the merged section's fragment the
/// symbol's origin - so a symbol's address never follows a replacement
/// chain. The copies are identical, so the symbol's offset is
/// unchanged. (Section-relative relocations still resolve through the
/// chain in isec_addr.)
pub(crate) fn redirect_symbols_to_replacements<E: Target>(ctx: &mut Context<E>) {
    let isecs = &ctx.isecs;
    ctx.symbols.syms.par_iter_mut().for_each(|sym| {
        if let Some(i) = sym.input_section() {
            let mut r = i as usize;
            while isecs[r].replacement != NO_REPLACEMENT {
                r = isecs[r].replacement as usize;
            }
            if r != i as usize {
                sym.set_input_section(Some(r as u32));
            }
        }
    });
}

/// Auto-hides eligible weak definitions. Compilers mark a weak
/// definition whose address is never observed with
/// .weak_def_can_be_hidden (nlist n_desc carries N_WEAK_DEF and
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
    // may be hidden". A C++ debug link has millions of weak-def nlists
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
        for (nlist, &sym_id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
            if !is_weak_def(nlist) {
                continue;
            }
            let bits = if nlist.n_desc & N_WEAK_REF != 0 { SEEN } else { SEEN | NOT_HIDABLE };
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

/// Whether an nlist is an external weak definition in a section.
fn is_weak_def(nlist: &NList) -> bool {
    !nlist.is_stab()
        && nlist.is_extern()
        && nlist.n_type() == N_SECT
        && nlist.n_desc & N_WEAK_DEF != 0
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
    // A C++ debug link has millions of weak-def nlists (every inline
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
/// weak definition, each with the winning copy's subsection, in nlist
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
        let (nlist, sym_id) = (&obj.nlists[i], obj.symbols[i]);
        if !is_weak_def(nlist) {
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
            let mut v: Vec<u64> = (obj.nlists.iter())
                .filter(|n| !n.is_stab() && n.n_type() == N_SECT)
                .map(|n| n.n_value)
                .collect();
            v.sort_unstable();
            v.dedup();
            v
        });
        let l = &ctx.isecs[loser];
        let (start, end) = (l.input_addr as u64, l.input_addr as u64 + l.size as u64);
        let lo = values.partition_point(|&v| v < start);
        let hi = values.partition_point(|&v| v < end);
        if values[lo..hi].iter().all(|&v| v == nlist.n_value) {
            out.push((loser, winner as usize));
        }
    }
    out
}

/// Reports the symbols live objects define strongly more than once, as
/// ld-prime does once resolution settles (and, in a final link, no
/// symbol is undefined): each with the files that define it, then their
/// number. Resolution keeps the first strong definition it meets; a weak
/// or common one yields quietly. Under -dead_strip only a symbol whose
/// kept definition is live is reported, and -allow_dead_duplicates lets
/// one stay whose other definitions are all dead; ld-prime counts the
/// symbols it doesn't report in the number all the same. It lists them
/// in no stable order; mold sorts them by name.
pub fn check_duplicate_symbols<E: Target>(ctx: &Context<E>) {
    report_duplicates(ctx, duplicate_symbols(ctx, false));
}

/// Reports, before LTO, the symbols two bitcode files define strongly,
/// whose modules libLTO could not merge. A duplicate between bitcode and
/// a Mach-O object is left to the check after LTO, which finds it in a
/// compiled object (see lto_roots).
pub fn check_bitcode_duplicates<E: Target>(ctx: &Context<E>) {
    if ctx.lto_modules.len() > 1 {
        report_duplicates(ctx, duplicate_symbols(ctx, true));
    }
}

/// A symbol defined strongly more than once: the files that do, and
/// whether any of those definitions is live and the one that won is.
struct Duplicate {
    sym: SymbolId,
    files: Vec<usize>,
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
                let (nlist, sym_id) = (&obj.nlists[i], obj.symbols[i]);
                if nlist.is_stab()
                    || !nlist.is_extern()
                    || !matches!(nlist.n_type(), N_SECT | N_ABS)
                    || nlist.n_desc & N_WEAK_DEF != 0
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
        let mut files: Vec<usize> = group.iter().map(|&(_, obj, _)| obj).collect();
        files.push(winner as usize);
        if among_bitcode && files.iter().filter(|&&obj| is_bitcode(obj)).count() < 2 {
            continue;
        }
        files.sort_by_key(|&obj| ctx.objs[obj].priority);
        dups.push(Duplicate {
            sym: id,
            files,
            any_live: group.iter().any(|&(_, _, live)| live),
            winner_live: sym.input_section().is_none_or(|isec| ctx.isecs[isec as usize].is_alive()),
        });
    }
    dups
}

/// Reports duplicate symbols, failing the link if one is.
fn report_duplicates<E: Target>(ctx: &Context<E>, dups: Vec<Duplicate>) {
    let mut count = 0;
    let mut reported = false;
    for dup in dups {
        if ctx.args.allow_dead_duplicates && !dup.any_live {
            continue;
        }
        count += 1;
        if !dup.winner_live {
            continue;
        }
        reported = true;
        let sym = &ctx.symbols[dup.sym];
        crate::error::notice(format_args!("duplicate symbol '{sym}' in:"));
        for obj in dup.files {
            crate::error::notice(format_args!("    {}", ctx.objs[obj].mf.name.raw()));
        }
    }
    if reported {
        error!("{count} duplicate symbols");
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
                .filter(|rel| rel.r_type != E::RELOC_SUBTRACTOR)
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
                && (obj.nlists.iter().zip(&obj.symbols)).any(|(n, &s)| s == id && n.is_common())
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
                out.push((sym_id, obj.nlists[idx as usize].n_desc & N_WEAK_REF != 0));
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

/// Reports references to symbols that are still unresolved; with
/// `-undefined dynamic_lookup` (or -U naming one) they become
/// flat-namespace imports that dyld resolves against any loaded image
/// at run time.
pub fn report_undef_errors<E: Target>(ctx: &mut Context<E>) {
    use std::sync::atomic::Ordering;
    // An alive object may name a symbol undefined that nothing refers
    // to - a .globl with neither a definition nor a relocation, as XNU
    // declares `SleepToken` under !WITH_CLASSIC_S2R. ld-prime drops
    // such a name without a word: no error, and no import under
    // -undefined dynamic_lookup. Most links have no undefined symbol at
    // all, so the relocations are looked at only when there is one.
    // A DTrace symbol is never defined (see dtrace).
    let undef: Vec<SymbolId> = (0..ctx.symbols.syms.len() as SymbolId)
        .into_par_iter()
        .filter(|&id| {
            let sym = &ctx.symbols[id];
            sym.is_used() && !sym.is_defined() && !crate::dtrace::is_dtrace_symbol(sym.name())
        })
        .collect();
    if undef.is_empty() {
        return;
    }
    let referenced = referenced_symbols(ctx);
    // A name the command line insists on must resolve, even under
    // -undefined dynamic_lookup or -U: -u, the entry point, a name an
    // export list gives without wildcards, an -alias base. The alias
    // itself counts as defined.
    let initial: hashbrown::HashSet<SymbolId> = crate::dead_strip::initial_undefines(ctx).collect();
    let aliases: hashbrown::HashSet<SymbolId> =
        ctx.args.aliases.iter().filter_map(|(_, alias)| ctx.symbols.get(alias)).collect();

    // A -static image has no dyld to look a symbol up at run time, so
    // ld-prime lets none stay undefined, whatever -undefined or -U say.
    let args = &ctx.args;
    let may_look_up = |id: SymbolId| {
        !args.static_link
            && (args.undefined_dynamic_lookup
                || args.allowed_undefined.iter().any(|n| n.as_slice() == ctx.symbols[id].name()))
            && !initial.contains(&id)
    };
    let (imports, mut errors): (Vec<SymbolId>, Vec<SymbolId>) = (undef.into_iter())
        .filter(|&id| referenced[id as usize].load(Ordering::Relaxed) && !aliases.contains(&id))
        .partition(|&id| may_look_up(id));
    for id in imports {
        let sym = &mut ctx.symbols[id];
        sym.set_file(FileId::Dylib(u32::MAX));
        sym.set_is_imported(true);
        sym.set_is_extern(true);
    }
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
        for (nlist, &sym_id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
            if !nlist.is_stab() && nlist.n_type() == N_UNDF && !nlist.is_common() {
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
        for (nlist, &sym_id) in obj.nlists[r.clone()].iter().zip(&obj.symbols[r]) {
            if nlist.is_stab() || nlist.n_type() != N_UNDF || nlist.is_common() {
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
                let strong = obj.nlists[idx as usize].n_desc & N_WEAK_REF == 0;
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
        .filter(|d| !crate::cmdline::in_shared_cache_path(&d.install_name))
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
/// templates link by habit, are let off. ld-prime warns once the link
/// has turned out to be sound, before it warns of redundant re-exports
/// and weak exports.
pub fn warn_unused_dylibs<E: Target>(ctx: &Context<E>) {
    if !ctx.args.warn_unused_dylibs {
        return;
    }
    const EXEMPT: [&[u8]; 3] = [
        b"/usr/lib/libSystem.B.dylib",
        b"/usr/lib/libc++.1.dylib",
        b"/System/Library/Frameworks/Foundation.framework/Versions/C/Foundation",
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
            && !EXEMPT.contains(&dylib.install_name.as_slice())
        {
            crate::warn!(
                "linking with ({}) but not using any symbols from it",
                raw(&dylib.install_name)
            );
        }
    }
}

/// True if a dylib's exports bound here moved to older libraries
/// ($ld$previous): ld-prime lists a library under the install names
/// bound to it, so one all of whose bound exports moved loses its load
/// command, named or not; libc++ does to libc++abi for macOS 13 if only
/// char8_t's type_info binds. (It drops a -needed_* or -reexport_*
/// library alike, not what the option asks for; those stay.)
fn exports_moved_away<E: Target>(ctx: &Context<E>, dylib: &input_files::DylibFile) -> bool {
    dylib.moved_exports.iter().any(|(&name, &target)| {
        let file = ctx.symbols.get(name).and_then(|id| ctx.symbols[id].file());
        file == Some(FileId::Dylib(target as u32))
    })
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
/// that order. A lazy dylib has none: its imports' n_desc names the
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
            || !strippable(dylib) && (dylib.is_reexported || !exports_moved_away(ctx, dylib));
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
            if E::classify_reloc(r.r_type) != RelocClass::Branch
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
/// refer to them. Only the relocations of subsections the output keeps
/// count: not those of a copy merged into another, such as a losing
/// weak definition. Swift's symbolic type references are weak, and the
/// copy in the object defining the type refers to its descriptor
/// directly while every other object's goes through a GOT slot.
pub fn scan_relocations<E: Target>(ctx: &mut Context<E>) {
    // Classification reads only; collect it on all cores. The apply
    // loop below stays serial so GOT and stub slots keep their
    // deterministic first-seen order.
    let ctx_ref: &Context<E> = ctx;
    let classes: Vec<(SymbolId, RelocClass)> = ctx_ref
        .isecs
        .par_iter()
        .filter(|isec| isec.is_emitted())
        .flat_map_iter(|isec| {
            input_files::isec_relocs_of(&ctx_ref.objs, isec).iter().filter_map(move |rel| {
                let id = ctx_ref.reloc_target_sym(isec.file as usize, rel)?;
                let mut class = E::classify_reloc(rel.r_type);
                // A one-byte branch (x86-64's jmp rel8) reaches only
                // code near it, so it takes no stub: one to an import
                // is a fixup error, as in ld-prime.
                if class == RelocClass::Branch && rel.size == 1 {
                    class = RelocClass::Plain;
                }
                // Plain references need no slot of any kind, and
                // they are the overwhelming majority; dropping them
                // here keeps the collected list (and the serial
                // apply loop below) small. The TLV/regular mismatch
                // check needs the TLV side only: a plain reference
                // to a thread-local is caught because thread-locals
                // are reached exclusively through TLV relocations,
                // checked against the symbol below either way.
                if class == RelocClass::Plain && !is_thread_local_sym(ctx_ref, id) {
                    return None;
                }
                Some((id, class))
            })
        })
        .collect();

    // A lazy dylib's symbols take no stub or GOT slot; the image
    // reaches them through the helpers of lazy_load::create_lazy_loads.
    // Calls of a delay-init dylib's go to the stubs of
    // delay_init::create_delay_init.
    let has_lazy = ctx.dylibs.iter().any(|d| d.is_lazy);
    let has_delay = ctx.dylibs.iter().any(|d| d.delay_init.is_some());
    for (id, class) in classes {
        if has_lazy && ctx.is_lazy_import(id) {
            continue;
        }
        if has_delay && class == RelocClass::Branch && ctx.is_delay_import(id) {
            continue;
        }
        let sym = &ctx.symbols[id];

        // Thread-locals live behind __thread_vars descriptors, so the
        // reference kind must agree with the symbol: a TLV load of
        // ordinary data would treat the variable's bytes as a
        // descriptor, and an ordinary load of a TLV would read the
        // descriptor as data. ld64 rejects both directions.
        if is_thread_local_sym(ctx, id) != matches!(class, RelocClass::Tlv) {
            fatal!("illegal thread local variable reference to regular symbol `{sym}`");
        }

        match class {
            RelocClass::Branch => add_branch_target(ctx, id),
            RelocClass::Got => add_got(ctx, id),
            // A GOT load of a local symbol needs no slot at all: it
            // relaxes, or ld-prime refuses the instruction.
            RelocClass::GotLoad if !ctx.can_relax_got(id) => add_got(ctx, id),
            // A TLV load relaxes to the descriptor's address like a GOT
            // load; one dyld must fill - an imported thread-local, or a
            // weak one coalesced across images (C++'s inline
            // thread_local) - goes through an ordinary __got entry, as
            // in ld-prime (no __thread_ptrs section, chained or classic).
            RelocClass::Tlv if !ctx.can_relax_got(id) => add_got(ctx, id),
            _ => {}
        }
    }
}

/// Gives a symbol a call reaches what the call goes through.
pub(crate) fn add_branch_target<E: Target>(ctx: &mut Context<E>, id: SymbolId) {
    // A call to a symbol dyld resolves by weak lookup - one of this
    // image's own coalescable weak definitions, or a dylib's weak
    // export - goes through a stub and a GOT slot, never a lazy
    // pointer, as ld64 does.
    if ctx.binds_weak_lookup(id) {
        add_stub(ctx, id);
        add_got(ctx, id);
        return;
    }
    // An x86-64 kext calls an import directly, unless -kexts_use_stubs:
    // kmutil fills in the call by an external relocation, or the stub's
    // GOT slot.
    if ctx.args.is_kext() && E::CPUTYPE == CPU_TYPE_X86_64 && !ctx.args.kexts_use_stubs {
        return;
    }
    if ctx.binds_as_import(id) {
        add_import_stub(ctx, id);
    }
}

/// True if the symbol resolves to a TLV descriptor: a definition in a
/// S_THREAD_LOCAL_VARIABLES section, or a dylib export listed as
/// thread-local. Symbols left to runtime lookup pass as either.
pub fn is_thread_local_sym<E: Target>(ctx: &Context<E>, id: SymbolId) -> bool {
    let sym = &ctx.symbols[id];
    match sym.file() {
        Some(FileId::Obj(_)) => sym.input_section().map(|i| i as usize).is_some_and(|isec| {
            ctx.hdr_of(&ctx.isecs[isec]).flags & SECTION_TYPE == S_THREAD_LOCAL_VARIABLES
        }),
        Some(FileId::Dylib(idx)) => {
            idx != u32::MAX && ctx.dylibs[idx as usize].tlv_exports.contains(sym.name())
        }
        _ => false,
    }
}

/// Personality functions are referenced from __unwind_info, and from
/// __eh_frame's CIEs, through the GOT.
pub fn scan_unwind_personalities<E: Target>(ctx: &mut Context<E>) {
    let mut personalities: Vec<_> =
        ctx.unwind_records.iter().filter_map(|rec| rec.personality()).collect();
    personalities.extend(ctx.fdes.iter().filter_map(|fde| ctx.cies[fde.cie as usize].personality));
    for id in personalities {
        add_got(ctx, id);
    }
}

/// Gives an import a stub, which jumps through the import's lazy
/// pointer, or, without lazy binding, its GOT slot.
pub(crate) fn add_import_stub<E: Target>(ctx: &mut Context<E>, id: SymbolId) {
    add_stub(ctx, id);
    if ctx.args.lazy_binding {
        ensure_stub_binder(ctx);
    } else {
        add_got(ctx, id);
    }
}

pub(crate) fn add_stub<E: Target>(ctx: &mut Context<E>, id: SymbolId) {
    if ctx.sym_aux(id).stub_idx == NO_IDX {
        ctx.sym_aux_mut(id).stub_idx = ctx.stubs.symbols.len() as u32;
        ctx.stubs.symbols.push(id);
    }
}

pub(crate) fn add_got<E: Target>(ctx: &mut Context<E>, id: SymbolId) {
    if ctx.sym_aux(id).got_idx == NO_IDX {
        ctx.sym_aux_mut(id).got_idx = ctx.got.got_syms.len() as u32;
        ctx.got.got_syms.push(id);
    }
}

/// Settles which stubs jump through a lazy pointer (and so have a stub
/// helper entry), once the stubs are made. Stubs and GOT slots stay in
/// the order relocations first reached them. Runs again when branch
/// shims add stubs.
pub fn finish_stubs<E: Target>(ctx: &mut Context<E>) {
    let stubs = &ctx.stubs.symbols;
    let lazy = |id| !ctx.binds_weak_lookup(id) && !ctx.has_branch_shim(id);
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

/// Collects the relocations of an image no dyld loads (LC_DYSYMTAB's):
/// a -static -pie image's local ones, and a kext's local and external
/// ones.
fn collect_relocations<E: Target>(ctx: &mut Context<E>) {
    if ctx.chunks.contains(&ChunkId::LocalRelocs) {
        ctx.local_relocs.locs = chunks::local_relocs::build(ctx);
        ctx.local_relocs.hdr.size = (ctx.local_relocs.locs.len() * size_of::<MachRel>()) as u64;
    }
    if ctx.chunks.contains(&ChunkId::ExternRelocs) {
        ctx.extern_relocs.relocs = chunks::extern_relocs::build(ctx);
        ctx.extern_relocs.hdr.size = (ctx.extern_relocs.relocs.len() * size_of::<MachRel>()) as u64;
    }
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

    while !finish_chain_starts(ctx) {
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
fn report_text_relocs<E: Target>(ctx: &Context<E>) {
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
    let size = encode_unwind_info(ctx);
    if size > ctx.unwind_info.hdr.size {
        ctx.unwind_info.min_size = size;
        return false;
    }
    true
}

/// Encodes __unwind_info for the addresses its segment has, and returns
/// its size. The personality cells the encoding cannot know yet (GOT
/// addresses) come back as a patch list for the copy phase.
fn encode_unwind_info<E: Target>(ctx: &mut Context<E>) -> u64 {
    let (data, personalities) = {
        let _t = ctx.timer("unwind_encode");
        chunks::unwind_info::encode_unwind_info(ctx)
    };
    let size = data.len() as u64;
    ctx.unwind_info.contents = data;
    ctx.unwind_info.personalities = personalities;
    size
}

/// Finds where the chains __TEXT,__chain_starts lists start, which
/// follows from where the fixups in the other segments are. Returns
/// false if their number changes the section's size; the layout is
/// then done again (which moves later segments whole, and so no chain).
fn finish_chain_starts<E: Target>(ctx: &mut Context<E>) -> bool {
    if !ctx.args.fixup_chains_section {
        return true;
    }
    let starts = chunks::chained_fixups::section_chain_starts(ctx);
    let size = chunks::chain_starts::ChainStartsSection::size(starts.len());
    let fits = size == ctx.chain_starts.hdr.size;
    ctx.chain_starts.hdr.size = size;
    ctx.chain_starts.starts = starts;
    fits
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
        let align = 1 << chunk_p2align(ctx, id);
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
            ChunkId::UnwindInfo => encode_unwind_info(ctx).max(ctx.unwind_info.min_size),
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

/// The alignment a chunk starts on: a section's own, and a __LINKEDIT
/// table's as ld-prime gives it. ld-prime starts the dyld opcodes, the
/// chained fixups and the local relocations wherever the table before
/// them ends - the first of them where __LINKEDIT starts, which a
/// -segalign below 8 leaves unaligned (each table's size is a multiple
/// of 8).
fn chunk_p2align<E: Target>(ctx: &Context<E>, id: ChunkId) -> u32 {
    match id {
        ChunkId::RebaseInfo
        | ChunkId::BindInfo
        | ChunkId::WeakBindInfo
        | ChunkId::LazyBindInfo
        | ChunkId::ChainedFixups
        | ChunkId::LocalRelocs => 0,
        ChunkId::Symtab
        | ChunkId::Strtab
        | ChunkId::ExportTrie
        | ChunkId::FunctionStarts
        | ChunkId::DataInCode
        | ChunkId::MergeableRecord
        | ChunkId::SplitInfo
        | ChunkId::ExternRelocs => 3,
        ChunkId::IndirectSymtab => 2,
        ChunkId::CodeSignature => 4,
        _ => ctx.chunk_header(id).p2align,
    }
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
    ctx.symtab.hdr.size = (ctx.symtab.len() * size_of::<NList>()) as u64;
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
    collect_relocations(ctx);
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

/// Gives an entry point that resolved to a dylib export the stub that
/// LC_MAIN will name; runs after scan_relocations, with the stubs of
/// the branch targets.
pub fn add_entry_stub<E: Target>(ctx: &mut Context<E>) {
    if !ctx.args.has_entry_point() {
        return;
    }
    if let Some(id) = ctx.symbols.get(&ctx.args.entry)
        && ctx.symbols[id].is_imported()
    {
        add_import_stub(ctx, id);
    }
}

/// With lazy binding, the stub helper enters dyld through
/// dyld_stub_binder (libSystem's): the symbol is bound from whichever
/// loaded dylib exports it - or, where the image may look it up
/// dynamically (-undefined dynamic_lookup, -U), from whatever image dyld
/// finds it in - given a GOT slot, and __dyld_private (the word
/// dyld_stub_binder is handed, ld64 puts it in __DATA,__data) is
/// synthesized. Once, on the first stub.
fn ensure_stub_binder<E: Target>(ctx: &mut Context<E>) {
    // Legacy LINKEDIT's helper enters dyld through crt1.o's
    // dyld_stub_binding_helper instead (see resolve_stub_binder).
    if ctx.stub_helper.dyld_stub_binder.is_some() || ctx.args.legacy_linkedit {
        return;
    }
    let Some(id) = bind_linker_import(ctx, b"dyld_stub_binder") else {
        fatal!("lazy binding needs dyld_stub_binder, which no loaded dylib exports");
    };
    ctx.symbols[id].set_is_used(true);
    add_got(ctx, id);
    ctx.stub_helper.dyld_stub_binder = Some(id);
    let isec = add_data_word(ctx, 8);
    ctx.stub_helper.dyld_private_isec = isec;
    ctx.extra_local_syms.push((b"__dyld_private", isec));
}

/// Synthesizes a zero word of `size` bytes, aligned to its size, in
/// __DATA,__data (after the inputs'), and returns its subsection.
pub(crate) fn add_data_word<E: Target>(ctx: &mut Context<E>, size: u32) -> u32 {
    let p2align = size.trailing_zeros() as u8;
    let (file, shndx) = ctx.add_synthetic_section(MachSection {
        sectname: bytes_to_name(b"__data"),
        segname: bytes_to_name(b"__DATA"),
        p2align: p2align as u32,
        flags: 0,
        ..Default::default()
    });
    ctx.isecs.push(InputSection {
        flags: InputSection::flags_placed(),
        ..InputSection::new(file, shndx, p2align, size, &[])
    });
    let isec = (ctx.isecs.len() - 1) as u32;
    let fields = vec![DataField::Bytes(vec![0; size as usize])];
    ctx.data_blobs.push(DataBlob { sect: b"__data", isec, fields });
    isec
}

/// Binds a symbol the linker's own code calls (dyld_stub_binder,
/// __dyld_lazy_load), unless something in the link defines it, to the
/// first loaded dylib that exports it - or, if none does and the image
/// may look the symbol up dynamically (-undefined dynamic_lookup, -U),
/// to whatever image dyld finds it in. None if neither.
pub(crate) fn bind_linker_import<E: Target>(
    ctx: &mut Context<E>,
    name: &'static [u8],
) -> Option<SymbolId> {
    let args = &ctx.args;
    let looked_up =
        args.undefined_dynamic_lookup || args.allowed_undefined.iter().any(|n| n == name);
    let dylib = match ctx.dylibs.iter().position(|d| d.exports.contains(name)) {
        Some(i) => i as u32,
        None if looked_up => u32::MAX,
        None => return None,
    };
    let id = ctx.symbols.intern(name);
    let sym = &mut ctx.symbols[id];
    if !sym.is_defined() {
        sym.set_file(FileId::Dylib(dylib));
        sym.set_is_imported(true);
        sym.set_is_extern(true);
        sym.set_input_section(None);
    }
    Some(id)
}

/// Finds the helper legacy LINKEDIT's stub helper entries jump to. It
/// binds no dyld_stub_binder: its entries go to dyld_stub_binding_helper,
/// which crt1.o, dylib1.o or bundle1.o defines; no dylib exports it.
/// (Otherwise dyld_stub_binder is bound once a stub needs it; see
/// ensure_stub_binder.)
pub fn resolve_stub_binder<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.legacy_linkedit {
        let id = ctx.symbols.get(b"dyld_stub_binding_helper");
        let id = id.filter(|&id| ctx.symbols[id].input_section().is_some());
        ctx.stub_helper.binding_helper = id;
    }
}

/// Copies all chunks to the output buffer and applies relocations, in
/// parallel: the buffer is carved into disjoint per-chunk slices, and
/// every chunk writes only within its own. The fixups, the symbol table
/// (which also fills the string table), the mach header, the UUID and
/// the code signature follow serially, in that order, since each
/// depends on the bytes before it. Each range of the buffer is queued
/// to `out` the moment it is final, so the file is written while the
/// rest is produced: everything between the header and the symbol
/// table after the copy and its fix-ups, the symbol and string tables
/// after copy_symtab, the header after the UUID, the signature last.
pub fn copy_chunks<E: Target>(
    ctx: &Context<E>,
    buf: &mut [u8],
    out: &crate::output_file::OutputFile,
) {
    let jobs: Vec<(ChunkId, Range<u64>)> = ctx
        .chunks
        .iter()
        .map(|&id| (id, ctx.chunk_header(id)))
        .filter(|(id, hdr)| {
            !matches!(
                id,
                ChunkId::MachHeader | ChunkId::Symtab | ChunkId::Strtab | ChunkId::CodeSignature
            ) && !hdr.is_zerofill()
                // An empty section (every subsection of a coverage
                // section dead, say) shares its file offset with its
                // neighbor; it has nothing to copy, and its range would
                // start inside the neighbor's.
                && hdr.size != 0
        })
        .map(|(id, hdr)| (id, hdr.fileoff..hdr.fileoff + hdr.size))
        .collect();
    let ranges: Vec<Range<u64>> = jobs.iter().map(|(_, range)| range.clone()).collect();
    let slices = crate::output_file::split_ranges(buf, &ranges);

    let t = ctx.timer("copy_chunks");
    jobs.par_iter().zip(slices).for_each(|(&(id, _), slice)| chunks::copy_buf(ctx, id, slice));
    drop(t);
    // Relocations that failed to apply fail the link before the fixups
    // are written.
    report_text_relocs(ctx);
    crate::error::checkpoint();

    if ctx.use_chained_fixups() {
        let _t = ctx.timer("write_fixup_chains");
        chunks::chained_fixups::write_fixup_chains(ctx, buf);
    }
    if ctx.chunks.contains(&ChunkId::LocalRelocs) {
        chunks::local_relocs::write(ctx, buf);
    }
    if ctx.chunks.contains(&ChunkId::ExternRelocs) {
        chunks::extern_relocs::write(ctx, buf);
    }
    let t = ctx.timer("apply_optimization_hints");
    E::apply_optimization_hints(ctx, buf);
    drop(t);

    let hdr_end = ctx.mach_header.hdr.size as usize;
    let sig_start = if ctx.chunks.contains(&ChunkId::CodeSignature) {
        ctx.code_signature.hdr.fileoff as usize
    } else {
        buf.len()
    };
    let symtab_start = (ctx.symtab.hdr.fileoff as usize).min(ctx.strtab.hdr.fileoff as usize);

    // Nothing below writes between the header and the symbol table.
    out.queue(hdr_end, symtab_start - hdr_end);
    let t = ctx.timer("copy_symtab");
    chunks::symtab::copy_symtab(ctx, buf);
    drop(t);
    out.queue(symtab_start, sig_start - symtab_start);
    chunks::copy_mach_header(ctx, buf);

    let hashes = compute_uuid(ctx, buf, sig_start);
    out.queue(0, hdr_end);

    if ctx.args.adhoc_codesign {
        let _t = ctx.timer("write_code_signature");
        chunks::code_signature::write(ctx, buf, &hashes);
    }
    out.queue(sig_start, buf.len() - sig_start);
}

/// Computes the UUID that identifies the build and writes it into the
/// header's LC_UUID, and returns the SHA256 hashes of every 4KiB page
/// before the code signature at `sig_start`, which the signature is
/// made of.
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
fn compute_uuid<E: Target>(ctx: &Context<E>, buf: &mut [u8], sig_start: usize) -> Vec<[u8; 32]> {
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
