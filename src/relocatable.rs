//! This file implements -r or --relocatable. That option forces the linker
//! to combine input object files into another single large object file.
//! Since the behavior of the linker when the option is given is quite
//! different from that of the normal execution mode, we separate code for
//! the feature into this separate file.
//!
//! The --relocatable option isn't used very often. After all, if you want
//! to combine object files into a single file, you could use `ar`.
//! However, some programs use it in a creative manner which is hard to be
//! substituted with static archives, so we need to support this option in
//! the same way as GNU ld does. A notable example is GHC (Glasgow Haskell
//! Compiler). GHC has its own dynamic linker which can load a .o file (as
//! opposed to a .so) into memory. GHC's module is not a shared object file
//! but a combined object file.
//!
//! There are many different ways to combine object files into a single file.
//! The simplest approach would be to just copy all sections from input files
//! to an output file as-is with a few exceptions for singleton sections such
//! as the symbol table or the string table. That works, but that's not
//! compatible with GNU ld.
//!
//! To be compatible with GNU ld, we need to do the followings:
//!
//!  - Regular sections containing opaque data (e.g. ".text" or ".data")
//!    are just copied as-is. Two sections with the same name are merged.
//!
//!  - .symtab, .strtab and .shstrtab are merged.
//!
//!  - COMDAT groups are uniquified.
//!
//!  - Relocations are copied, but we need to fix symbol indices.

use rayon::prelude::*;

use crate::arch::Arch;
use crate::context::Context;
use crate::elf::*;
use crate::input_files::FileId;
use crate::output_chunks::eh_frame::EhFrameRelocSection;
use crate::output_chunks::misc::{ComdatGroupSection, NotePropertySection, RiscvAttributesSection};
use crate::output_chunks::sframe::SFrameRelocSection;
use crate::output_chunks::symtab::ShstrtabSection;
use crate::output_chunks::{self, ChunkId, OutputEhdr, OutputShdr};
use crate::output_file::OutputFile;
use crate::passes;
use crate::util::align_to;

// Create linker-synthesized sections
fn create_synthetic_sections<E: Arch>(ctx: &mut Context<E>) {
    ctx.ehdr = Some(OutputEhdr::<E>::new(0));
    ctx.shdr = Some(OutputShdr::<E>::new());
    ctx.eh_frame_reloc = Some(EhFrameRelocSection::<E>::new());
    ctx.sframe_reloc = Some(SFrameRelocSection::<E>::new());
    ctx.shstrtab = Some(ShstrtabSection::new());
    ctx.chunks.extend([
        ChunkId::Ehdr,
        ChunkId::Shdr,
        ChunkId::EhFrame,
        ChunkId::EhFrameReloc,
        ChunkId::SFrame,
        ChunkId::SFrameReloc,
        ChunkId::Strtab,
        ChunkId::Symtab,
        ChunkId::Shstrtab,
    ]);
    if E::IS_X86 {
        ctx.note_property = Some(NotePropertySection::<E>::new());
        ctx.chunks.push(ChunkId::NoteProperty);
    }
    if E::IS_RISCV {
        ctx.riscv_attributes = Some(RiscvAttributesSection::new());
        ctx.chunks.push(ChunkId::RiscvAttributes);
    }
}

/// Create SHT_GROUP (i.e. comdat group) sections. We uniquify comdat
/// sections by signature. We want to propagate input comdat groups as
/// output comdat groups if they are still alive after uniquification.
fn create_comdat_group_sections<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("create_comdat_group_sections");
    let mut sections = Vec::new();
    for file in &ctx.objs {
        for group in &file.comdat_groups {
            if !group.is_owner() {
                continue;
            }
            let sym =
                file.base.symbols[file.base.shdrs[group.sect_idx as usize].sh_info.get() as usize];
            let mut members = Vec::new();
            for j in file.comdat_members(group) {
                let shdr = &file.base.shdrs[j as usize];
                let sh_type = shdr.sh_type.get();
                let is_reloc =
                    sh_type == if E::IS_RELA { SHT_RELA } else { SHT_REL } || sh_type == SHT_CREL;
                let target = if is_reloc { shdr.sh_info.get() } else { j };
                let Some(isec) = file.section(target as usize) else {
                    continue;
                };
                let Some(osec) = isec.output_section else {
                    continue;
                };
                if is_reloc {
                    if let Some(reloc) = ctx.output_sections[osec.index()].reloc_sec {
                        members.push(ChunkId::Reloc(reloc));
                    }
                } else {
                    members.push(ChunkId::Output(osec));
                }
            }
            sections.push(ComdatGroupSection::new(sym, members));
        }
    }
    for sec in sections {
        ctx.comdat_group_sections.push(sec);
        ctx.chunks.push(ChunkId::ComdatGroup(
            ctx.comdat_group_sections.len() as u32 - 1,
        ));
    }
}

/// Unresolved undefined symbols in the -r mode are simply propagated to an
/// output file as undefined symbols. This function guarantees that
/// unresolved undefined symbols belongs to some input file.
fn claim_unresolved_symbols<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("r_claim_unresolved_symbols");
    let candidates: Vec<(crate::input_files::ObjId, usize)> = ctx
        .objs
        .par_iter()
        .flat_map_iter(|file| {
            let file_id = file.id();
            (file.base.first_global..file.base.elf_syms.len())
                .filter(move |&i| file.base.elf_syms[i].is_undef())
                .map(move |i| (file_id, i))
        })
        .collect();
    for (obj_id, i) in candidates {
        let file = &ctx.objs[obj_id.index()];
        let id = file.base.symbols[i];
        let esym = &file.base.elf_syms[i];
        let priority = file.base.priority;
        let sym = &ctx.symbols[id];
        if let Some(owner) = sym.file() {
            if !sym.is_undef() || ctx.file(owner).priority <= priority {
                continue;
            }
        }
        let sym = &mut ctx.symbols[id];
        sym.set_file(FileId::Obj(obj_id));
        sym.clear_origin();
        sym.value = 0;
        sym.sym_idx = i as u32;
        sym.set_esym(esym);
    }
}

/// Set output section in-file offsets. Output section memory addresses
/// are left as zero.
fn set_osec_offsets<E: Arch>(ctx: &mut Context<E>) -> u64 {
    let mut offset = 0;
    for id in ctx.chunks.clone() {
        let hdr = ctx.chunk_header_mut(id);
        offset = align_to(offset, hdr.shdr.sh_addralign.get());
        hdr.shdr.sh_offset.set(offset);
        offset += hdr.shdr.sh_size.get();
    }
    offset
}

pub fn combine_objects<E: Arch>(ctx: &mut Context<E>) {
    passes::create_output_sections(ctx);
    create_synthetic_sections(ctx);
    claim_unresolved_symbols(ctx);
    passes::compute_section_sizes(ctx);
    passes::sort_output_sections(ctx);
    passes::create_output_symtab(ctx);
    output_chunks::eh_frame::construct(ctx);
    output_chunks::sframe::construct(ctx);
    passes::create_reloc_sections(ctx);
    create_comdat_group_sections(ctx);
    passes::compute_section_headers(ctx);

    let filesize = set_osec_offsets(ctx);
    let mut output = OutputFile::open(&ctx.args.output, filesize, 0o666, false);
    crate::driver::copy_chunks(ctx, output.buf());
    output.close();
    crate::error::checkpoint();

    if ctx.args.print_map {
        crate::mapfile::print_map(ctx);
    }
    if ctx.args.stats {
        passes::show_stats(ctx);
    }
    if ctx.args.perf {
        ctx.timers.print();
    }
    if ctx.args.quick_exit {
        crate::error::exit_after_cleanup(0);
    }
}
