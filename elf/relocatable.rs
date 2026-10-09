//! This file implements -r or --relocatable. That option makes the linker
//! combine input object files into another single large object file.
//! Since the behavior of the linker when the option is given is quite
//! different from that of the normal execution mode, we put the code for
//! the feature in this separate file.
//!
//! The --relocatable option isn't used very often. After all, if you want
//! to combine object files into a single file, you could use `ar`.
//! However, some programs use it in a creative manner that is hard to
//! replace with static archives, so we need to support this option in
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
//! To be compatible with GNU ld, we need to do the following:
//!
//!  - Regular sections containing opaque data (e.g. ".text" or ".data")
//!    are just copied as-is. Two sections with the same name are merged.
//!
//!  - .symtab, .strtab and .shstrtab are merged.
//!
//!  - COMDAT groups are uniquified. Other section groups are preserved.
//!
//!  - Relocations are copied, but we need to fix symbol indices.

use std::collections::HashMap;

use mold_common::util::align_to;

use crate::arch::{Family, Target};
use crate::chunks::comdat_group::ComdatGroupSection;
use crate::chunks::note_property::NotePropertySection;
use crate::chunks::output_section::{self, OutputSection};
use crate::chunks::riscv_attributes::RiscvAttributesSection;
use crate::chunks::{self, ChunkId, OutputSectionId};
use crate::context::Context;
use crate::elf::*;
use crate::input_files::FileId;
use crate::input_sections::InputSectionId;
use crate::passes;

/// An output section's sh_link can refer to only one section, so
/// SHF_LINK_ORDER sections of the same name share an output section only
/// if the sections they are linked to do too.
fn split_link_order_sections<E: Target>(ctx: &mut Context<E>) {
    for i in 0..ctx.chunks.len() {
        let ChunkId::Output(id) = ctx.chunks[i] else {
            continue;
        };
        let osec = &ctx.output_sections[id.index()];
        if osec.hdr.shdr.sh_flags.get() & SHF_LINK_ORDER as u64 == 0 {
            continue;
        }

        let mut groups: Vec<Vec<InputSectionId>> = Vec::new();
        let mut group_of = HashMap::new();
        for &m in &osec.members {
            let link = output_section::link_order_target(ctx, m);
            let j = *group_of.entry(link).or_insert_with(|| {
                groups.push(Vec::new());
                groups.len() - 1
            });
            groups[j].push(m);
        }

        let (name, shdr) = (osec.hdr.name, osec.hdr.shdr);
        let mut groups = groups.into_iter();
        ctx.output_sections[id.index()].members = groups.next().unwrap();

        for members in groups {
            let id = OutputSectionId::new(ctx.output_sections.len() as u32);
            for &m in &members {
                let isec = ctx.input_section(m);
                let (file, shndx) = (isec.file, isec.shndx as usize);
                ctx.objs[file.index()].section_mut(shndx).unwrap().output_section = Some(id);
            }
            let mut osec = OutputSection::<E>::new(name, shdr.sh_type.get());
            osec.hdr.shdr.sh_flags = shdr.sh_flags;
            osec.hdr.shdr.sh_addralign = shdr.sh_addralign;
            osec.members = members;
            ctx.output_sections.push(osec);
            ctx.chunks.push(ChunkId::Output(id));
        }
    }
}

// Create linker-synthesized sections
fn create_synthetic_sections<E: Target>(ctx: &mut Context<E>) {
    ctx.ehdr = Some(chunks::new_ehdr::<E>(0));
    ctx.shdr = Some(chunks::new_shdr::<E>());
    ctx.eh_frame_reloc = Some(chunks::eh_frame_reloc::new_header::<E>());
    ctx.sframe_reloc = Some(chunks::sframe_reloc::new_header::<E>());
    ctx.shstrtab = Some(chunks::shstrtab::new_header());
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
    if E::IS_X86 || E::FAMILY == Family::Arm64 {
        ctx.note_property = Some(NotePropertySection::<E>::new());
        ctx.chunks.push(ChunkId::NoteProperty);
    }
    if E::IS_RISCV {
        ctx.riscv_attributes = Some(RiscvAttributesSection::new());
        ctx.chunks.push(ChunkId::RiscvAttributes);
    }
}

/// Create SHT_GROUP sections. We uniquify comdat sections by signature.
/// We want to propagate input comdat groups as output comdat groups if
/// they are still alive after uniquification. Groups whose flag word is 0
/// and GCC's .debug_macro groups are never uniquified, so all of them are
/// propagated.
fn create_comdat_group_sections<E: Target>(ctx: &mut Context<E>) {
    let _t = ctx.timer("create_comdat_group_sections");
    let mut sections = Vec::new();
    for file in &ctx.objs {
        let comdat_groups =
            file.comdat_groups.iter().filter(|g| g.is_owner()).map(|g| (g.sect_idx, GRP_COMDAT));
        let debug_macro_groups = file.debug_macro_groups.iter().map(|&i| (i, GRP_COMDAT));
        let non_comdat_groups = file.non_comdat_groups.iter().map(|&i| (i, 0));
        for (sect_idx, flags) in comdat_groups.chain(debug_macro_groups).chain(non_comdat_groups) {
            let sym = file.base.symbols[file.base.shdrs[sect_idx as usize].sh_info.get() as usize];
            let mut members = Vec::new();
            for j in file.group_members(sect_idx) {
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
            // All members may have been discarded, e.g. by --strip-debug.
            // An empty group section is rejected by other tools, so drop it.
            if !members.is_empty() {
                sections.push(ComdatGroupSection::new(sym, flags, members));
            }
        }
    }
    for sec in sections {
        ctx.comdat_group_sections.push(sec);
        ctx.chunks.push(ChunkId::ComdatGroup(ctx.comdat_group_sections.len() as u32 - 1));
    }
}

/// Unresolved undefined symbols in the -r mode are simply propagated to an
/// output file as undefined symbols. This function guarantees that
/// unresolved undefined symbols belong to some input file.
fn claim_unresolved_symbols<E: Target>(ctx: &mut Context<E>) {
    let _t = ctx.timer("r_claim_unresolved_symbols");
    for file in &ctx.objs {
        let priority = file.base.priority;
        for i in file.base.first_global..file.base.elf_syms.len() {
            let esym = &file.base.elf_syms[i];
            if !esym.is_undef() {
                continue;
            }
            let id = file.base.symbols[i];
            let sym = &ctx.symbols[id];
            if let Some(owner) = sym.file()
                && (!sym.is_undef() || ctx.file(owner).priority <= priority)
            {
                continue;
            }
            let sym = &mut ctx.symbols[id];
            sym.set_file(FileId::Obj(file.id()));
            sym.clear_origin();
            sym.value = 0;
            sym.set_sym_idx(i as u32);
            sym.set_esym(esym);
        }
    }
}

/// Set output section in-file offsets. Output section memory addresses
/// are left as zero.
fn set_osec_offsets<E: Target>(ctx: &mut Context<E>) -> u64 {
    let mut offset = 0;
    for i in 0..ctx.chunks.len() {
        let id = ctx.chunks[i];
        let hdr = ctx.chunk_header_mut(id);
        offset = align_to(offset, hdr.shdr.sh_addralign.get());
        hdr.shdr.sh_offset.set(offset);
        offset += hdr.shdr.sh_size.get();
    }
    offset
}

pub fn combine_objects<E: Target>(ctx: &mut Context<E>) {
    passes::create_output_sections(ctx);
    split_link_order_sections(ctx);
    create_synthetic_sections(ctx);
    claim_unresolved_symbols(ctx);
    if ctx.note_property.is_some() {
        chunks::note_property::construct(ctx);
    }
    passes::compute_section_sizes(ctx);
    passes::sort_output_sections(ctx);
    passes::create_output_symtab(ctx);
    chunks::eh_frame::construct(ctx);
    chunks::sframe::construct(ctx);
    passes::create_reloc_sections(ctx);
    create_comdat_group_sections(ctx);
    passes::compute_section_headers(ctx);

    let filesize = set_osec_offsets(ctx);
    let mut output = crate::driver::open_output_file(&ctx.args, filesize, 0o666, false);
    crate::driver::copy_chunks(ctx, output.buf());
    output.close();
    mold_common::error::checkpoint();

    if let Some(output) = &ctx.args.map {
        crate::mapfile::print_map(ctx, output);
    }
    if ctx.args.stats {
        passes::show_stats(ctx);
    }
    if ctx.args.perf {
        ctx.timers.print(&mut std::io::stdout());
    }
    if ctx.args.quick_exit {
        mold_common::error::exit_after_cleanup(0);
    }
}
