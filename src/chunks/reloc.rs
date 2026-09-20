//! Relocation tables retained by `-r` and `--emit-relocs`.

use bstr::BStr;

use crate::chunks::{ChunkHeader, OutputSectionId};
use crate::context::Context;
use crate::elf::*;
use crate::input_sections::{FragmentLookup, InputSection, r_delta};
use crate::symbol::{OriginValue, Symbol};
use crate::target::Target;

// RelocSection represents a relocation table for an output file.
// These tables are emitted for `-r` and for final links with `--emit-relocs`.
#[derive(Debug)]
pub struct RelocSection<E: Target> {
    pub hdr: ChunkHeader<E>,
    pub output_section: OutputSectionId,
    /// The index of the first relocation of each member.
    offsets: Vec<u64>,
}

pub fn new<E: Target>(ctx: &Context<E>, osec_id: OutputSectionId) -> RelocSection<E> {
    let osec = &ctx.output_sections[osec_id.index()];
    let (prefix, ty) =
        if E::IS_RELA { (b".rela".as_slice(), SHT_RELA) } else { (b".rel".as_slice(), SHT_REL) };
    let name = BStr::new(crate::util::leak_bytes([prefix, osec.hdr.name.as_ref()].concat()));
    let mut hdr = ChunkHeader::<E>::with_name(name, ty, SHF_INFO_LINK as u64);
    hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
    let entsize = std::mem::size_of::<ElfRel<E>>() as u64;
    hdr.shdr.sh_entsize.set(entsize);

    // Compute an offset for each input section
    let mut offsets = Vec::with_capacity(osec.members.len());
    let mut sum = 0u64;
    for &m in &osec.members {
        offsets.push(sum);
        let isec = ctx.input_section(m);
        let file = &ctx.objs[isec.file.index()];
        sum += isec.rels(file).len() as u64;
    }
    hdr.shdr.sh_size.set(sum * entsize);
    RelocSection { hdr, output_section: osec_id, offsets }
}

pub fn update_shdr<E: Target>(ctx: &mut Context<E>, i: u32) {
    let symtab_shndx = ctx.symtab.shndx;
    let osec = ctx.reloc_sections[i as usize].output_section;
    let osec_shndx = ctx.output_sections[osec.index()].hdr.shndx;
    let sec = &mut ctx.reloc_sections[i as usize];
    sec.hdr.shdr.sh_link.set(symtab_shndx);
    sec.hdr.shdr.sh_info.set(osec_shndx);
}

/// Maps a symbol and addend to the output symbol table. Input section symbols
/// are replaced by output section symbols, with their offsets in the addend.
/// A section without an output section has no surviving symbol reference.
#[inline]
pub fn output_symidx_addend<E: Target>(
    ctx: &Context<E>,
    sym: &Symbol,
    addend: impl FnOnce() -> i64,
) -> Option<(u32, i64)> {
    if sym.st_type() == STT_SECTION {
        let target = sym.input_section_ref(ctx)?;
        Some((
            ctx.output_section(target.output_section?).hdr.shndx,
            addend() + target.offset() as i64,
        ))
    } else {
        Some((sym.output_sym_idx(ctx), addend()))
    }
}

// Translates an input relocation's symbol reference into the {r_sym, addend}
// pair that is valid in the output file. The returned r_sym is either an output
// section index (for section-relative relocs) or an output symbol table index.
fn symidx_addend<'a, E: Target>(
    ctx: &'a Context<E>,
    isec: &InputSection<E>,
    rel: &ElfRel<E>,
    cache: &mut FragmentLookup<'a>,
) -> (u32, i64) {
    let file = &ctx.objs[isec.file.index()];
    let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];

    if !isec.is_alloc()
        && let Some((frag, addend)) = isec.fragment(ctx, rel, cache)
    {
        let msec = &ctx.merged_sections[frag.section.index()];
        return (msec.hdr.shndx, msec.fragments.get(frag.entry).offset() as i64 + addend);
    }

    if sym.st_type() == STT_SECTION {
        if let OriginValue::Fragment(frag) = sym.origin() {
            let msec = &ctx.merged_sections[frag.section.index()];
            return (
                msec.hdr.shndx,
                msec.fragments.get(frag.entry).offset() as i64
                    + sym.value as i64
                    + isec.rel_addend(rel),
            );
        }
    } else if !sym.write_to_symtab() {
        return (0, 0);
    }
    // A dead debug section can refer to a COMDAT-eliminated section.
    output_symidx_addend(ctx, sym, || isec.rel_addend(rel)).unwrap_or((0, 0))
}

/// Writes the relocations. With `-r` on a REL target, the addends are
/// written into the output section's bytes, passed as `osec_buf`.
pub fn copy_buf<E: Target>(ctx: &Context<E>, i: u32, buf: &mut [u8], osec_buf: Option<&mut [u8]>) {
    let sec = &ctx.reloc_sections[i as usize];
    let osec = &ctx.output_sections[sec.output_section.index()];
    let out = rels_from_bytes_mut::<E>(buf);
    let mut osec_buf = osec_buf;

    for (mi, &m) in osec.members.iter().enumerate() {
        let isec = ctx.input_section(m);
        let file = &ctx.objs[isec.file.index()];
        let base = sec.offsets[mi] as usize;
        let mut cache = FragmentLookup::default();
        for (j, rel) in isec.rels(file).iter().enumerate() {
            let (symidx, addend) = symidx_addend(ctx, isec, rel, &mut cache);
            let mut r_offset = osec.hdr.shdr.sh_addr.get() + isec.offset() + rel.r_offset();
            if E::IS_RISCV || E::IS_LOONGARCH {
                // On RISC-V and LoongArch, relaxation may have deleted instructions,
                // shifting this relocation's offset.
                r_offset -= r_delta(isec, rel.r_offset()) as u64;
            }

            // SH4 object files store addends in the relocated places rather
            // than in r_addend, and the relocation records we emit here are
            // meant to be consumed as if they were in an object file, so we
            // follow that convention.
            let out_addend = if E::FAMILY == crate::target::Family::Sh4 { 0 } else { addend };
            out[base + j] = ElfRel::<E>::new(r_offset, rel.r_type(), symidx, out_addend);

            if ctx.args.relocatable
                && let Some(osec_buf) = osec_buf.as_deref_mut()
            {
                let loc = (isec.offset() + rel.r_offset()) as usize;
                E::write_addend(&mut osec_buf[loc..], addend, rel);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmdline::Args;
    use crate::target::I386;

    #[test]
    fn discarded_section_does_not_read_implicit_addend() {
        let ctx = Context::<I386>::new(Args::default(), Vec::new());
        let mut sym = Symbol::new(BStr::new(b".text"));
        let mut esym = ElfSym::<I386>::default();
        esym.set_type(STT_SECTION);
        sym.set_esym(&esym);

        assert_eq!(output_symidx_addend(&ctx, &sym, || panic!("read discarded relocation")), None);
    }
}
