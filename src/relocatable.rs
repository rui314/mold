//! `-r` / `--relocatable`: combining object files into one object file.
//!
//! Regular sections are copied and merged by name, the symbol and string
//! tables are merged, COMDAT groups are uniquified, and relocations are
//! copied with their symbol indices rewritten. This matches what GNU ld
//! produces, which some programs (notably GHC's runtime linker) depend on.

use rayon::prelude::*;

use crate::arch::Arch;
use crate::chunks::eh_frame::EhFrameRelocSection;
use crate::chunks::misc::{ComdatGroupSection, NotePropertySection};
use crate::chunks::sframe::SFrameRelocSection;
use crate::chunks::symtab::ShstrtabSection;
use crate::chunks::{self, ChunkId, OutputEhdr, OutputShdr};
use crate::context::Context;
use crate::elf::*;
use crate::input_files::FileId;
use crate::output_file::OutputFile;
use crate::passes;
use crate::util::align_to;

fn create_synthetic_sections<E: Arch>(ctx: &mut Context<E>) {
    ctx.ehdr = Some(OutputEhdr::new::<E>(0));
    ctx.shdr = Some(OutputShdr::new::<E>());
    ctx.eh_frame_reloc = Some(EhFrameRelocSection::new::<E>());
    ctx.sframe_reloc = Some(SFrameRelocSection::new::<E>());
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
        ctx.note_property = Some(NotePropertySection::new::<E>());
        ctx.chunks.push(ChunkId::NoteProperty);
    }
}

/// Propagates input COMDAT groups that survived uniquification as output
/// groups.
fn create_comdat_group_sections<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("create_comdat_group_sections");
    let mut sections = Vec::new();
    for file in &ctx.objs {
        for group in &file.comdat_groups {
            if !group.is_owner() {
                continue;
            }
            let sym =
                file.base.symbols[file.base.shdrs.at(group.sect_idx as usize).sh_info as usize];
            let mut members = Vec::new();
            for j in file.comdat_members(group) {
                let shdr = &file.base.shdrs.at(j as usize);
                let is_reloc = shdr.sh_type == if E::IS_RELA { SHT_RELA } else { SHT_REL }
                    || shdr.sh_type == SHT_CREL;
                let target = if is_reloc { shdr.sh_info } else { j };
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

/// Unresolved symbols are propagated to the output as undefined symbols
/// belonging to some input file.
fn claim_unresolved_symbols<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("r_claim_unresolved_symbols");
    let candidates: Vec<(usize, usize)> = ctx
        .objs
        .par_iter()
        .enumerate()
        .flat_map_iter(|(fi, file)| {
            (file.base.first_global..file.base.elf_syms.len())
                .filter(move |&i| file.base.elf_syms.at(i).is_undef())
                .map(move |i| (fi, i))
        })
        .collect();
    for (fi, i) in candidates {
        let file = &ctx.objs[fi];
        let id = file.base.symbols[i];
        let esym = file.base.elf_syms.at(i);
        let priority = file.base.priority;
        let sym = &ctx.symbols[id];
        if let Some(owner) = sym.file() {
            if !sym.is_undef() || ctx.file(owner).priority <= priority {
                continue;
            }
        }
        let sym = &mut ctx.symbols[id];
        sym.set_file(FileId::Obj(crate::input_files::ObjId(fi as u32)));
        sym.clear_origin();
        sym.value = 0;
        sym.sym_idx = i as u32;
        sym.set_esym(&esym);
    }
}

/// Assigns file offsets; addresses stay zero.
fn set_osec_offsets<E: Arch>(ctx: &mut Context<E>) -> u64 {
    let mut offset = 0;
    for id in ctx.chunks.clone() {
        let hdr = ctx.chunk_header_mut(id);
        offset = align_to(offset, hdr.shdr.sh_addralign);
        hdr.shdr.sh_offset = offset;
        offset += hdr.shdr.sh_size;
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
    chunks::eh_frame::construct(ctx);
    chunks::sframe::construct(ctx);
    passes::create_reloc_sections(ctx);
    create_comdat_group_sections(ctx);
    passes::compute_section_headers(ctx);

    let filesize = set_osec_offsets(ctx);
    let mut output = OutputFile::open(&ctx.diag, &ctx.args.output, filesize, 0o666, false);
    crate::driver::copy_chunks(ctx, output.buf());
    output.close(&ctx.diag);
    ctx.checkpoint();

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
        crate::diagnostics::exit_after_cleanup(0);
    }
}
