//! `.riscv.attributes`, merged RISC-V target attributes.

use crate::arch::Arch;
use crate::context::Context;
use crate::elf::*;
use crate::output_chunks::ChunkHeader;

/// `.riscv.attributes` describes the ISA the output requires, merged
/// from the input files' attributes.
#[derive(Debug)]
pub struct RiscvAttributesSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
    pub contents: Vec<u8>,
}

impl<E: Layout> RiscvAttributesSection<E> {
    pub fn new() -> RiscvAttributesSection<E> {
        RiscvAttributesSection {
            hdr: ChunkHeader::<E>::new(".riscv.attributes", SHT_RISCV_ATTRIBUTES, 0),
            contents: Vec::new(),
        }
    }
}

impl<E: Layout> Default for RiscvAttributesSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
    if !ctx.riscv_attributes.as_ref().unwrap().contents.is_empty() {
        return;
    }
    let contents = crate::arch::riscv::attributes_contents(ctx);
    let sec = ctx.riscv_attributes.as_mut().unwrap();
    sec.hdr.shdr.sh_size.set(contents.len() as u64);
    sec.contents = contents;
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    let sec = ctx.riscv_attributes.as_ref().unwrap();
    buf[..sec.contents.len()].copy_from_slice(&sec.contents);
}
