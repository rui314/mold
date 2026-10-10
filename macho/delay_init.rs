//! This file implements delay-init dylibs (-delay-l, -delay_library and
//! -delay_framework, which dyld supports from macOS 15 on). dyld loads and
//! binds a delay-init dylib at startup like any other dylib, but doesn't
//! run its initializers (its static constructors) then. They run only when
//! the program first uses one of the dylib's symbols, which keeps them out
//! of the program's startup time.
//!
//! The image names such a dylib with a load command flagged as delayed
//! (DYLIB_USE_DELAYED_INIT). dyld runs the dylib's initializers when the
//! program calls dlopen() on the dylib. So the linker rewrites each
//! reference to one of the dylib's symbols to go through a small piece of
//! code (see chunks::delay_init) that calls dlopen() the first time, and
//! then goes on through the symbol's GOT slot as usual. A reference that
//! can't be rewritten that way, such as a pointer in data, which dyld
//! fills at startup, before the dylib is initialized, is an error, as it
//! is with the macOS linker. A symbol of a library the delay-init dylib
//! re-exports goes through the same code, which dlopen()s the delay-init
//! dylib.

use mold_common::bytes::display;
use mold_common::mem::leak_bytes;
use mold_common::{error, fatal};

use crate::arch::{LazyRef, Target};
use crate::chunks::delay_init::{DelayHelper, DelayStub, DelayUse, DlopenHelper};
use crate::context::Context;
use crate::input_files::{FileId, add_cstring, add_data_word};
use crate::lazy_load::{LazyUseSite, import_uses, load_helper_name};
use crate::symbol::SymbolId;

/// Makes the stubs and helpers through which the image reaches the
/// symbols of its delay-init dylibs, as ld-prime does: calls branch to
/// a stub per symbol, GOT loads (and x86-64's compares of a GOT slot)
/// call a helper, and each first has the dylib's dlopen helper dlopen()
/// it, once; then they go on through the symbol's __got slot. ld-prime
/// refuses any other reference, such as a pointer in data, which dyld
/// binds at launch and so before the dylib is initialized.
pub fn create_delay_init<E: Target>(ctx: &mut Context<E>) {
    if !ctx.dylibs.iter().any(|d| d.delay_init.is_some()) {
        return;
    }
    let uses = delay_uses(ctx);
    // (A mergeable dylib can't keep them; see lazy_load::create_lazy_loads.)
    if ctx.args.make_mergeable && !uses.is_empty() {
        fatal!("-delay-l/-delay_library/-delay_framework cannot be used with -make_mergeable");
    }
    let dlopen_of = create_dlopen_helpers(ctx, &uses);
    if ctx.delay_init.dlopens.is_empty() {
        return;
    }
    create_delay_stubs(ctx, &uses, &dlopen_of);
    create_delay_helpers(ctx, &uses, &dlopen_of);

    // The dlopen helpers call _dlopen through its stub.
    if let Some(id) = ctx.symbols.lookup(b"_dlopen") {
        crate::chunks::stubs::add_symbol(ctx, id);
        ctx.delay_init.dlopen_sym = Some(id);
    }
}

/// The references to delay-init dylibs' symbols from live subsections,
/// in input order, once the ones that can't be delayed are reported:
/// among them an __objc_classrefs slot of a class, which only a link
/// below macOS 15 keeps (see objc::fold_objc_classrefs).
fn delay_uses<E: Target>(ctx: &Context<E>) -> Vec<LazyUseSite> {
    let uses = import_uses(ctx, |id| ctx.symbols[id].is_delay_import(ctx));
    for &(isec, _, id, how) in &uses {
        if how == LazyRef::Unsupported {
            let (sym, subsec) = (&ctx.symbols[id], ctx.isecs[isec as usize].name(ctx));
            let subsec = display(&subsec);
            error!("use of '{sym}' in '{subsec}' cannot be delayed");
        }
    }
    mold_common::error::checkpoint();
    uses
}

/// The install name of the dylib a delay-init dylib's dlopen helper
/// dlopen()s: its own, or for a library it re-exports, its own.
fn dlopen_name<E: Target>(ctx: &Context<E>, id: SymbolId) -> &[u8] {
    let Some(FileId::Dylib(d)) = ctx.symbols[id].file() else { unreachable!() };
    ctx.dylibs[d as usize].delay_init.as_deref().unwrap()
}

/// Gives each dylib the stubs and helpers dlopen() its dlopen helper,
/// by install name: the helper's flag word in __data and the install
/// name's C string in __cstring, after the inputs'. Returns the helper
/// of each install name.
fn create_dlopen_helpers<E: Target>(
    ctx: &mut Context<E>,
    uses: &[LazyUseSite],
) -> hashbrown::HashMap<Vec<u8>, u32> {
    let mut names: Vec<Vec<u8>> = uses
        .iter()
        .filter(|u| !matches!(u.3, LazyRef::Slot | LazyRef::Unsupported))
        .map(|u| dlopen_name(ctx, u.2).to_vec())
        .collect();
    names.sort_unstable();
    names.dedup();
    if names.is_empty() {
        return Default::default();
    }

    let mut dlopen_of = hashbrown::HashMap::new();
    for (i, install_name) in names.into_iter().enumerate() {
        let leaf = install_name.rsplit(|&c| c == b'/').next().unwrap_or(&install_name);
        let name = leak_bytes([b"_dlopenHelper$", leaf].concat());
        let flag_name = leak_bytes([b"_dlopenHelperFlag$", leaf].concat());
        let flag = add_data_word(ctx, 4);
        ctx.extra_local_syms.push((flag_name, flag));
        let string = add_cstring(ctx, &install_name);
        dlopen_of.insert(install_name.clone(), i as u32);
        let offset = 0;
        let helper = DlopenHelper { install_name, name, flag_name, flag, string, offset };
        ctx.delay_init.dlopens.push(helper);
    }
    dlopen_of
}

/// Makes a stub for each symbol something calls, by name, which jumps
/// through the symbol's __got slot.
fn create_delay_stubs<E: Target>(
    ctx: &mut Context<E>,
    uses: &[LazyUseSite],
    dlopen_of: &hashbrown::HashMap<Vec<u8>, u32>,
) {
    let mut called: Vec<SymbolId> =
        uses.iter().filter(|u| u.3 == LazyRef::Call).map(|u| u.2).collect();
    called.sort_unstable_by_key(|&id| ctx.symbols[id].name());
    called.dedup();
    for (i, &id) in called.iter().enumerate() {
        crate::chunks::got::add_got_symbol(ctx, id);
        let got = ctx.symbols[id].got_idx(&ctx.symbols).unwrap();
        ctx.symbols.aux_mut(id).delay_stub_idx = i as u32;
        let dlopen = dlopen_of[dlopen_name(ctx, id)];
        let name = leak_bytes([ctx.symbols[id].name(), b"$delayInitStub"].concat());
        ctx.delay_init.stubs.push(DelayStub { sym: id, name, dlopen, got });
    }
}

/// Makes the helpers for GOT loads and compares: one per symbol and
/// register, or per load in arm64 frameless code, in the order of their
/// first uses; then lays out __delay_helper, the dlopen helpers after
/// them.
fn create_delay_helpers<E: Target>(
    ctx: &mut Context<E>,
    uses: &[LazyUseSite],
    dlopen_of: &hashbrown::HashMap<Vec<u8>, u32>,
) {
    let mut helpers: Vec<DelayHelper> = Vec::new();
    let mut index: hashbrown::HashMap<(SymbolId, DelayUse), usize> = hashbrown::HashMap::new();
    let mut sites = hashbrown::HashMap::new();
    for &(isec, offset, id, how) in uses {
        let kind = match how {
            LazyRef::Cmp => DelayUse::Cmp,
            LazyRef::Load => {
                let (reg, own) = E::lazy_load_site(ctx.isecs[isec as usize].contents(), offset);
                DelayUse::Load { reg, site: own.then_some((isec, offset)) }
            }
            _ => continue,
        };
        let i = *index.entry((id, kind)).or_insert_with(|| {
            let sym = ctx.symbols[id].name();
            let name = match kind {
                DelayUse::Cmp => [sym, b"$cmpHelper"].concat(),
                DelayUse::Load { reg, site } => load_helper_name(ctx, sym, b"$", reg, site),
            };
            let name = leak_bytes(name);
            let dlopen = dlopen_of[dlopen_name(ctx, id)];
            helpers.push(DelayHelper { sym: id, kind, name, dlopen, offset: 0 });
            helpers.len() - 1
        });
        sites.insert((isec, offset), i as u32);
    }

    let mut offset = 0;
    for h in &mut helpers {
        h.offset = offset;
        offset += E::delay_helper_size(h.kind);
    }
    for d in &mut ctx.delay_init.dlopens {
        d.offset = offset;
        offset += E::DLOPEN_HELPER_SIZE;
    }
    ctx.delay_init.sites = sites;
    ctx.delay_init.helpers = helpers;
}
