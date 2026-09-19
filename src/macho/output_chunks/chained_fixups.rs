//! The LC_DYLD_CHAINED_FIXUPS payload in __LINKEDIT: the modern
//! replacement for the rebase and bind opcode streams, with the fixup
//! chains it describes threaded through the data sections.

use crate::fatal;
use crate::macho::arch::Arch;
use crate::macho::arch::RelocClass;
use crate::macho::context::Context;
use crate::macho::format::*;
use crate::macho::input_files::FileId;
use crate::macho::output_chunks::ChunkHeader;
use crate::macho::passes::file_display;
use crate::macho::symbol::SymbolId;

#[derive(Debug)]
pub struct ChainedFixupsSection {
    pub hdr: ChunkHeader,
    /// The encoded payload, built during layout.
    pub contents: Vec<u8>,
    /// Every dynamic fixup location, sorted by address: (address,
    /// bound symbol or None for a rebase, addend).
    pub fixups: Vec<(u64, Option<SymbolId>, u64)>,
    /// The import table: (symbol, table addend), sorted; and each
    /// symbol's first ordinal.
    pub imports: Vec<(SymbolId, u64)>,
    pub ordinals: std::collections::HashMap<SymbolId, usize>,
}

impl Default for ChainedFixupsSection {
    fn default() -> Self {
        Self::new()
    }
}

impl ChainedFixupsSection {
    pub fn new() -> ChainedFixupsSection {
        ChainedFixupsSection {
            hdr: ChunkHeader::linkedit(),
            contents: Vec::new(),
            fixups: Vec::new(),
            imports: Vec::new(),
            ordinals: std::collections::HashMap::new(),
        }
    }
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    let data = &ctx.chained_fixups.contents;
    buf[..data.len()].copy_from_slice(data);
}

/// Builds the LC_DYLD_CHAINED_FIXUPS payload. Instead of opcode
/// streams, chained fixups store, per page of each segment, the offset
/// of the first fixup; each 64-bit fixup word in the data itself then
/// encodes its target (a rebase value or an import ordinal) plus the
/// distance to the next fixup in the page, forming a chain dyld walks.
/// Builds the chained-fixups payload; returns the encoded bytes, the
/// collected fixups, the import table and the symbol->import ordinal
/// map, for the caller to store on the context.
pub type ChainedFixups = (
    Vec<u8>,
    Vec<(u64, Option<crate::macho::symbol::SymbolId>, u64)>,
    Vec<(crate::macho::symbol::SymbolId, u64)>,
    std::collections::HashMap<crate::macho::symbol::SymbolId, usize>,
);

pub fn build_chained_fixups<E: Arch>(ctx: &Context<E>) -> ChainedFixups {
    // An image with nothing to fix up still gets the payload (a
    // header and a starts table with no pages), as ld64 writes it:
    // dyld reads the format from the load command, and its absence
    // would mean classic dyld info.
    let fixups = collect_fixups(ctx);

    // The import table: one entry per (symbol, table addend). Addends
    // up to 255 are carried inline in the fixup word and use the
    // symbol's base entry.
    let mut dynsyms: Vec<(crate::macho::symbol::SymbolId, u64)> = fixups
        .iter()
        .filter_map(|&(_, sym, addend)| {
            sym.map(|s| (s, if addend <= MAX_INLINE_ADDEND { 0 } else { addend }))
        })
        .collect();
    dynsyms.sort_unstable();
    dynsyms.dedup();
    let mut ordinals = std::collections::HashMap::new();
    for (i, &(sym, _)) in dynsyms.iter().enumerate().rev() {
        ordinals.insert(sym, i);
    }

    let max_addend = dynsyms.iter().map(|&(_, a)| a).max().unwrap_or(0);
    let import_format = if max_addend == 0 {
        DYLD_CHAINED_IMPORT
    } else if dynsyms.iter().all(|&(_, a)| i32::try_from(a as i64).is_ok()) {
        DYLD_CHAINED_IMPORT_ADDEND
    } else {
        DYLD_CHAINED_IMPORT_ADDEND64
    };

    let push32 = |buf: &mut Vec<u8>, v: u32| buf.extend_from_slice(&v.to_le_bytes());
    let push16 = |buf: &mut Vec<u8>, v: u16| buf.extend_from_slice(&v.to_le_bytes());
    let push64 = |buf: &mut Vec<u8>, v: u64| buf.extend_from_slice(&v.to_le_bytes());
    let pad8 = |buf: &mut Vec<u8>| {
        while !buf.len().is_multiple_of(8) {
            buf.push(0);
        }
    };

    let mut buf = Vec::new();
    // dyld_chained_fixups_header; the offsets are backpatched.
    push32(&mut buf, 0); // fixups_version
    push32(&mut buf, 0); // starts_offset
    push32(&mut buf, 0); // imports_offset
    push32(&mut buf, 0); // symbols_offset
    push32(&mut buf, dynsyms.len() as u32);
    push32(&mut buf, import_format);
    push32(&mut buf, 0); // symbols_format: uncompressed
    pad8(&mut buf);

    // dyld_chained_starts_in_image
    let starts_offset = buf.len();
    buf[4..8].copy_from_slice(&(starts_offset as u32).to_le_bytes());
    let seg_count = ctx.segments.len();
    push32(&mut buf, seg_count as u32);
    let seg_info_table = buf.len();
    for _ in 0..seg_count {
        push32(&mut buf, 0);
    }
    pad8(&mut buf);

    // Per-segment page tables
    let image_base = ctx.args.pagezero_size;
    for (seg_idx, seg) in ctx.segments.iter().enumerate() {
        let lo = fixups.partition_point(|&(a, _, _)| a < seg.cmd.vmaddr);
        let hi = fixups.partition_point(|&(a, _, _)| a < seg.cmd.vmaddr + seg.cmd.vmsize);
        if lo == hi {
            continue;
        }
        let fx = &fixups[lo..hi];

        let off = buf.len() - starts_offset;
        let ent = seg_info_table + seg_idx * 4;
        buf[ent..ent + 4].copy_from_slice(&(off as u32).to_le_bytes());

        let page_size = E::PAGE_SIZE;
        let npages = ((fx.last().unwrap().0 + 1 - seg.cmd.vmaddr).div_ceil(page_size)) as usize;
        // The record is 22 bytes of fields plus one u16 per page,
        // padded to 8; the declared size must match the bytes present.
        let size = crate::util::align_to(22 + npages as u64 * 2, 8) as u32;
        let rec_start = buf.len();

        push32(&mut buf, size);
        push16(&mut buf, page_size as u16);
        push16(&mut buf, DYLD_CHAINED_PTR_64);
        push64(&mut buf, seg.cmd.vmaddr - image_base);
        push32(&mut buf, 0); // max_valid_pointer
        push16(&mut buf, npages as u16);
        let mut j = 0;
        for i in 0..npages {
            let page_addr = seg.cmd.vmaddr + i as u64 * page_size;
            while j < fx.len() && fx[j].0 < page_addr {
                j += 1;
            }
            if j < fx.len() && fx[j].0 < page_addr + page_size {
                push16(&mut buf, (fx[j].0 & (page_size - 1)) as u16);
            } else {
                push16(&mut buf, DYLD_CHAINED_PTR_START_NONE);
            }
        }
        buf.resize(rec_start + size as usize, 0);
    }

    // Import table
    let imports_offset = buf.len();
    buf[8..12].copy_from_slice(&(imports_offset as u32).to_le_bytes());
    let mut name_offs = Vec::with_capacity(dynsyms.len());
    let mut nameoff: u32 = 0;
    for (i, &(sym, _)) in dynsyms.iter().enumerate() {
        name_offs.push(nameoff);
        if i + 1 == dynsyms.len() || dynsyms[i + 1].0 != sym {
            nameoff += ctx.symbols[sym].name().len() as u32 + 1;
        }
    }
    for (i, &(sym, addend)) in dynsyms.iter().enumerate() {
        let s = &ctx.symbols[sym];
        // An import names its dylib; one of this image's own weak
        // definitions is bound by weak lookup (ordinal -3), which
        // makes dyld search every loaded image for the coalesced
        // winner.
        let ordinal_bits = |bits: u32| -> u64 {
            match s.file() {
                Some(FileId::Dylib(dylib)) if !ctx.binds_weak_lookup(sym) => {
                    ctx.chained_import_ordinal(dylib, bits)
                }
                _ => (BIND_SPECIAL_DYLIB_WEAK_LOOKUP as i64 as u64) & ((1u64 << bits) - 1),
            }
        };
        let weak = s.is_weak_ref() as u32;
        match import_format {
            DYLD_CHAINED_IMPORT => {
                let ordinal = ordinal_bits(8) as u32;
                push32(&mut buf, ordinal | (weak << 8) | (name_offs[i] << 9));
            }
            DYLD_CHAINED_IMPORT_ADDEND => {
                let ordinal = ordinal_bits(8) as u32;
                push32(&mut buf, ordinal | (weak << 8) | (name_offs[i] << 9));
                push32(&mut buf, addend as u32);
            }
            _ => {
                let ordinal = ordinal_bits(16);
                push64(&mut buf, ordinal | ((weak as u64) << 16) | ((name_offs[i] as u64) << 32));
                push64(&mut buf, addend);
            }
        }
    }

    // Symbol names
    let symbols_offset = buf.len();
    buf[12..16].copy_from_slice(&(symbols_offset as u32).to_le_bytes());
    for (i, &(sym, _)) in dynsyms.iter().enumerate() {
        if i + 1 == dynsyms.len() || dynsyms[i + 1].0 != sym {
            buf.extend_from_slice(ctx.symbols[sym].name().as_bytes());
            buf.push(0);
        }
    }
    pad8(&mut buf);

    (buf, fixups, dynsyms, ordinals)
}

/// Writes the fixup chains into the copied output: every fixup word is
/// rewritten to encode its payload plus the 4-byte-stride distance to
/// the next fixup in the same page.
pub fn write_fixup_chains<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    let page_mask = !(E::PAGE_SIZE - 1);

    for seg in &ctx.segments {
        let lo = ctx.chained_fixups.fixups.partition_point(|&(a, _, _)| a < seg.cmd.vmaddr);
        let hi = ctx
            .chained_fixups
            .fixups
            .partition_point(|&(a, _, _)| a < seg.cmd.vmaddr + seg.cmd.vmsize);
        let fx = &ctx.chained_fixups.fixups[lo..hi];

        for (i, &(addr, sym, addend)) in fx.iter().enumerate() {
            let next = match fx.get(i + 1) {
                Some(&(next_addr, _, _)) if next_addr & page_mask == addr & page_mask => {
                    (next_addr - addr) / 4
                }
                _ => 0,
            };
            if addr % 4 != 0 {
                fatal!("unaligned fixup; re-link with -no_fixup_chains");
            }

            let off = (seg.cmd.fileoff + (addr - seg.cmd.vmaddr)) as usize;
            let word = match sym {
                Some(sym) => {
                    // dyld_chained_ptr_64_bind
                    let ordinal = if addend <= MAX_INLINE_ADDEND {
                        ctx.chained_fixups.ordinals[&sym] as u64
                    } else {
                        let base = ctx.chained_fixups.ordinals[&sym];
                        ctx.chained_fixups.imports[base..]
                            .iter()
                            .position(|&(s, a)| s == sym && a == addend)
                            .map(|p| (base + p) as u64)
                            .unwrap()
                    };
                    let inline_addend = if addend <= MAX_INLINE_ADDEND { addend } else { 0 };
                    ordinal | (inline_addend << 24) | (next << 51) | (1 << 63)
                }
                None => {
                    // dyld_chained_ptr_64_rebase; the word currently
                    // holds the absolute target address.
                    let val = u64::from_le_bytes(buf[off..off + 8].try_into().unwrap());
                    if val & 0x00ff_fff0_0000_0000 != 0 {
                        let sect = ctx
                            .chunks
                            .iter()
                            .map(|&id| ctx.chunk_header(id))
                            .find(|hdr| hdr.addr <= addr && addr < hdr.addr + hdr.size)
                            .map(|hdr| format!("{},{}", hdr.segname, hdr.sectname))
                            .unwrap_or_default();
                        fatal!(
                            "rebase target unencodable at {addr:#x} in {sect} (value {val:#x}); re-link with -no_fixup_chains"
                        );
                    }
                    let target = val & 0xf_ffff_ffff;
                    let high8 = val >> 56;
                    target | (high8 << 36) | (next << 51)
                }
            };
            buf[off..off + 8].copy_from_slice(&word.to_le_bytes());
        }
    }
}

pub fn collect_fixups<E: Arch>(
    ctx: &Context<E>,
) -> Vec<(u64, Option<crate::macho::symbol::SymbolId>, u64)> {
    use rayon::prelude::*;

    // Every subsection's fixups are independent; collect them on all
    // cores and sort the union in parallel, as mold does.
    let mut fixups: Vec<(u64, Option<crate::macho::symbol::SymbolId>, u64)> = ctx
        .isecs
        .par_iter()
        .filter(|isec| {
            isec.is_alive() && isec.replacement == crate::macho::input_sections::NO_REPLACEMENT
        })
        .flat_map_iter(|isec| {
            let base = ctx.chunk_header(isec.output_section().unwrap()).addr + isec.offset as u64;
            crate::macho::input_files::isec_relocs_of(&ctx.objs, isec).iter().filter_map(
                move |rel| {
                    if E::classify_reloc(rel.r_type) != RelocClass::Plain
                        || rel.size != 8
                        || rel.is_pcrel
                        || rel.is_subtracted
                        || rel.r_type == E::RELOC_SUBTRACTOR
                    {
                        return None;
                    }
                    if ctx
                        .reloc_target_sym(isec.file as usize, rel)
                        .is_some_and(|id| ctx.is_absolute_symbol(id) && !ctx.binds_at_runtime(id))
                    {
                        return None;
                    }
                    let addr = base + rel.offset as u64;
                    // A chain link's stride is 4 bytes, so a fixup at an
                    // unaligned address is unrepresentable. ld64 diagnoses
                    // the offending input section rather than the output.
                    if !addr.is_multiple_of(4) {
                        fatal!(
                            "{}({},{}): unaligned base relocation",
                            file_display(&ctx.objs[isec.file as usize]),
                            ctx.hdr_of(isec).segname(),
                            ctx.hdr_of(isec).sectname()
                        );
                    }
                    match ctx.reloc_target_sym(isec.file as usize, rel) {
                        Some(id) if ctx.binds_at_runtime(id) => {
                            Some((addr, Some(id), rel.addend as u64))
                        }
                        _ => {
                            if !ctx.reloc_target_is_tls(isec.file as usize, rel) {
                                Some((addr, None, 0))
                            } else {
                                None
                            }
                        }
                    }
                },
            )
        })
        .collect();

    {
        let addr = ctx.got.hdr.addr;
        for (i, &id) in ctx.got.got_syms.iter().enumerate() {
            if ctx.is_absolute_symbol(id) && !ctx.binds_at_runtime(id) {
                continue;
            }
            let sym = Some(id).filter(|&id| ctx.binds_at_runtime(id));
            fixups.push((addr + i as u64 * 8, sym, 0));
        }
    }
    {
        let addr = ctx.thread_ptrs.hdr.addr;
        for (i, &id) in ctx.thread_ptrs.symbols.iter().enumerate() {
            let sym = Some(id).filter(|&id| ctx.symbols[id].is_imported());
            fixups.push((addr + i as u64 * 8, sym, 0));
        }
    }
    for i in 0..ctx.objc_stubs.symbols.len() + ctx.objc_stubs.extra_selrefs.len() {
        if !ctx.objc_stub_reuses_selref(i) {
            fixups.push((ctx.objc_selref_addr(i), None, 0));
        }
    }
    for (addr, _) in super::dyld_info::data_blob_pointers(ctx) {
        fixups.push((addr, None, 0));
    }

    fixups.par_sort_unstable_by_key(|&(addr, _, _)| addr);
    fixups
}

/// The largest addend a chained bind can carry inline; anything bigger
/// goes into the import table.
const MAX_INLINE_ADDEND: u64 = 255;
