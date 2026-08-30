//! Linker relaxation that shrinks sections, for RISC-V and LoongArch.
//!
//! RISC instructions have room for only small immediates, so a branch
//! that may need to reach far is emitted as an instruction pair: RISC-V's
//! AUIPC+JALR reaches ±2 GiB where JAL alone reaches ±1 MiB. Most targets
//! have the compiler emit the short form and let the linker redirect
//! out-of-range branches through thunks. RISC-V and LoongArch do the
//! opposite: the compiler emits the long form, and the linker replaces it
//! with a shorter one when the target turns out to be close enough.
//!
//! Deleting an instruction from the middle of a section shifts everything
//! after it, so sections are no longer copied as a unit: `r_deltas`
//! records how far the bytes at each point moved, relocation offsets are
//! adjusted through it, and symbol values are adjusted in place. Sections
//! only ever shrink, so there is no oscillation to worry about.
//!
//! Since even in-section branches may cross deleted bytes, all branches
//! of these targets are expressed with relocations, which is why their
//! object files have so many.

use rayon::prelude::*;

use crate::arch::Arch;
use crate::chunks::{self, ChunkId};
use crate::context::Context;
use crate::elf::*;
use crate::input_sections::{r_delta, InputSection, RelocDelta, SectionRef};
use crate::symbol::Symbol;

/// The distance between a relocated place and a symbol.
pub fn compute_distance<E: Arch>(
    ctx: &Context<E>,
    sym: &Symbol,
    isec: &InputSection,
    rel: &ElfRel,
) -> i64 {
    // An absolute symbol counts as infinitely far away, since shrinking
    // can increase the distance to it. Branching to an absolute address
    // is extremely rare in real code anyway.
    if sym.is_absolute() {
        return i64::MAX;
    }
    let s = sym.addr(ctx) as i64;
    let p = (isec.addr(ctx) + rel.r_offset) as i64;
    s.wrapping_add(rel.r_addend).wrapping_sub(p)
}

/// Finds the relaxable relocations of all executable sections, records
/// the bytes they save, and shrinks the sections accordingly.
///
/// Relaxing one branch may bring another within reach, but such cases
/// are negligible (mold's own RISC-V build ends up ~0.04% larger than GNU
/// ld's), so relocations are scanned only once.
pub fn shrink_sections<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("shrink_sections");

    let shrunk: Vec<(SectionRef, Vec<RelocDelta>)> = {
        let ctx: &Context<E> = ctx;
        ctx.objs
            .par_iter()
            .flat_map_iter(|file| {
                file.input_sections()
                    .filter(|isec| isec.is_alive() && isec.sh_flags & SHF_EXECINSTR as u64 != 0)
                    .filter_map(|isec| {
                        let deltas = E::shrink_section(ctx, isec);
                        (!deltas.is_empty()).then_some((
                            SectionRef {
                                file: isec.file,
                                shndx: isec.shndx,
                            },
                            deltas,
                        ))
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    for (r, deltas) in shrunk {
        let isec = ctx.section_mut(r);
        isec.sh_size -= deltas.last().unwrap().delta as u64;
        ctx.set_r_deltas(r, deltas.into_boxed_slice());
    }

    // Symbols past removed bytes move with their sections.
    let obj_ids: Vec<_> = ctx.objs.iter().map(crate::input_files::ObjectFile::id).collect();
    for obj_id in obj_ids {
        let fi = obj_id.index();
        let file_id = crate::input_files::FileId::Obj(obj_id);
        for i in 0..ctx.objs[fi].base.symbols.len() {
            let id = ctx.objs[fi].base.symbols[i];
            let sym = &ctx.symbols[id];
            if sym.file() != Some(file_id) {
                continue;
            }
            let Some(isec) = sym.input_section_ref() else {
                continue;
            };
            if isec.sh_flags & SHF_EXECINSTR as u64 == 0 {
                continue;
            }
            let delta = r_delta(isec, sym.value);
            if delta != 0 {
                ctx.symbols[id].value -= delta as u64;
            }
        }
    }

    for id in ctx.chunks.clone() {
        if let ChunkId::Output(osec) = id {
            if ctx.output_sections[osec.index()].hdr.shdr.sh_flags & SHF_EXECINSTR as u64 != 0 {
                chunks::compute_section_size(ctx, id);
            }
        }
    }
}
