//! Delay-init dylibs (-delay-l, -delay_library, -delay_framework): a
//! dylib dyld loads and binds at launch as any other, but whose
//! initializers run only once the image dlopen()s it, before its first
//! use of one of the dylib's symbols. ld-prime gives the dylib a
//! dylib_use_command with DYLIB_USE_DELAYED_INIT and reaches its
//! symbols through code that dlopen()s it once (see chunks::delay_init);
//! the public libraries such a dylib re-exports are delayed with it,
//! by its dlopen helper.

use rayon::prelude::*;

use crate::chunks::delay_init::{DelayHelper, DelayStub, DelayUse, DlopenHelper};
use crate::context::Context;
use crate::input_files::{self, FileId};
use crate::macho::*;
use crate::symbol::{NO_IDX, SymbolId};
use crate::target::{LazyRef, Target};
use crate::util::leak_bytes;

/// A reference to a delay-init dylib's symbol: the subsection, the
/// relocation's offset in it, the symbol, and how it refers to it.
type DelayUseSite = (u32, u32, SymbolId, LazyRef);

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
    // (A mergeable dylib can't keep them; see passes::create_lazy_loads.)
    if ctx.args.make_mergeable && !uses.is_empty() {
        crate::fatal!(
            "-delay-l/-delay_library/-delay_framework cannot be used with -make_mergeable"
        );
    }
    let dlopen_of = create_dlopen_helpers(ctx, &uses);
    if ctx.delay_init.dlopens.is_empty() {
        return;
    }
    create_delay_stubs(ctx, &uses, &dlopen_of);
    create_delay_helpers(ctx, &uses, &dlopen_of);

    // The dlopen helpers call _dlopen through its stub.
    if let Some(id) = ctx.symbols.get(b"_dlopen") {
        crate::passes::add_stub(ctx, id);
        if ctx.args.lazy_binding {
            crate::passes::ensure_stub_binder(ctx);
        } else {
            crate::passes::add_got(ctx, id);
        }
        ctx.delay_init.dlopen_sym = Some(id);
    }
}

/// The references to delay-init dylibs' symbols from live subsections,
/// in input order, once the ones ld-prime refuses are reported: by the
/// name of the fixup, or for a class an __objc_classrefs slot points
/// at - what macOS before 15 keeps, where later ones load the class
/// from the GOT (see objc::fold_objc_classrefs) - by the class.
fn delay_uses<E: Target>(ctx: &Context<E>) -> Vec<DelayUseSite> {
    let uses: Vec<DelayUseSite> = (0..ctx.isecs.len())
        .into_par_iter()
        .filter(|&i| {
            let isec = &ctx.isecs[i];
            isec.is_alive() && isec.replacement == crate::input_sections::NO_REPLACEMENT
        })
        .flat_map_iter(|i| {
            let (file, data) = (ctx.isecs[i].file as usize, ctx.isecs[i].data());
            ctx.isec_relocs(i).iter().filter_map(move |r| {
                let id = ctx.reloc_target_sym(file, r)?;
                ctx.is_delay_import(id).then(|| (i as u32, r.offset, id, E::lazy_ref(r, data)))
            })
        })
        .collect();
    for &(isec, _, id, how) in &uses {
        if how != LazyRef::Unsupported {
            continue;
        }
        let sym = &ctx.symbols[id];
        let sec = &ctx.isecs[isec as usize];
        let hdr = ctx.hdr_of(sec);
        if hdr.sectname() == b"__objc_classrefs"
            && let Some(class) = sym.name().strip_prefix(b"_OBJC_CLASS_$_")
        {
            let file = crate::passes::resolved_file_name(ctx.objs[sec.file as usize].mf);
            let class = crate::error::raw(class);
            crate::error!(
                "use of ObjC class '{class}' in '{file}' cannot be delayed when targeting an older OS versions"
            );
            continue;
        }
        let split = ctx.objs[sec.file as usize].subsections_via_symbols;
        let subsec = if input_files::is_record_list(hdr, split) {
            b"anon"[..].into()
        } else {
            ctx.subsec_name(isec as usize)
        };
        let subsec = crate::error::raw(&subsec);
        crate::error!("use of '{sym}' in '{subsec}' cannot be delayed.");
    }
    crate::error::checkpoint();
    uses
}

/// The install name of the dylib a delay-init dylib's dlopen helper
/// dlopen()s: its own, or for a library it re-exports, its own.
fn dlopen_name<E: Target>(ctx: &Context<E>, id: SymbolId) -> &[u8] {
    let Some(FileId::Dylib(d)) = ctx.symbols[id].file() else { unreachable!() };
    ctx.dylibs[d as usize].delay_init.as_deref().unwrap()
}

/// Gives each dylib the stubs and helpers dlopen() its dlopen helper,
/// by install name: the helper's flag word in __data (ahead of
/// __dyld_private) and the install name's C string, after the inputs'
/// in __cstring. ld-prime merges an input's copy of the string into its
/// own. Returns the helper of each install name.
fn create_dlopen_helpers<E: Target>(
    ctx: &mut Context<E>,
    uses: &[DelayUseSite],
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

    let copies = input_cstrings(ctx, &names);
    let mut dlopen_of = hashbrown::HashMap::new();
    for (i, install_name) in names.into_iter().enumerate() {
        let leaf = install_name.rsplit(|&c| c == b'/').next().unwrap_or(&install_name);
        let name = leak_bytes([b"_dlopenHelper$", leaf].concat());
        let flag_name = leak_bytes([b"_dlopenHelperFlag$", leaf].concat());
        let flag = crate::passes::add_data_word(ctx, 4);
        ctx.extra_local_syms.push((flag_name, flag));
        let string = add_cstring(ctx, &install_name);
        for &(copy, _) in copies.iter().filter(|&&(_, j)| j == i) {
            ctx.isecs[copy as usize].replacement = string;
        }
        dlopen_of.insert(install_name.clone(), i as u32);
        let offset = 0;
        let helper = DlopenHelper { install_name, name, flag_name, flag, string, offset };
        ctx.delay_init.dlopens.push(helper);
    }
    if !copies.is_empty() {
        crate::passes::redirect_symbols_to_replacements(ctx);
    }
    let private = ctx.stub_helper.dyld_private_isec;
    if let Some(i) = ctx.data_blobs.iter().position(|b| b.isec == private) {
        let blob = ctx.data_blobs.remove(i);
        ctx.data_blobs.push(blob);
    }
    dlopen_of
}

/// The inputs' __cstring literals that spell one of `names`, with the
/// index of the name.
fn input_cstrings<E: Target>(ctx: &Context<E>, names: &[Vec<u8>]) -> Vec<(u32, usize)> {
    let mut found = Vec::new();
    for (i, isec) in ctx.isecs.iter().enumerate() {
        if !isec.is_alive()
            || ctx.is_internal(isec.file as usize)
            || isec.replacement != crate::input_sections::NO_REPLACEMENT
        {
            continue;
        }
        let hdr = ctx.hdr_of(isec);
        if hdr.segname() != b"__TEXT"
            || hdr.sectname() != b"__cstring"
            || hdr.section_type() != S_CSTRING_LITERALS
        {
            continue;
        }
        let Some(data) = isec.data().strip_suffix(b"\0") else { continue };
        if let Some(j) = names.iter().position(|n| n.as_slice() == data) {
            found.push((i as u32, j));
        }
    }
    found
}

/// Synthesizes a C string in __TEXT,__cstring, after the inputs', and
/// returns its subsection.
fn add_cstring<E: Target>(ctx: &mut Context<E>, s: &[u8]) -> u32 {
    let (file, shndx) = ctx.add_synthetic_section(MachSection {
        sectname: bytes_to_name(b"__cstring"),
        segname: bytes_to_name(b"__TEXT"),
        flags: S_CSTRING_LITERALS,
        ..Default::default()
    });
    let mut bytes = s.to_vec();
    bytes.push(0);
    let bytes: &'static [u8] = Vec::leak(bytes);
    ctx.isecs.push(crate::input_sections::InputSection {
        file,
        shndx,
        p2align: 0,
        input_addr: 0,
        size: bytes.len() as u32,
        contents: bytes.as_ptr() as usize,
        rel_offset: 0,
        nrels: 0,
        output_section: u32::MAX,
        offset: 0,
        flags: crate::input_sections::InputSection::flags_alive_no_modulus(),
        replacement: crate::input_sections::NO_REPLACEMENT,
        unwind_offset: 0,
        nunwind: 0,
    });
    (ctx.isecs.len() - 1) as u32
}

/// Makes a stub for each symbol something calls, by name. A stub jumps
/// through a __got slot of its own when GOT loads of the symbol read
/// one, as in ld-prime, and through that one otherwise.
fn create_delay_stubs<E: Target>(
    ctx: &mut Context<E>,
    uses: &[DelayUseSite],
    dlopen_of: &hashbrown::HashMap<Vec<u8>, u32>,
) {
    let mut called: Vec<SymbolId> =
        uses.iter().filter(|u| u.3 == LazyRef::Call).map(|u| u.2).collect();
    called.sort_unstable_by_key(|&id| ctx.symbols[id].name());
    called.dedup();
    for (i, &id) in called.iter().enumerate() {
        let got = match ctx.sym_aux(id).got_idx {
            NO_IDX => {
                crate::passes::add_got(ctx, id);
                ctx.sym_aux(id).got_idx
            }
            _ => {
                ctx.got.got_syms.push(id);
                ctx.got.got_syms.len() as u32 - 1
            }
        };
        ctx.sym_aux_mut(id).delay_stub_idx = i as u32;
        let dlopen = dlopen_of[dlopen_name(ctx, id)];
        let name = leak_bytes([ctx.symbols[id].name(), b"$delayInitStub"].concat());
        ctx.delay_init.stubs.push(DelayStub { sym: id, name, dlopen, got });
    }
}

/// Makes the helpers for GOT loads and compares: one per symbol and
/// register, or per load in arm64 frameless code, by symbol, a
/// symbol's in the order of their first uses (where ld-prime's lazy-load
/// helpers go by their own names); then lays out __delay_helper, the
/// dlopen helpers after them.
fn create_delay_helpers<E: Target>(
    ctx: &mut Context<E>,
    uses: &[DelayUseSite],
    dlopen_of: &hashbrown::HashMap<Vec<u8>, u32>,
) {
    let mut helpers: Vec<DelayHelper> = Vec::new();
    let mut index: hashbrown::HashMap<(SymbolId, DelayUse), usize> = hashbrown::HashMap::new();
    let mut sites = Vec::new();
    for &(isec, offset, id, how) in uses {
        let kind = match how {
            LazyRef::Cmp => DelayUse::Cmp,
            LazyRef::Load => {
                let (reg, own) = E::lazy_load_site(ctx.isecs[isec as usize].data(), offset);
                DelayUse::Load { reg, site: own.then_some((isec, offset)) }
            }
            _ => continue,
        };
        let i = *index.entry((id, kind)).or_insert_with(|| {
            let sym = ctx.symbols[id].name();
            let name = match kind {
                DelayUse::Cmp => [sym, b"$cmpHelper"].concat(),
                DelayUse::Load { reg, site: None } => {
                    [sym, b"$loadHelper_", E::lazy_register_name(reg).as_bytes()].concat()
                }
                DelayUse::Load { reg, site: Some(_) } => [
                    sym,
                    b"$loadHelper_",
                    E::lazy_register_name(reg).as_bytes(),
                    b"$for$",
                    &ctx.subsec_name(isec as usize),
                    format!("+{offset}").as_bytes(),
                ]
                .concat(),
            };
            let name = leak_bytes(name);
            let dlopen = dlopen_of[dlopen_name(ctx, id)];
            helpers.push(DelayHelper { sym: id, kind, name, dlopen, offset: 0 });
            helpers.len() - 1
        });
        sites.push(((isec, offset), i));
    }

    let mut sorted: Vec<(usize, DelayHelper)> = helpers.into_iter().enumerate().collect();
    sorted.sort_by_key(|(_, h)| ctx.symbols[h.sym].name());
    let mut rank = vec![0; sorted.len()];
    let mut offset = 0;
    for (r, (i, h)) in sorted.iter_mut().enumerate() {
        rank[*i] = r as u32;
        h.offset = offset;
        offset += E::delay_helper_size(h.kind);
    }
    for d in &mut ctx.delay_init.dlopens {
        d.offset = offset;
        offset += E::DLOPEN_HELPER_SIZE;
    }
    let delay = &mut ctx.delay_init;
    delay.sites = sites.into_iter().map(|(site, i)| (site, rank[i])).collect();
    delay.helpers = sorted.into_iter().map(|(_, h)| h).collect();
}
