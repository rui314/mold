//! Since RISC instructions are generally up to 32 bits long, there's no
//! way to embed very large immediates into their branch instructions. For
//! example, RISC-V's JAL (jump and link) instruction can jump to only
//! within PC ± 1 MiB because its immediate is 21 bits long. If the
//! destination is further than that, we need to use two instructions
//! instead; the first instruction being AUIPC, which sets the upper 20
//! bits of a displacement to a register, and the second being JALR, which
//! specifies the lower 12 bits and the register. Combined, they specify a
//! 32-bit displacement, which is sufficient to support the medium code
//! model.
//!
//! However, always using two or more instructions for function calls is a
//! waste of time and space if the branch target is within a single
//! instruction's reach. There are two approaches to address this problem
//! as follows:
//!
//!  1. The compiler optimistically emits a single branch instruction for
//!     all function calls. The linker then checks if the branch target is
//!     reachable, and if not, redirects the branch to a linker-synthesized
//!     code sequence that uses two or more instructions to branch further.
//!     That linker-synthesized code is called a "thunk". All RISC psABIs
//!     except RISC-V and LoongArch take this approach.
//!
//!  2. The compiler pessimistically emits two instructions to branch
//!     anywhere in PC ± 2 GiB, and the linker rewrites them with a single
//!     instruction if the branch target is close enough. RISC-V and
//!     LoongArch take this approach.
//!
//! This file contains functions to support (2). For (1), see thunks.rs.
//!
//! With the presence of this code-shrinking relaxation, sections can no
//! longer be considered as atomic units. If we delete an instruction from
//! the middle of a section, the section contents after that point need to
//! be shifted by the size of the instruction. Symbol values and relocation
//! offsets have to be shifted too if they refer to bytes past the deleted
//! ones.
//!
//! In mold, we use `r_deltas` to memorize how many bytes have been shifted
//! for relocations. For symbols, we directly mutate their `value` member.
//!
//! RISC-V and LoongArch object files tend to have way more relocations
//! than those for other targets. This is because all branches, including
//! those that jump within the same section, are explicitly expressed with
//! relocations. Here is why we need them: all control-flow statements,
//! such as `if` or `for`, are implemented using branch instructions. For
//! other targets, the compiler doesn't emit relocations for such branches
//! because it knows at compile-time exactly how many bytes have to be
//! skipped. That's not true in RISC-V and LoongArch because the linker may
//! delete bytes between a branch and its target. Therefore, all branches,
//! including in-section ones, have to be explicitly expressed with
//! relocations.
//!
//! Note that this mechanism only shrinks sections and never enlarges them,
//! as the compiler always emits the longest instruction sequence. This
//! makes the linker implementation a bit simpler because we don't need to
//! worry about oscillation.


use rayon::prelude::*;

use crate::arch::Arch;
use crate::chunks::{self, ChunkId};
use crate::context::Context;
use crate::elf::*;
use crate::input_files::{FileId, SymbolEditor};
use crate::input_sections::{r_delta, InputSection, RelocDelta};
use crate::symbol::Symbol;

/// Returns the distance between a relocated place and a symbol.
pub fn compute_distance<E: Arch>(
    ctx: &Context<E>,
    sym: &Symbol,
    isec: &InputSection<E>,
    rel: &ElfRel<E>,
) -> i64 {
    // We handle absolute symbols as if they were infinitely far away
    // because `shrink_section` may increase a distance between a branch
    // instruction and an absolute symbol. Branching to an absolute
    // location is extremely rare in real code, though.
    if sym.is_absolute() {
        return i64::MAX;
    }
    // Compute a distance between the relocated place and the symbol.
    let s = sym.addr(ctx) as i64;
    let p = (isec.addr(ctx) + rel.r_offset()) as i64;
    s.wrapping_add(rel.r_addend()).wrapping_sub(p)
}

/// Find all relaxable relocations and record how many bytes we can save
/// into r_deltas.
///
/// Technically speaking, relaxing relocations may allow more relocations
/// to be relaxed because the distance between a branch instruction and
/// its target may decrease as a result of relaxation. That said, the
/// number of such relocations is negligible (I tried to self-host mold
/// on RISC-V as an experiment and found that the mold-built .text is
/// only ~0.04% larger than that of GNU ld), so we don't bother to handle
/// them. We scan relocations only once here.
pub fn shrink_sections<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("shrink_sections");

    let shrunk: Vec<Vec<(u32, Vec<RelocDelta>)>> = {
        let ctx: &Context<E> = ctx;
        ctx.objs
            .par_iter()
            .map(|file| {
                file.input_sections()
                    .filter(|isec| isec.is_alive() && isec.sh_flags & SHF_EXECINSTR as u64 != 0)
                    .filter_map(|isec| {
                        let deltas = E::shrink_section(ctx, isec);
                        (!deltas.is_empty()).then_some((isec.shndx, deltas))
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    let Context { objs, .. } = ctx;
    objs.par_iter_mut().zip(shrunk).for_each(|(file, shrunk)| {
        for (shndx, deltas) in shrunk {
            let isec = file
                .section_mut(shndx as usize)
                .expect("no such input section");
            isec.sh_size -= deltas.last().unwrap().delta as u64;
            isec.set_r_deltas(deltas.into_boxed_slice());
        }
    });

    // Fix symbol values.
    {
        let Context { objs, symbols, .. } = ctx;
        let editor = SymbolEditor::new(symbols.as_mut_slice());
        objs.par_iter().for_each(|file| {
            let file_id = FileId::Obj(file.id());
            for &id in &file.base.symbols {
                editor.with_symbol(id, |sym| {
                    if sym.file() != Some(file_id) {
                        return;
                    }
                    let Some(section) = sym.input_section() else {
                        return;
                    };
                    debug_assert_eq!(section.file(), file.id());
                    let isec = file.sections.input(section.index());
                    debug_assert_eq!(isec.file, file.id());
                    if isec.sh_flags & SHF_EXECINSTR as u64 == 0 {
                        return;
                    }
                    let delta = r_delta(isec, sym.value);
                    if delta != 0 {
                        sym.value -= delta as u64;
                    }
                });
            }
        });
    }

    // Recompute sizes of executable sections
    let sizes: Vec<_> = {
        let ctx: &Context<E> = ctx;
        ctx.chunks
            .par_iter()
            .filter_map(|&id| match id {
                ChunkId::Output(osec)
                    if ctx.output_sections[osec.index()].hdr.shdr.sh_flags.get()
                        & SHF_EXECINSTR as u64
                        != 0 =>
                {
                    Some((osec, chunks::output_section::layout(ctx, osec)))
                }
                _ => None,
            })
            .collect()
    };
    for (osec, size) in sizes {
        ctx.output_sections[osec.index()].hdr.shdr.sh_size.set(size);
    }
}
