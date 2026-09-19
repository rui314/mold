//! __TEXT,__unwind_info: the compact unwind table, generated from the
//! objects' __compact_unwind records.

use crate::macho::arch::Arch;
use crate::macho::context::Context;
use crate::macho::format::*;
use crate::macho::output_chunks::ChunkHeader;
use crate::macho::symbol::SymbolId;

#[derive(Debug)]
pub struct UnwindInfoSection {
    pub hdr: ChunkHeader,
    /// The encoded table, except its personality cells (GOT addresses
    /// unknown when __TEXT is sized): the symbols to patch into offsets
    /// 28, 32, ... at copy time.
    pub contents: Vec<u8>,
    pub personalities: Vec<SymbolId>,
}

impl UnwindInfoSection {
    pub fn new() -> UnwindInfoSection {
        let mut hdr = ChunkHeader::new("__TEXT", "__unwind_info");
        hdr.p2align = 2;
        UnwindInfoSection { hdr, contents: Vec::new(), personalities: Vec::new() }
    }
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    let sec = &ctx.unwind_info;
    debug_assert_eq!(sec.contents.len() as u64, sec.hdr.size);
    buf[..sec.contents.len()].copy_from_slice(&sec.contents);
    // Patch the personality cells now the GOT has addresses; the header
    // says where the array is (after the common encodings).
    let base = ctx.args.pagezero_size;
    let personality_off = u32::from_le_bytes(sec.contents[12..16].try_into().unwrap()) as usize;
    for (i, &sym) in sec.personalities.iter().enumerate() {
        let off = personality_off + i * 4;
        let val = ctx.sym_got_addr(sym).wrapping_sub(base) as u32;
        buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
    }
}

/// Encodes the __unwind_info section from the compact unwind records.
///
/// __unwind_info stores unwind records in two-level tables: a first-level
/// table of page entries, each covering up to 2^24 bytes of code, and
/// second-level pages holding 32-bit entries with the function's low
/// address bits and an index into a per-page encoding table.
///
/// This runs twice: once during layout for the section's size (function
/// addresses are final by then, so the size is stable) and once when the
/// output is written, with every referenced address final.
/// Encodes __unwind_info. The personality entries are image-relative
/// pointers to GOT slots, whose addresses are not final when __TEXT
/// (and this section's size) is computed - so they are returned as a
/// patch list instead of written, and the copy phase fills the cells
/// at offsets 28, 32, ... once the GOT has its address. Everything
/// else in the encoding is final at sizing time.
pub fn encode_unwind_info<E: Arch>(ctx: &Context<E>) -> (Vec<u8>, Vec<SymbolId>) {
    use rayon::prelude::*;
    let mut records: Vec<crate::macho::input_files::UnwindRecord> = ctx
        .unwind_records
        .par_iter()
        .filter(|rec| {
            ctx.isecs[rec.isec as usize].is_alive()
                && ctx.isecs[rec.isec as usize].replacement
                    == crate::macho::input_sections::NO_REPLACEMENT
        })
        .cloned()
        .collect();
    if records.is_empty() {
        return (Vec::new(), Vec::new());
    }

    let base = ctx.args.pagezero_size;
    let func_addr = |r: &crate::macho::input_files::UnwindRecord| {
        ctx.isec_addr(r.isec as usize) + r.input_offset as u64
    };

    // Records synthesized from DWARF unwind info encode the FDE's
    // offset in __eh_frame in the low 24 bits.
    for rec in &mut records {
        if let Some(fde) = rec.fde() {
            rec.encoding = E::UNWIND_MODE_DWARF | (ctx.fdes[fde].output_offset & 0xff_ffff);
        }
    }

    // Assign personality indices, encoded in bits 28-29 of the encoding.
    let mut personalities: Vec<SymbolId> = Vec::new();
    for rec in &mut records {
        if let Some(p) = rec.personality() {
            let idx = match personalities.iter().position(|&s| s == p) {
                Some(idx) => idx,
                None => {
                    personalities.push(p);
                    personalities.len() - 1
                }
            };
            if idx >= 3 {
                crate::fatal!("too many personality functions");
            }
            rec.encoding |= ((idx + 1) as u32) << UNWIND_PERSONALITY_MASK.trailing_zeros();
        }
    }

    records.par_sort_by_key(func_addr);

    // Merge adjacent records with identical contents.
    let mut merged: Vec<crate::macho::input_files::UnwindRecord> =
        Vec::with_capacity(records.len());
    for rec in records {
        match merged.last_mut() {
            Some(last)
                if func_addr(last) + last.code_len as u64 == func_addr(&rec)
                    && last.encoding == rec.encoding
                    && last.personality() == rec.personality()
                    && last.lsda().is_none()
                    && rec.lsda().is_none() =>
            {
                last.code_len += rec.code_len;
            }
            _ => merged.push(rec),
        }
    }
    let records = merged;

    // The common encodings table: the encodings the image uses more
    // than once, most frequent first, up to 127 of them (a compressed
    // entry's 8-bit index names a common encoding below the table's
    // count and a page-local one above it). ld64 fills it the same
    // way; a one-off encoding - every DWARF-mode one, with its FDE
    // offset - stays page-local.
    let common: Vec<(u32, usize)> = {
        let mut freq: std::collections::HashMap<u32, (usize, usize)> =
            std::collections::HashMap::new();
        for (i, rec) in records.iter().enumerate() {
            let e = freq.entry(rec.encoding).or_insert((0, i));
            e.0 += 1;
        }
        let mut all: Vec<(u32, usize, usize)> =
            freq.into_iter().map(|(e, (n, first))| (e, n, first)).collect();
        all.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
        all.into_iter().filter(|&(_, n, _)| n > 1).take(127).map(|(e, n, _)| (e, n)).collect()
    };
    let common_idx: std::collections::HashMap<u32, u32> =
        common.iter().enumerate().map(|(i, &(e, _))| (e, i as u32)).collect();

    // Second-level pages, 4096 bytes each, filled from the end of the
    // record list as ld64 does (so the first page is the partial one).
    // A compressed page holds 32-bit entries (a 24-bit offset from the
    // page's first function and an 8-bit encoding index) plus its
    // page-local encodings; a regular page 8-byte entries. Each page
    // takes the format that holds more of the remaining records.
    const PAGE_SIZE: usize = 4096;
    const COMPRESSED_HDR: usize = 12;
    const REGULAR_HDR: usize = 8;
    struct Page {
        start: usize,
        end: usize,
        compressed: bool,
        encodings: Vec<u32>,
    }
    let mut pages: Vec<Page> = Vec::new();
    let mut end = records.len();
    while end > 0 {
        let last_addr = func_addr(&records[end - 1]);
        let mut encs: Vec<u32> = Vec::new();
        let mut n = 0;
        let mut i = end;
        while i > 0 {
            let rec = &records[i - 1];
            let is_common = common_idx.contains_key(&rec.encoding);
            let new_enc = !is_common && !encs.contains(&rec.encoding);
            if new_enc && common.len() + encs.len() + 1 > 256 {
                break;
            }
            let encs_len = encs.len() + new_enc as usize;
            if COMPRESSED_HDR + (n + 1) * 4 + encs_len * 4 > PAGE_SIZE {
                break;
            }
            if last_addr - func_addr(rec) >= (1 << 24) {
                break;
            }
            if new_enc {
                encs.push(rec.encoding);
            }
            n += 1;
            i -= 1;
        }
        let regular = end.min((PAGE_SIZE - REGULAR_HDR) / 8);
        if n >= regular {
            pages.push(Page { start: end - n, end, compressed: true, encodings: encs });
            end -= n;
        } else {
            pages.push(Page {
                start: end - regular,
                end,
                compressed: false,
                encodings: Vec::new(),
            });
            end -= regular;
        }
    }
    pages.reverse();

    let num_lsda = records.iter().filter(|r| r.lsda().is_some()).count();

    // Compute the layout of the section.
    let common_off = 28;
    let personality_off = common_off + common.len() * 4;
    let page1_off = personality_off + personalities.len() * 4;
    let lsda_off = page1_off + (pages.len() + 1) * 12;
    let page2_off = lsda_off + num_lsda * 8;

    let push32 = |buf: &mut Vec<u8>, val: u32| buf.extend_from_slice(&val.to_le_bytes());
    let push16 = |buf: &mut Vec<u8>, val: u16| buf.extend_from_slice(&val.to_le_bytes());

    let mut buf = Vec::new();
    push32(&mut buf, UNWIND_SECTION_VERSION);
    push32(&mut buf, common_off as u32);
    push32(&mut buf, common.len() as u32);
    push32(&mut buf, personality_off as u32);
    push32(&mut buf, personalities.len() as u32);
    push32(&mut buf, page1_off as u32);
    push32(&mut buf, pages.len() as u32 + 1);
    for &(enc, _) in &common {
        push32(&mut buf, enc);
    }

    // Personalities are image-relative pointers to the functions' GOT
    // slots, patched in by the copy phase (see above).
    for &_sym in &personalities {
        push32(&mut buf, 0);
    }

    // Each second-level page's blob and LSDA rows depend only on its
    // own records, so the pages build in parallel; the first-level
    // index is then a serial walk over the blob lengths.
    struct PageOut {
        page2: Vec<u8>,
        lsda: Vec<u8>,
        first: u32,
    }
    let outs: Vec<PageOut> = pages
        .par_iter()
        .map(|page| {
            let span = &records[page.start..page.end];
            let mut page2 = Vec::new();
            let mut lsda = Vec::new();
            for rec in span {
                if let Some((isec, off)) = rec.lsda() {
                    push32(&mut lsda, (func_addr(rec) - base) as u32);
                    push32(&mut lsda, (ctx.isec_addr(isec) + off as u64 - base) as u32);
                }
            }

            if page.compressed {
                push32(&mut page2, UNWIND_SECOND_LEVEL_COMPRESSED);
                push16(&mut page2, COMPRESSED_HDR as u16); // entries offset
                push16(&mut page2, span.len() as u16);
                push16(&mut page2, (COMPRESSED_HDR + span.len() * 4) as u16); // encodings offset
                push16(&mut page2, page.encodings.len() as u16);
                let page_base = func_addr(&span[0]);
                for rec in span {
                    let enc_idx = match common_idx.get(&rec.encoding) {
                        Some(&i) => i,
                        None => {
                            common.len() as u32
                                + page.encodings.iter().position(|&e| e == rec.encoding).unwrap()
                                    as u32
                        }
                    };
                    let entry = (func_addr(rec) - page_base) as u32 | enc_idx << 24;
                    push32(&mut page2, entry);
                }
                for enc in &page.encodings {
                    push32(&mut page2, *enc);
                }
            } else {
                push32(&mut page2, UNWIND_SECOND_LEVEL_REGULAR);
                push16(&mut page2, REGULAR_HDR as u16);
                push16(&mut page2, span.len() as u16);
                for rec in span {
                    push32(&mut page2, (func_addr(rec) - base) as u32);
                    push32(&mut page2, rec.encoding);
                }
            }
            PageOut { page2, lsda, first: (func_addr(&span[0]) - base) as u32 }
        })
        .collect();

    let mut page1 = Vec::new();
    let mut lsda = Vec::new();
    let mut page2 = Vec::new();
    for out in &outs {
        push32(&mut page1, out.first);
        push32(&mut page1, (page2_off + page2.len()) as u32);
        push32(&mut page1, (lsda_off + lsda.len()) as u32);
        lsda.extend_from_slice(&out.lsda);
        page2.extend_from_slice(&out.page2);
    }

    // The terminating first-level entry.
    let last = records.last().unwrap();
    push32(&mut page1, (func_addr(last) + last.code_len as u64 + 1 - base) as u32);
    push32(&mut page1, 0);
    push32(&mut page1, (lsda_off + lsda.len()) as u32);

    buf.extend_from_slice(&page1);
    buf.extend_from_slice(&lsda);
    buf.extend_from_slice(&page2);
    (buf, personalities)
}
