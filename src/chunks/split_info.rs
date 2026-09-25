//! LC_SEGMENT_SPLIT_INFO data: records inter-segment and inter-section
//! references so that a kernel collection or dyld shared-cache builder
//! can slide segments independently (as kmutil does).

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::input_sections::RelocTarget;
use crate::macho_consts::*;
use crate::target::Target;
use crate::util::encode_uleb;
use std::collections::BTreeMap;

pub const DYLD_CACHE_ADJ_V2_FORMAT: u8 = 0x7f;
pub const DYLD_CACHE_ADJ_V2_POINTER_32: u8 = 0x01;
pub const DYLD_CACHE_ADJ_V2_POINTER_64: u8 = 0x02;
pub const DYLD_CACHE_ADJ_V2_DELTA_32: u8 = 0x03;
pub const DYLD_CACHE_ADJ_V2_DELTA_64: u8 = 0x04;
pub const DYLD_CACHE_ADJ_V2_ARM64_ADRP: u8 = 0x05;
pub const DYLD_CACHE_ADJ_V2_ARM64_OFF12: u8 = 0x06;
pub const DYLD_CACHE_ADJ_V2_ARM64_BR26: u8 = 0x07;
pub const DYLD_CACHE_ADJ_V2_ARM_MOVW_MOVT: u8 = 0x08;
pub const DYLD_CACHE_ADJ_V2_ARM_BR24: u8 = 0x09;
pub const DYLD_CACHE_ADJ_V2_THUMB_MOVW_MOVT: u8 = 0x0a;
pub const DYLD_CACHE_ADJ_V2_THUMB_BR22: u8 = 0x0b;
pub const DYLD_CACHE_ADJ_V2_IMAGE_OFF_32: u8 = 0x0c;
pub const DYLD_CACHE_ADJ_V2_THREADED_POINTER_64: u8 = 0x0d;

#[derive(Debug)]
pub struct SplitInfoSection {
    pub hdr: ChunkHeader,
    pub contents: Vec<u8>,
}

impl SplitInfoSection {
    pub fn new() -> Self {
        Self { hdr: ChunkHeader::linkedit(), contents: Vec::new() }
    }
}

impl Default for SplitInfoSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let data = &ctx.split_info.contents;
    buf[..data.len()].copy_from_slice(data);
}

#[derive(Clone, Copy, Debug)]
pub struct SplitEntry {
    pub from_sect: u8,
    pub to_sect: u8,
    pub kind: u8,
    pub from_offset: u64,
    pub to_offset: u64,
}

pub fn build<E: Target>(ctx: &Context<E>) -> Vec<u8> {
    if !ctx.args.split_seg_info {
        return Vec::new();
    }

    let mut entries = Vec::new();

    // Iterate all live subsections and collect cross-section references.
    for isec_id in 0..ctx.isecs.len() {
        let isec = &ctx.isecs[isec_id];
        if !isec.is_alive() {
            continue;
        }
        let from_sect = ctx.isec_n_sect(isec);
        if from_sect == 0 {
            continue;
        }
        if isec.output_section().is_none() {
            continue;
        }
        let from_isec_offset = isec.offset as u64;

        let obj = isec.file as usize;
        let rels = ctx.isec_relocs(isec_id);

        let mut i = 0;
        while i < rels.len() {
            let r = &rels[i];
            let a = r.addend;
            let offset_in_sect = from_isec_offset + r.offset as u64;

            // Target information
            let (to_sect, to_offset, kind) = match r.r_type {
                ARM64_RELOC_PAGE21 | ARM64_RELOC_GOT_LOAD_PAGE21 => {
                    let target_isec = match r.target() {
                        RelocTarget::Sym(idx) => {
                            let sym_id = ctx.objs[obj].symbols[idx as usize];
                            ctx.symbols[sym_id].input_section().map(|s| s as usize)
                        }
                        RelocTarget::Section(idx) => Some(idx as usize),
                    };
                    if let Some(tisec_id) = target_isec {
                        let tisec = &ctx.isecs[ctx.resolve_isec(tisec_id)];
                        let t_sect = ctx.isec_n_sect(tisec);
                        if t_sect != 0 && t_sect != from_sect {
                            let sym_val = match r.target() {
                                RelocTarget::Sym(idx) => {
                                    ctx.symbols[ctx.objs[obj].symbols[idx as usize]].value
                                }
                                RelocTarget::Section(_) => 0,
                            };
                            let to_off = tisec.offset as u64 + sym_val.wrapping_add_signed(a);
                            (t_sect, to_off, DYLD_CACHE_ADJ_V2_ARM64_ADRP)
                        } else {
                            i += 1;
                            continue;
                        }
                    } else {
                        i += 1;
                        continue;
                    }
                }
                ARM64_RELOC_PAGEOFF12 | ARM64_RELOC_GOT_LOAD_PAGEOFF12 => {
                    let target_isec = match r.target() {
                        RelocTarget::Sym(idx) => {
                            let sym_id = ctx.objs[obj].symbols[idx as usize];
                            ctx.symbols[sym_id].input_section().map(|s| s as usize)
                        }
                        RelocTarget::Section(idx) => Some(idx as usize),
                    };
                    if let Some(tisec_id) = target_isec {
                        let tisec = &ctx.isecs[ctx.resolve_isec(tisec_id)];
                        let t_sect = ctx.isec_n_sect(tisec);
                        if t_sect != 0 && t_sect != from_sect {
                            let sym_val = match r.target() {
                                RelocTarget::Sym(idx) => {
                                    ctx.symbols[ctx.objs[obj].symbols[idx as usize]].value
                                }
                                RelocTarget::Section(_) => 0,
                            };
                            let to_off = tisec.offset as u64 + sym_val.wrapping_add_signed(a);
                            (t_sect, to_off, DYLD_CACHE_ADJ_V2_ARM64_OFF12)
                        } else {
                            i += 1;
                            continue;
                        }
                    } else {
                        i += 1;
                        continue;
                    }
                }
                ARM64_RELOC_BRANCH26 => {
                    let target_isec = match r.target() {
                        RelocTarget::Sym(idx) => {
                            let sym_id = ctx.objs[obj].symbols[idx as usize];
                            ctx.symbols[sym_id].input_section().map(|s| s as usize)
                        }
                        RelocTarget::Section(idx) => Some(idx as usize),
                    };
                    if let Some(tisec_id) = target_isec {
                        let tisec = &ctx.isecs[ctx.resolve_isec(tisec_id)];
                        let t_sect = ctx.isec_n_sect(tisec);
                        if t_sect != 0 && t_sect != from_sect {
                            let sym_val = match r.target() {
                                RelocTarget::Sym(idx) => {
                                    ctx.symbols[ctx.objs[obj].symbols[idx as usize]].value
                                }
                                RelocTarget::Section(_) => 0,
                            };
                            let to_off = tisec.offset as u64 + sym_val.wrapping_add_signed(a);
                            (t_sect, to_off, DYLD_CACHE_ADJ_V2_ARM64_BR26)
                        } else {
                            i += 1;
                            continue;
                        }
                    } else {
                        i += 1;
                        continue;
                    }
                }
                ARM64_RELOC_UNSIGNED => {
                    let target_isec = match r.target() {
                        RelocTarget::Sym(idx) => {
                            let sym_id = ctx.objs[obj].symbols[idx as usize];
                            ctx.symbols[sym_id].input_section().map(|s| s as usize)
                        }
                        RelocTarget::Section(idx) => Some(idx as usize),
                    };
                    if let Some(tisec_id) = target_isec {
                        let tisec = &ctx.isecs[ctx.resolve_isec(tisec_id)];
                        let t_sect = ctx.isec_n_sect(tisec);
                        if t_sect != 0 {
                            let sym_val = match r.target() {
                                RelocTarget::Sym(idx) => {
                                    ctx.symbols[ctx.objs[obj].symbols[idx as usize]].value
                                }
                                RelocTarget::Section(_) => 0,
                            };
                            let to_off = tisec.offset as u64 + sym_val.wrapping_add_signed(a);
                            let k = if r.size == 8 {
                                DYLD_CACHE_ADJ_V2_POINTER_64
                            } else {
                                DYLD_CACHE_ADJ_V2_POINTER_32
                            };
                            (t_sect, to_off, k)
                        } else {
                            i += 1;
                            continue;
                        }
                    } else {
                        i += 1;
                        continue;
                    }
                }
                _ => {
                    i += 1;
                    continue;
                }
            };

            entries.push(SplitEntry {
                from_sect,
                to_sect,
                kind,
                from_offset: offset_in_sect,
                to_offset,
            });
            i += 1;
        }
    }

    if entries.is_empty() {
        return Vec::new();
    }

    // Encode V2 format:
    // Whole         :== <count> FromToSection+
    // FromToSection :== <from-sect-index> <to-sect-index> <count> ToOffset+
    // ToOffset      :== <to-sect-offset-delta> <count> FromOffset+
    // FromOffset    :== <kind> <count> <from-sect-offset-delta>

    // Grouping: (from_sect, to_sect) -> to_offset -> kind -> Vec<from_offset>
    let mut whole: BTreeMap<(u8, u8), BTreeMap<u64, BTreeMap<u8, Vec<u64>>>> = BTreeMap::new();
    for e in entries {
        whole
            .entry((e.from_sect, e.to_sect))
            .or_default()
            .entry(e.to_offset)
            .or_default()
            .entry(e.kind)
            .or_default()
            .push(e.from_offset);
    }

    let mut buf = Vec::new();
    buf.push(DYLD_CACHE_ADJ_V2_FORMAT);

    encode_uleb(&mut buf, whole.len() as u64);
    for ((from_sect, to_sect), to_offsets) in whole {
        encode_uleb(&mut buf, from_sect as u64);
        encode_uleb(&mut buf, to_sect as u64);
        encode_uleb(&mut buf, to_offsets.len() as u64);

        let mut last_to_offset = 0u64;
        for (to_offset, from_offsets) in to_offsets {
            encode_uleb(&mut buf, to_offset - last_to_offset);
            encode_uleb(&mut buf, from_offsets.len() as u64);

            for (kind, mut offsets) in from_offsets {
                encode_uleb(&mut buf, kind as u64);
                encode_uleb(&mut buf, offsets.len() as u64);
                offsets.sort_unstable();
                let mut last_from_offset = 0u64;
                for off in offsets {
                    encode_uleb(&mut buf, off - last_from_offset);
                    last_from_offset = off;
                }
            }
            last_to_offset = to_offset;
        }
    }

    buf.push(0); // trailing null byte
    while buf.len() % 8 != 0 {
        buf.push(0);
    }

    buf
}
