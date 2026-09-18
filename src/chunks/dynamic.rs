//! `.dynamic`, information consumed by the dynamic linker.

use crate::arch::{Arch, Family};
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::input_files::FileId;
use crate::symbol::SymbolId;

// .dynamic contains various information for dynamically-linked ELF files.
// At runtime, the dynamic linker reads the information to work
// appropriately.
pub fn new_header<E: Arch>(args: &crate::cmdline::Args) -> ChunkHeader<E> {
    let mut hdr = ChunkHeader::<E>::new(".dynamic", SHT_DYNAMIC, 0);
    hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
    hdr.shdr.sh_entsize.set(ElfDyn::<E>::size() as u64);
    if args.z_rodynamic {
        hdr.shdr.sh_flags.set(SHF_ALLOC as u64);
        hdr.is_relro = false;
    } else {
        hdr.shdr.sh_flags.set((SHF_ALLOC | SHF_WRITE) as u64);
        hdr.is_relro = true;
    }
    hdr
}

// An ARM64 function with a non-standard calling convention is marked with
// STO_AARCH64_VARIANT_PCS bit in the symbol table.
//
// A function with that bit is not safe to be called through a lazy PLT
// stub because the PLT resolver may clobber registers that should be
// preserved in a non-standard calling convention.
//
// To solve the problem, the dynamic linker scans the dynamic symbol table
// at process startup time and resolve symbols with STO_AARCH64_VARIANT_PCS
// bit eagerly, so that the PLT resolver won't be called for that symbol
// lazily. As an optimization, it does so only when DT_AARCH64_VARIANT_PCS
// is set in the dynamic section.
//
// This function returns true if DT_AARCH64_VARIANT_PCS needs to be set.
fn contains_variant_pcs<E: Arch>(ctx: &Context<E>) -> bool {
    ctx.plt.symbols.iter().any(|&id| ctx.symbols[id].esym(ctx).arm64_variant_pcs())
}

fn sym_addr_if_defined<E: Arch>(ctx: &Context<E>, id: SymbolId) -> Option<u64> {
    let sym = &ctx.symbols[id];
    match sym.file() {
        Some(FileId::Obj(_)) => Some(sym.addr(ctx)),
        _ => None,
    }
}

fn for_each_entry<E: Arch>(ctx: &Context<E>, mut define: impl FnMut(u32, u64)) {
    let dynstr = &ctx.dynstr;
    let plt = &ctx.plt;
    let (rel, relsz, relent) =
        if E::IS_RELA { (DT_RELA, DT_RELASZ, DT_RELAENT) } else { (DT_REL, DT_RELSZ, DT_RELENT) };

    for dso in &ctx.dsos {
        define(DT_NEEDED, dynstr.find_string(dso.soname));
    }
    if !ctx.args.rpaths.is_empty() {
        let tag = if ctx.args.enable_new_dtags { DT_RUNPATH } else { DT_RPATH };
        define(tag, dynstr.find_string(ctx.args.rpaths.as_encoded_bytes()));
    }
    if !ctx.args.soname.is_empty() {
        define(DT_SONAME, dynstr.find_string(ctx.args.soname.as_encoded_bytes()));
    }
    for s in &ctx.args.auxiliary {
        define(DT_AUXILIARY, dynstr.find_string(s));
    }
    if !ctx.args.audit.is_empty() {
        define(DT_AUDIT, dynstr.find_string(&ctx.args.audit));
    }
    if !ctx.args.depaudit.is_empty() {
        define(DT_DEPAUDIT, dynstr.find_string(&ctx.args.depaudit));
    }
    for s in &ctx.args.filter {
        define(DT_FILTER, dynstr.find_string(s));
    }

    if ctx.reldyn.hdr.shdr.sh_size.get() != 0 {
        if ctx.args.pack_dyn_relocs_android {
            let (rel, relsz) = if E::IS_RELA {
                (DT_ANDROID_RELA, DT_ANDROID_RELASZ)
            } else {
                (DT_ANDROID_REL, DT_ANDROID_RELSZ)
            };
            define(rel, ctx.reldyn.hdr.shdr.sh_addr.get());
            define(relsz, ctx.reldyn.hdr.shdr.sh_size.get());
        } else {
            define(rel, ctx.reldyn.hdr.shdr.sh_addr.get());
            define(relsz, ctx.reldyn.hdr.shdr.sh_size.get());
            define(relent, std::mem::size_of::<ElfRel<E>>() as u64);
        }
    }

    if let Some(relrdyn) = &ctx.relrdyn {
        let (relr, relrsz, relrent) = if ctx.args.use_android_relr_tags {
            (DT_ANDROID_RELR, DT_ANDROID_RELRSZ, DT_ANDROID_RELRENT)
        } else {
            (DT_RELR, DT_RELRSZ, DT_RELRENT)
        };
        define(relr, relrdyn.shdr.sh_addr.get());
        define(relrsz, relrdyn.shdr.sh_size.get());
        define(relrent, relrdyn.shdr.sh_entsize.get());
    }

    if ctx.relplt.shdr.sh_size.get() != 0 {
        define(DT_JMPREL, ctx.relplt.shdr.sh_addr.get());
        define(DT_PLTRELSZ, ctx.relplt.shdr.sh_size.get());
        define(DT_PLTREL, rel as u64);
    }

    if E::IS_SPARC {
        if plt.hdr.shdr.sh_size.get() != 0 {
            define(DT_PLTGOT, plt.hdr.shdr.sh_addr.get());
        }
    } else if E::FAMILY == Family::Ppc32 {
        if ctx.gotplt.shdr.sh_size.get() != 0 {
            define(
                DT_PLTGOT,
                ctx.gotplt.shdr.sh_addr.get() + crate::chunks::gotplt::header_size::<E>(),
            );
        }
    } else if ctx.gotplt.shdr.sh_size.get() != 0 {
        define(DT_PLTGOT, ctx.gotplt.shdr.sh_addr.get());
    }

    if ctx.dynsym.hdr.shdr.sh_size.get() != 0 {
        define(DT_SYMTAB, ctx.dynsym.hdr.shdr.sh_addr.get());
        define(DT_SYMENT, std::mem::size_of::<ElfSym<E>>() as u64);
    }
    if ctx.dynstr.hdr.shdr.sh_size.get() != 0 {
        define(DT_STRTAB, ctx.dynstr.hdr.shdr.sh_addr.get());
        define(DT_STRSZ, ctx.dynstr.hdr.shdr.sh_size.get());
    }

    let value = |id: Option<SymbolId>| id.map_or(0, |id| ctx.symbols[id].value);
    if ctx.find_chunk_by_type(SHT_INIT_ARRAY).is_some() {
        let start = value(ctx.syms.init_array_start);
        define(DT_INIT_ARRAY, start);
        define(DT_INIT_ARRAYSZ, value(ctx.syms.init_array_end) - start);
    }
    if ctx.find_chunk_by_type(SHT_PREINIT_ARRAY).is_some() {
        let start = value(ctx.syms.preinit_array_start);
        define(DT_PREINIT_ARRAY, start);
        define(DT_PREINIT_ARRAYSZ, value(ctx.syms.preinit_array_end) - start);
    }
    if ctx.find_chunk_by_type(SHT_FINI_ARRAY).is_some() {
        let start = value(ctx.syms.fini_array_start);
        define(DT_FINI_ARRAY, start);
        define(DT_FINI_ARRAYSZ, value(ctx.syms.fini_array_end) - start);
    }

    if ctx.versym.hdr.shdr.sh_size.get() != 0 {
        define(DT_VERSYM, ctx.versym.hdr.shdr.sh_addr.get());
    }
    if ctx.verneed.hdr.shdr.sh_size.get() != 0 {
        define(DT_VERNEED, ctx.verneed.hdr.shdr.sh_addr.get());
        define(DT_VERNEEDNUM, ctx.verneed.hdr.shdr.sh_info.get() as u64);
    }
    if let Some(verdef) = &ctx.verdef {
        define(DT_VERDEF, verdef.hdr.shdr.sh_addr.get());
        define(DT_VERDEFNUM, verdef.hdr.shdr.sh_info.get() as u64);
    }

    if let Some(addr) = sym_addr_if_defined(ctx, ctx.syms.init) {
        define(DT_INIT, addr);
    }
    if let Some(addr) = sym_addr_if_defined(ctx, ctx.syms.fini) {
        define(DT_FINI, addr);
    }

    if let Some(hash) = &ctx.hash {
        define(DT_HASH, hash.shdr.sh_addr.get());
    }
    if let Some(gnu_hash) = &ctx.gnu_hash {
        define(DT_GNU_HASH, gnu_hash.hdr.shdr.sh_addr.get());
    }
    let has_textrel = ctx.has_textrel.load(std::sync::atomic::Ordering::Relaxed);
    if has_textrel {
        define(DT_TEXTREL, 0);
    }

    let mut flags = 0u64;
    let mut flags1 = 0u64;
    if ctx.args.pie {
        flags1 |= DF_1_PIE as u64;
    }
    if ctx.args.z_now {
        flags |= DF_BIND_NOW as u64;
        flags1 |= DF_1_NOW as u64;
    }
    if ctx.args.z_origin {
        flags |= DF_ORIGIN as u64;
        flags1 |= DF_1_ORIGIN as u64;
    }
    if !ctx.args.z_dlopen {
        flags1 |= DF_1_NOOPEN as u64;
    }
    if ctx.args.z_nodefaultlib {
        flags1 |= DF_1_NODEFLIB as u64;
    }
    if !ctx.args.z_delete {
        flags1 |= DF_1_NODELETE as u64;
    }
    if !ctx.args.z_dump {
        flags1 |= DF_1_NODUMP as u64;
    }
    if ctx.args.z_initfirst {
        flags1 |= DF_1_INITFIRST as u64;
    }
    if ctx.args.z_interpose {
        flags1 |= DF_1_INTERPOSE as u64;
    }
    if !ctx.got.gottp_syms.is_empty() {
        flags |= DF_STATIC_TLS as u64;
    }
    if has_textrel {
        flags |= DF_TEXTREL as u64;
    }
    if flags != 0 {
        define(DT_FLAGS, flags);
    }
    if flags1 != 0 {
        define(DT_FLAGS_1, flags1);
    }

    if E::FAMILY == Family::Arm64 && contains_variant_pcs(ctx) {
        define(DT_AARCH64_VARIANT_PCS, 0);
    }
    // RISC-V has the same feature but with a different name.
    if E::IS_RISCV && plt.symbols.iter().any(|&id| ctx.symbols[id].esym(ctx).riscv_variant_cc()) {
        define(DT_RISCV_VARIANT_CC, 0);
    }
    if E::FAMILY == Family::Ppc32 {
        define(DT_PPC_GOT, ctx.gotplt.shdr.sh_addr.get());
    }
    if E::IS_PPC64 {
        // PPC64_GLINK is defined by the psABI to refer to 32 bytes before
        // the first PLT entry. I don't know why it's 32 bytes off, but
        // it's what it is.
        define(
            DT_PPC64_GLINK,
            plt.hdr.shdr.sh_addr.get() + crate::chunks::plt::entry_offset::<E>(0) - 32,
        );
    }

    // GDB needs a DT_DEBUG entry in an executable to store a word-size
    // data for its own purpose. Its content is not important.
    if !ctx.args.shared && !ctx.args.z_rodynamic {
        define(DT_DEBUG, 0);
    }

    define(DT_NULL, 0);
    for _ in 0..ctx.args.spare_dynamic_tags.max(0) {
        define(DT_NULL, 0);
    }
}

pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
    if ctx.args.is_static && !ctx.args.pie {
        return;
    }
    let mut n = 0;
    for_each_entry(ctx, |_, _| n += 1);
    let size = (n * ElfDyn::<E>::size()) as u64;
    let dynamic = ctx.dynamic.as_mut().unwrap();
    dynamic.shdr.sh_size.set(size);
    dynamic.shdr.sh_link.set(ctx.dynstr.hdr.shndx);
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    debug_assert_eq!(ctx.dynamic.as_ref().unwrap().shdr.sh_size.get() as usize, buf.len());
    let mut slots = buf.chunks_exact_mut(ElfDyn::<E>::size());
    for_each_entry(ctx, |d_tag, d_val| {
        let entry = ElfDyn::<E> { d_tag: E::Word::new(d_tag as u64), d_val: E::Word::new(d_val) };
        entry.write(slots.next().unwrap());
    });
    debug_assert!(slots.next().is_none());
}
