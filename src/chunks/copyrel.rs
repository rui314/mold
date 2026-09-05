//! `.copyrel` and `.copyrel.rel.ro`, storage for copy relocations.

use crate::arch::Arch;
use crate::chunks::{self, ChunkHeader};
use crate::context::Context;
use crate::elf::*;
use crate::error;
use crate::input_files::FileId;
use crate::symbol::SymbolId;
use crate::util::align_to;

// .copyrel and .copyrel.rel.ro represent memory regions to which the
// runtime copies symbols from other ELF files for copy relocations.
#[derive(Debug)]
pub struct CopyrelSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
    pub symbols: Vec<SymbolId>,
}

impl<E: Layout> CopyrelSection<E> {
    pub fn new(is_relro: bool) -> CopyrelSection<E> {
        let name = if is_relro {
            ".copyrel.rel.ro"
        } else {
            ".copyrel"
        };
        let mut hdr = ChunkHeader::<E>::new(name, SHT_NOBITS, (SHF_ALLOC | SHF_WRITE) as u64);
        hdr.is_relro = is_relro;
        CopyrelSection {
            hdr,
            symbols: Vec::new(),
        }
    }
}

pub fn add_symbol<E: Arch>(ctx: &mut Context<E>, relro: bool, id: SymbolId) {
    debug_assert!(!ctx.args.shared);
    let sym = &ctx.symbols[id];
    if sym.has_copyrel() {
        return;
    }
    let Some(FileId::Dso(dso_id)) = sym.file() else {
        debug_assert!(sym.is_undef_weak());
        let file = ctx.file_display(sym.file().unwrap());
        error!("{file}: cannot create a copy relocation for {sym}; recompile with -fPIE or -fPIC");
        return;
    };
    let dso = &ctx.dsos[dso_id.index()];
    if sym.esym(ctx).st_visibility() == STV_PROTECTED {
        error!("{dso}: cannot create a copy relocation for protected symbol '{sym}'; recompile with -fPIC"
        );
        return;
    }
    if !ctx.args.z_copyreloc {
        error!("-z nocopyreloc: {dso}: cannot create a copy relocation for symbol '{sym}'; recompile with -fPIC"
        );
        return;
    }

    let alignment = dso.alignment(sym);
    let size = sym.esym(ctx).st_size().get();
    // We need to create dynamic symbols not only for this particular symbol
    // but also for its aliases (i.e. other symbols at the same address)
    // becasue otherwise the aliases are broken apart at runtime.
    // For example, `environ`, `_environ` and `__environ` in libc.so are
    // aliases. If one of the symbols is copied by a copy relocation, other
    // symbols have to refer to the copied place as well.
    let aliases: Vec<SymbolId> = dso.symbols_at(ctx, sym, dso_id).to_vec();

    let sec = if relro {
        &mut ctx.copyrel_relro
    } else {
        &mut ctx.copyrel
    };
    sec.symbols.push(id);
    let offset = align_to(sec.hdr.shdr.sh_size.get(), alignment);
    sec.hdr.shdr.sh_size.set(offset + size);
    let align = sec.hdr.shdr.sh_addralign.get().max(alignment);
    sec.hdr.shdr.sh_addralign.set(align);

    for alias in aliases {
        ctx.symbols.aux_mut(alias);
        let s = &mut ctx.symbols[alias];
        s.set_imported(true);
        s.set_exported(true);
        s.set_copyrel(true);
        s.set_copyrel_readonly(relro);
        s.value = offset;
        chunks::dynsym::add_symbol(ctx, alias);
    }
}

pub fn write_dynrels<E: Arch>(ctx: &Context<E>, sec: &CopyrelSection<E>, out: &mut [E::Rel]) {
    for (i, &id) in sec.symbols.iter().enumerate() {
        let sym = &ctx.symbols[id];
        out[i] = ElfRel::<E>::new(
            sym.addr(ctx),
            E::R_COPY,
            sym.dynsym_idx(&ctx.symbols).unwrap_or(0),
            0,
        );
    }
    debug_assert_eq!(sec.symbols.len(), out.len());
}
