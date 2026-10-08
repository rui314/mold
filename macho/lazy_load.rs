//! Lazy dylibs (-lazy-l, -lazy_library, -lazy_framework, from macOS 27
//! on): a dylib dyld loads only once the image first uses one of its
//! symbols. Such a dylib has no LC_LOAD_DYLIB: it has an
//! LC_LAZY_LOAD_DYLIB_INFO record (see chunks::lazy_load_info) naming
//! it, a flag word and the symbols the image uses from it, each with a
//! __lazy_load_got slot, which the image reaches through the helpers of
//! chunks::lazy_helpers.

use rayon::prelude::*;

use crate::arch::{LazyRef, Target};
use crate::chunks::lazy_helpers::{LazyHelper, LazyUse};
use crate::chunks::lazy_load_info::{LazyDylib, record_size};
use crate::context::Context;
use crate::error::raw;
use crate::input_files::FileId;
use crate::symbol::SymbolId;
use crate::util::leak_bytes;

/// Binds __dyld_lazy_load, which the lazy-load helpers call (see
/// create_lazy_loads), in an image that uses a symbol of a lazy dylib,
/// before the dylibs no symbol binds to are dropped.
pub fn bind_dyld_lazy_load<E: Target>(ctx: &mut Context<E>) {
    if !ctx.dylibs.iter().any(|d| d.is_lazy) {
        return;
    }
    let ctx_ref: &Context<E> = ctx;
    let uses_lazy =
        ctx_ref.symbols.syms.par_iter().any(|sym| sym.is_used() && sym.is_lazy_import(ctx_ref));
    if uses_lazy && let Some(id) = ctx.bind_linker_import(b"__dyld_lazy_load") {
        ctx.symbols[id].set_used(true);
    }
}

/// Makes what the image reaches the symbols of its lazy dylibs through,
/// as ld-prime does. Calls go to a helper per symbol that jumps
/// through the slot once the flag says the dylib is loaded, and first
/// has __dyld_lazy_load (libdyld's, called through a stub as any
/// import) load it and bind the slots; a GOT load calls a helper that
/// goes through the slot likewise (see LazyUse). ld-prime refuses any
/// other reference, such as a pointer in data, which dyld would have
/// to bind at launch; the error names the fixup as it does.
///
/// A mergeable dylib would record a rewritten instruction under the
/// compiler's fixup, for no merge to relink: ld-prime crashes on one
/// that uses a lazy dylib (and makes a dylib no link can merge of one
/// that uses a delay-init dylib), which mold refuses; one that names
/// such a dylib but uses nothing of it links as any other.
pub fn create_lazy_loads<E: Target>(ctx: &mut Context<E>) {
    if !ctx.dylibs.iter().any(|d| d.is_lazy) {
        return;
    }
    let uses = lazy_uses(ctx);
    if ctx.args.make_mergeable && !uses.is_empty() {
        crate::fatal!("-lazy-l/-lazy_library/-lazy_framework cannot be used with -make_mergeable");
    }
    let (flags, slots) = create_lazy_load_slots(ctx, &uses);
    create_lazy_helpers(ctx, &uses, &flags, &slots);

    // The helpers call __dyld_lazy_load through its stub (see
    // bind_dyld_lazy_load).
    if !ctx.lazy_helpers.helpers.is_empty() {
        let id = ctx.symbols.lookup(b"__dyld_lazy_load").filter(|&id| ctx.symbols[id].is_defined());
        let Some(id) = id else {
            crate::fatal!("lazy-load dylibs need __dyld_lazy_load, which no loaded dylib exports");
        };
        crate::chunks::stubs::add_symbol(ctx, id);
        ctx.lazy_helpers.dyld_lazy_load = Some(id);
    }
}

/// A reference to a symbol of a lazy or delay-init dylib: the
/// subsection, the relocation's offset in it, the symbol, and how it
/// refers to it.
pub(crate) type LazyUseSite = (u32, u32, SymbolId, LazyRef);

/// A __lazy_load_got slot: its symbol, and whether it is the one the
/// symbol's call helper has to itself (see Target::LAZY_CALL_OWN_SLOT).
type LazySlot = (SymbolId, bool);

/// The references from live subsections, in input order, to the
/// symbols `is_import` picks: a lazy dylib's, or a delay-init one's.
pub(crate) fn import_uses<E: Target>(
    ctx: &Context<E>,
    is_import: impl Fn(SymbolId) -> bool + Sync,
) -> Vec<LazyUseSite> {
    let is_import = &is_import;
    (0..ctx.isecs.len())
        .into_par_iter()
        .filter(|&i| ctx.isecs[i].is_emitted())
        .flat_map_iter(|i| {
            let (file, data) = (ctx.isecs[i].file as usize, ctx.isecs[i].data());
            ctx.isecs[i].rels(&ctx.objs[file]).iter().filter_map(move |r| {
                let id = ctx.reloc_target_sym(file, r)?;
                is_import(id).then(|| (i as u32, r.offset, id, E::lazy_ref(r, data)))
            })
        })
        .collect()
}

/// The references to lazy dylibs' symbols from live subsections, in
/// input order, once the ones ld-prime refuses are reported.
fn lazy_uses<E: Target>(ctx: &Context<E>) -> Vec<LazyUseSite> {
    let uses = import_uses(ctx, |id| ctx.symbols[id].is_lazy_import(ctx));
    for &(isec, _, id, how) in &uses {
        if how == LazyRef::Unsupported {
            let sym = &ctx.symbols[id];
            let sec = &ctx.isecs[isec as usize];
            let file = &ctx.objs[sec.file as usize];
            let subsec = if crate::input_files::is_record_list(
                sec.hdr(file),
                file.subsections_via_symbols,
            ) {
                b"anon"[..].into()
            } else {
                ctx.subsec_name(isec as usize)
            };
            let subsec = raw(&subsec);
            crate::error!("use of '{sym}' in '{subsec}' cannot be lazy loaded.");
        }
    }
    // A stub or GOT slot another pass made for one (an unwind
    // personality's, a class's whose __objc_classrefs slot stays, see
    // objc::fold_objc_classrefs) would be a pointer dyld binds at
    // launch, and is refused alike. (ld-prime leaves a personality's
    // slot zero.)
    for &id in ctx.stubs.symbols.iter().chain(&ctx.got.got_syms) {
        let sym = &ctx.symbols[id];
        if sym.is_lazy_import(ctx) {
            crate::error!("use of '{sym}' in 'anon' cannot be lazy loaded.");
        }
    }
    crate::error::checkpoint();
    uses
}

/// The slot a use of a symbol goes through.
fn lazy_slot<E: Target>(id: SymbolId, how: LazyRef) -> LazySlot {
    (id, E::LAZY_CALL_OWN_SLOT && how == LazyRef::Call)
}

/// Gives each lazy dylib the image uses its flag word, its symbols
/// their __lazy_load_got slots and it its record, all in the order of
/// the image's first uses. Returns each dylib's flag word's subsection,
/// and each slot's index.
fn create_lazy_load_slots<E: Target>(
    ctx: &mut Context<E>,
    uses: &[LazyUseSite],
) -> (Vec<u32>, hashbrown::HashMap<LazySlot, u32>) {
    // The dylibs used, and each one's slots, which follow one another.
    let mut used: Vec<usize> = Vec::new();
    let mut by_dylib: Vec<Vec<LazySlot>> = vec![Vec::new(); ctx.dylibs.len()];
    let mut seen = hashbrown::HashSet::new();
    for &(_, _, id, how) in uses {
        let Some(FileId::Dylib(d)) = ctx.symbols[id].file() else { continue };
        let slot = lazy_slot::<E>(id, how);
        if seen.insert(slot) {
            if by_dylib[d as usize].is_empty() {
                used.push(d as usize);
            }
            by_dylib[d as usize].push(slot);
        }
    }

    let mut flags = vec![u32::MAX; ctx.dylibs.len()];
    let mut index = hashbrown::HashMap::new();
    let mut slots = Vec::new();
    let mut offset = 0;
    for d in used {
        let flag = ctx.add_data_word(4);
        let install_name = &ctx.dylibs[d].install_name;
        let leaf = install_name.rsplit(|&c| c == b'/').next().unwrap_or(install_name);
        let name = leak_bytes([b"_lazyLoadFlag$", leaf].concat());
        ctx.extra_local_syms.push((name, flag));
        flags[d] = flag;

        let got_start = slots.len() as u32;
        let list = std::mem::take(&mut by_dylib[d]);
        for &(id, own) in &list {
            index.insert((id, own), slots.len() as u32);
            // The slot an arm64 ldr loads (see LazyRef::Slot).
            if !own {
                ctx.symbols.aux_mut(id).lazy_got_idx = slots.len() as u32;
            }
            let name = leak_bytes([ctx.symbols[id].name(), b"$lazyGOT"].concat());
            slots.push((id, name));
        }
        let syms: Vec<_> = list.into_iter().map(|(id, _)| id).collect();
        let size = record_size(ctx, &ctx.dylibs[d].install_name, &syms);
        let dylib = d as u32;
        ctx.lazy_load_info.dylibs.push(LazyDylib { dylib, flag, syms, got_start, offset, size });
        offset += size;
    }
    ctx.lazy_load_got.slots = slots;
    ctx.lazy_load_info.hdr.size = offset as u64;
    (flags, index)
}

/// Makes the helpers, in the order of the image's first uses: one per
/// symbol for calls, and one per symbol and register for GOT loads, or
/// per load in arm64 frameless code.
fn create_lazy_helpers<E: Target>(
    ctx: &mut Context<E>,
    uses: &[LazyUseSite],
    flags: &[u32],
    slots: &hashbrown::HashMap<LazySlot, u32>,
) {
    let mut helpers: Vec<LazyHelper> = Vec::new();
    let mut index: hashbrown::HashMap<(SymbolId, LazyUse), usize> = hashbrown::HashMap::new();
    let mut sites = hashbrown::HashMap::new();
    let mut size = 0;
    for &(isec, offset, id, how) in uses {
        let kind = match how {
            LazyRef::Call => LazyUse::Call,
            LazyRef::Cmp => LazyUse::Cmp,
            LazyRef::Load => {
                let (reg, own) = E::lazy_load_site(ctx.isecs[isec as usize].data(), offset);
                LazyUse::Load { reg, site: own.then_some((isec, offset)) }
            }
            LazyRef::Slot | LazyRef::Unsupported => continue,
        };
        let i = *index.entry((id, kind)).or_insert_with(|| {
            let sym = ctx.symbols[id].name();
            let name = match kind {
                LazyUse::Call => [sym, b"$lazyLoadStub"].concat(),
                LazyUse::Cmp => [sym, b"$lazyGOT$cmpHelper"].concat(),
                LazyUse::Load { reg, site } => load_helper_name(ctx, sym, b"$lazyGOT$", reg, site),
            };
            let Some(FileId::Dylib(d)) = ctx.symbols[id].file() else { unreachable!() };
            let (flag, slot) = (flags[d as usize], slots[&lazy_slot::<E>(id, how)]);
            let name = leak_bytes(name);
            helpers.push(LazyHelper { sym: id, kind, name, flag, slot, offset: size });
            size += E::lazy_helper_size(kind);
            helpers.len() - 1
        });
        if how != LazyRef::Call {
            sites.insert((isec, offset), i as u32);
        }
    }

    for (i, h) in helpers.iter().enumerate().filter(|(_, h)| h.kind == LazyUse::Call) {
        ctx.symbols.aux_mut(h.sym).lazy_stub_idx = i as u32;
    }
    ctx.lazy_helpers.sites = sites;
    ctx.lazy_helpers.helpers = helpers;
}

/// The name of a helper for GOT loads of symbol `sym` into register
/// `reg`, a lazy dylib's or a delay-init one's, as ld-prime names it:
/// `<sym><infix>loadHelper_<reg>`, and for the helper of one load of
/// its own (see LazyUse::Load), `$for$<subsection>+<offset>` after it.
pub(crate) fn load_helper_name<E: Target>(
    ctx: &Context<E>,
    sym: &[u8],
    infix: &[u8],
    reg: u8,
    site: Option<(u32, u32)>,
) -> Vec<u8> {
    let mut name = [sym, infix, b"loadHelper_", E::lazy_register_name(reg).as_bytes()].concat();
    if let Some((isec, offset)) = site {
        name.extend_from_slice(b"$for$");
        name.extend_from_slice(&ctx.subsec_name(isec as usize));
        name.extend_from_slice(format!("+{offset}").as_bytes());
    }
    name
}
