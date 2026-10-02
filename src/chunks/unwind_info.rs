//! __TEXT,__unwind_info: the compact unwind table, generated from the
//! objects' __compact_unwind records.

use rayon::prelude::*;

use crate::chunks::ChunkHeader;
use crate::chunks::sectcreate::InputPlace;
use crate::context::Context;
use crate::input_files::UnwindRecord;
use crate::input_sections::InputSection;
use crate::macho::*;
use crate::symbol::SymbolId;
use crate::target::Target;

#[derive(Debug)]
pub struct UnwindInfoSection {
    pub hdr: ChunkHeader,
    /// The encoded table, except its personality cells (GOT addresses
    /// unknown when __TEXT is sized): the symbols to patch into offsets
    /// 28, 32, ... at copy time.
    pub contents: Vec<u8>,
    pub personalities: Vec<SymbolId>,
    /// The room __TEXT keeps for the section when that is more than its
    /// first encoding takes: what an encoding with every segment placed
    /// took (see set_osec_offsets). The contents are padded with zeros.
    pub min_size: u64,
}

/// The greatest offset into __eh_frame the low 24 bits of a DWARF-mode
/// encoding hold.
pub const MAX_FDE_OFFSET: u32 = 0xff_ffff;

impl UnwindInfoSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::new(b"__TEXT", b"__unwind_info");
        hdr.p2align = 2;
        Self { hdr, contents: Vec::new(), personalities: Vec::new(), min_size: 0 }
    }
}

impl Default for UnwindInfoSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let sec = &ctx.unwind_info;
    debug_assert!(sec.contents.len() as u64 <= sec.hdr.size);
    buf[..sec.contents.len()].copy_from_slice(&sec.contents);
    // Patch the personality cells now the GOT has addresses; the header
    // says where the array is (after the common encodings).
    let base = ctx.mach_header.hdr.addr;
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
/// It runs as __TEXT is laid out, for the section's size, when the
/// addresses in __TEXT are final and those in other segments are not
/// yet (they are taken as they are, wrapping around the image base);
/// if the section covers any of those (covers_other_segments), it runs
/// again once every segment is placed. The personality entries are
/// image-relative pointers to GOT slots, whose addresses are not final
/// either - so they are returned as a patch list instead of written,
/// and the copy phase fills the cells at offsets 28, 32, ... once the
/// GOT has its address.
pub fn encode_unwind_info<E: Target>(ctx: &Context<E>) -> (Vec<u8>, Vec<SymbolId>) {
    let mut records: Vec<crate::input_files::UnwindRecord> = ctx
        .unwind_records
        .par_iter()
        .filter(|rec| {
            // A folded copy's record is gone, but for one that kept its
            // FDE (see output_sections::kept_fdes_of).
            ctx.isecs[rec.isec as usize].is_alive()
                && (ctx.isecs[rec.isec as usize].replacement
                    == crate::input_sections::NO_REPLACEMENT
                    || rec.fde().is_some())
        })
        .cloned()
        .collect();
    records.extend(bare_code_records(ctx, &records));
    if records.is_empty() {
        return (Vec::new(), Vec::new());
    }

    let base = ctx.mach_header.hdr.addr;
    let func_addr = |r: &crate::input_files::UnwindRecord| {
        ctx.isec_addr(r.isec as usize) + r.input_offset as u64
    };

    // A DWARF-mode record's encoding holds its FDE's offset in
    // __eh_frame in the low 24 bits, or 0 if they can't hold it, as in
    // ld-prime (which warns, see lay_out_eh_frame): the unwinder then
    // looks for the FDE through the whole section. Its personality and
    // LSDA are the FDE's, which ld-prime lists in the tables below as a
    // compact record's, though the unwinder reads them from the FDE.
    for rec in &mut records {
        if let Some(fde) = rec.fde() {
            let off = ctx.fdes[fde].output_offset;
            rec.encoding = E::UNWIND_MODE_DWARF | if off <= MAX_FDE_OFFSET { off } else { 0 };
            if let Some(p) = function_personality(ctx, rec) {
                rec.personality_sym = p;
            }
            if let Some((isec, off)) = function_lsda(ctx, rec) {
                (rec.lsda_isec, rec.lsda_off) = (isec as u32, off);
                rec.encoding |= UNWIND_HAS_LSDA;
            }
        }
    }

    // ld-prime orders the entries of one address by encoding: an empty
    // subsection's of encoding 0 comes before the function sharing its
    // address, and of two records for a function (or one at a
    // section's end and the next section's first), the greater
    // encoding, which the unwinder finds, comes last.
    records.par_sort_by_key(|r| (func_addr(r), r.encoding));

    // Assign personality indices, encoded in bits 28-29 of the
    // encoding, in order of first use by address.
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

    // The table ends where the last function does, as in ld64, however
    // much of it its unwind record covers.
    let last = records.last().unwrap();
    let end = ctx.isec_addr(last.isec as usize) + ctx.isecs[last.isec as usize].size as u64;

    // Merge consecutive records with identical contents. An entry has no
    // length - it covers the code up to the next one - so the padding
    // between two functions does not keep them apart. ld-prime keeps
    // each entry of encoding 0, code without unwind info, though, and
    // each in DWARF mode (alike only if their FDEs are out of reach).
    records.dedup_by(|rec, last| {
        rec.encoding != 0
            && rec.encoding & UNWIND_MODE_MASK != E::UNWIND_MODE_DWARF
            && last.encoding == rec.encoding
            && last.personality() == rec.personality()
            && last.lsda().is_none()
            && rec.lsda().is_none()
    });

    // The common encodings table: the encodings the merged entries use
    // more than once, most frequent first and equally frequent ones in
    // increasing order, up to 127 of them (a compressed entry's 8-bit
    // index names a common encoding below the table's count and a
    // page-local one above it). ld64 fills it the same way; a one-off
    // encoding stays page-local, as does every DWARF-mode one, even
    // one that several FDEs out of reach share.
    let common: Vec<(u32, usize)> = {
        let mut freq: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
        for rec in records.iter().filter(|r| r.encoding & UNWIND_MODE_MASK != E::UNWIND_MODE_DWARF)
        {
            *freq.entry(rec.encoding).or_default() += 1;
        }
        let mut all: Vec<(u32, usize)> = freq.into_iter().collect();
        all.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        all.into_iter().filter(|&(_, n)| n > 1).take(127).collect()
    };
    let common_idx: std::collections::HashMap<u32, u32> =
        common.iter().enumerate().map(|(i, &(e, _))| (e, i as u32)).collect();

    // Second-level pages, 4096 bytes each, filled from the start of the
    // record list as ld-prime does (so the last page is the partial
    // one). A compressed page holds 32-bit entries (a 24-bit offset from
    // the page's first function and an 8-bit encoding index) plus its
    // page-local encodings, in order of first use; a regular page 8-byte
    // entries. Each page takes the format that holds more of the
    // remaining records.
    const PAGE_SIZE: usize = 4096;
    const COMPRESSED_HDR: usize = 12;
    const REGULAR_HDR: usize = 8;
    const REGULAR_ENTRIES: usize = (PAGE_SIZE - REGULAR_HDR) / 8;
    struct Page {
        start: usize,
        end: usize,
        compressed: bool,
        encodings: Vec<u32>,
    }
    let mut pages: Vec<Page> = Vec::new();
    let mut start = 0;
    while start < records.len() {
        let first_addr = func_addr(&records[start]);
        let mut encs: Vec<u32> = Vec::new();
        let mut n = 0;
        let mut i = start;
        while i < records.len() {
            let rec = &records[i];
            let is_common = common_idx.contains_key(&rec.encoding);
            let new_enc = !is_common && !encs.contains(&rec.encoding);
            if new_enc && common.len() + encs.len() + 1 > 256 {
                break;
            }
            let encs_len = encs.len() + new_enc as usize;
            if COMPRESSED_HDR + (n + 1) * 4 + encs_len * 4 > PAGE_SIZE {
                break;
            }
            if func_addr(rec) - first_addr >= (1 << 24) {
                break;
            }
            if new_enc {
                encs.push(rec.encoding);
            }
            n += 1;
            i += 1;
        }
        let regular = (records.len() - start).min(REGULAR_ENTRIES);
        if n >= regular {
            pages.push(Page { start, end: start + n, compressed: true, encodings: encs });
            start += n;
        } else {
            pages.push(Page {
                start,
                end: start + regular,
                compressed: false,
                encodings: Vec::new(),
            });
            start += regular;
        }
    }

    // The LSDA index lists a record's LSDA only if its encoding says it
    // has one (UNWIND_HAS_LSDA), as ld-prime does; a record with an
    // LSDA is kept apart from its neighbors above all the same.
    let listed_lsda = |r: &UnwindRecord| r.lsda().filter(|_| r.encoding & UNWIND_HAS_LSDA != 0);
    let num_lsda = records.iter().filter(|r| listed_lsda(r).is_some()).count();

    // Compute the layout of the section. ld-prime sizes the first-level
    // index before it picks page formats, for the most pages the records
    // could take (all regular) plus the terminator and one spare, and
    // leaves the entries it doesn't use zero.
    let common_off = 28;
    let personality_off = common_off + common.len() * 4;
    let page1_off = personality_off + personalities.len() * 4;
    let index_len = (records.len().div_ceil(REGULAR_ENTRIES) + 2) * 12;
    let lsda_off = page1_off + index_len;
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
                if let Some((isec, off)) = listed_lsda(rec) {
                    push32(&mut lsda, func_addr(rec).wrapping_sub(base) as u32);
                    push32(&mut lsda, (ctx.isec_addr(isec) + off as u64).wrapping_sub(base) as u32);
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
                    push32(&mut page2, func_addr(rec).wrapping_sub(base) as u32);
                    push32(&mut page2, rec.encoding);
                }
            }
            PageOut { page2, lsda, first: func_addr(&span[0]).wrapping_sub(base) as u32 }
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
        // ld-prime ends each page at an 8-byte boundary of the section.
        let end = page2_off + page2.len();
        page2.resize(page2.len() + end.next_multiple_of(8) - end, 0);
    }

    // The terminating first-level entry.
    push32(&mut page1, (end + 1).wrapping_sub(base) as u32);
    push32(&mut page1, 0);
    push32(&mut page1, (lsda_off + lsda.len()) as u32);
    page1.resize(index_len, 0);

    buf.extend_from_slice(&page1);
    buf.extend_from_slice(&lsda);
    buf.extend_from_slice(&page2);
    (buf, personalities)
}

/// Whether the image has __unwind_info: ld-prime writes it for any
/// unwind info, an FDE of a function that gets no entry of its own
/// (not being code) or a CIE no FDE points at too, listing each code
/// subsection then.
pub fn is_needed<E: Target>(ctx: &Context<E>) -> bool {
    let has_eh_frame = || !ctx.fdes.is_empty() || ctx.cies.iter().any(|c| ctx.keeps_lone_cie(c));
    !ctx.unwind_records.is_empty()
        || (has_eh_frame() && ctx.isecs.par_iter().any(|isec| is_code_subsec(ctx, isec)))
}

/// Whether __unwind_info covers addresses outside __TEXT, its own
/// segment: code in another segment (a -rename_section can move __text
/// out), a function with unwind info in a section of data (ld-prime
/// warns, but gives it its entry all the same), or an LSDA there.
/// Those are not final when the section is first encoded, with __TEXT.
pub fn covers_other_segments<E: Target>(ctx: &Context<E>) -> bool {
    let segname = ctx.unwind_info.hdr.segname;
    let code_elsewhere = ctx.chunks.iter().map(|&id| ctx.chunk_header(id)).any(|hdr| {
        hdr.segname != segname
            && hdr.flags & (S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS) != 0
    });
    let elsewhere = |isec: usize| {
        ctx.isecs[isec].output_section().is_some_and(|id| ctx.chunk_header(id).segname != segname)
    };
    code_elsewhere
        || ctx.unwind_records.par_iter().any(|rec| {
            elsewhere(rec.isec as usize)
                || function_lsda(ctx, rec).is_some_and(|(isec, _)| elsewhere(isec))
        })
}

/// The personality routine of a record's function: the record's own,
/// or for one in DWARF mode, its FDE's CIE's.
pub(crate) fn function_personality<E: Target>(
    ctx: &Context<E>,
    rec: &UnwindRecord,
) -> Option<SymbolId> {
    rec.personality().or_else(|| ctx.cies[ctx.fdes[rec.fde()?].cie as usize].personality)
}

/// The LSDA of a record's function: the record's own, or for one in
/// DWARF mode, its FDE's.
pub(crate) fn function_lsda<E: Target>(
    ctx: &Context<E>,
    rec: &UnwindRecord,
) -> Option<(usize, u32)> {
    rec.lsda().or_else(|| {
        let (isec, off) = ctx.fdes[rec.fde()?].lsda?;
        Some((isec as usize, off))
    })
}

/// Records for the code that has no unwind information: ld-prime gives
/// every subsection of a code section - an output section of pure
/// instructions, not one the assembler marked as holding some - an
/// entry, encoding 0 ("none") for one without a record of its own, so
/// that it does not fall under the unwind rules of the function before
/// it - an empty subsection too, such as the empty __text of an object
/// with only data.
///
/// A record anywhere in a subsection is the subsection's: its start
/// then gets no entry, and the code ahead of the record falls under the
/// entry before. To ld-prime an alternate entry point starts a
/// subsection of its own, so a record at one is not that of mold's
/// subsection holding it. Without MH_SUBSECTIONS_VIA_SYMBOLS a section
/// is one subsection, but ld-prime still splits it at its labels (see
/// unsplit_bare_subsecs).
fn bare_code_records<E: Target>(ctx: &Context<E>, records: &[UnwindRecord]) -> Vec<UnwindRecord> {
    use std::collections::HashMap;

    // Where each subsection's first record is, and for a section of an
    // object without subsections, where each of its records is and how
    // much code it spans.
    let mut first: HashMap<u32, u32> = HashMap::new();
    let mut unsplit: HashMap<u32, Vec<(u32, u32)>> = HashMap::new();
    for rec in records {
        let off = first.entry(rec.isec).or_insert(rec.input_offset);
        *off = (*off).min(rec.input_offset);
        if !ctx.objs[ctx.isecs[rec.isec as usize].file as usize].subsections_via_symbols {
            unsplit.entry(rec.isec).or_default().push((rec.input_offset, rec.code_len));
        }
    }

    ctx.isecs
        .par_iter()
        .enumerate()
        .filter(|&(_, isec)| is_code_subsec(ctx, isec))
        .flat_map_iter(|(i, isec)| {
            let i = i as u32;
            let obj = &ctx.objs[isec.file as usize];
            let pieces = if obj.subsections_via_symbols {
                let bare = match first.get(&i) {
                    None => true,
                    Some(0) => false,
                    Some(&off) => first_alt_entry(obj, isec) <= off,
                };
                if bare { vec![(0, isec.size)] } else { Vec::new() }
            } else {
                unsplit_bare_subsecs(obj, isec, unsplit.get(&i).map_or(&[], Vec::as_slice))
            };
            pieces.into_iter().map(move |(off, size)| bare_record(i, off, size))
        })
        .collect()
}

/// The record of a piece of code with no unwind information.
fn bare_record(isec: u32, off: u32, size: u32) -> UnwindRecord {
    use crate::input_files::UNWIND_NONE;
    UnwindRecord {
        isec,
        input_offset: off,
        code_len: size,
        encoding: 0,
        personality_sym: UNWIND_NONE,
        lsda_isec: UNWIND_NONE,
        lsda_off: 0,
        fde_idx: UNWIND_NONE,
    }
}

/// The offset of the first alternate entry point (N_ALT_ENTRY) inside
/// a subsection, or u32::MAX if it has none.
fn first_alt_entry(obj: &crate::input_files::ObjectFile, isec: &InputSection) -> u32 {
    let lo = isec.input_addr as u64;
    obj.nlists
        .iter()
        .filter(|n| {
            !n.is_stab()
                && n.n_type() == N_SECT
                && n.n_desc & N_ALT_ENTRY != 0
                && n.n_sect as u32 == isec.shndx + 1
                && lo < n.n_value
                && n.n_value < lo + isec.size as u64
        })
        .map(|n| (n.n_value - lo) as u32)
        .min()
        .unwrap_or(u32::MAX)
}

/// The subsections ld-prime makes of a section of an object without
/// MH_SUBSECTIONS_VIA_SYMBOLS, as (offset, size), that have none of the
/// records of `records` (offset, length). ld-prime splits such a
/// section at each label past its start, an alternate entry point's
/// too, as if symbols split it: the labels at its start name its first
/// subsection, and of several labels at one place, all but the last
/// name empty subsections (all do at its end).
///
/// ld-prime goes by the labels alone, so a label inside a function,
/// within the length of the function's record, gets encoding 0 too,
/// and the code past it can't be unwound. Such a label gets no entry
/// here, leaving the record the whole of its code (a section that
/// can't be split is one function there).
fn unsplit_bare_subsecs(
    obj: &crate::input_files::ObjectFile,
    isec: &InputSection,
    records: &[(u32, u32)],
) -> Vec<(u32, u32)> {
    let lo = isec.input_addr as u64;
    let mut labels: Vec<u32> = obj
        .nlists
        .iter()
        .filter(|n| {
            !n.is_stab()
                && n.n_type() == N_SECT
                && n.n_sect as u32 == isec.shndx + 1
                && lo < n.n_value
                && n.n_value <= lo + isec.size as u64
        })
        .map(|n| (n.n_value - lo) as u32)
        .collect();
    labels.sort_unstable();
    let mut records = records.to_vec();
    records.sort_unstable();
    // How far the records up to each one reach.
    let reach: Vec<u64> = records
        .iter()
        .scan(0, |end, &(off, len)| {
            *end = (*end).max(off as u64 + len as u64);
            Some(*end)
        })
        .collect();

    // A subsection has the records from its start to the next label;
    // the last one, those at the section's end too.
    (0..=labels.len())
        .filter_map(|k| {
            let start = if k == 0 { 0 } else { labels[k - 1] };
            let next = labels.get(k).copied();
            let i = records.partition_point(|&(off, _)| off < start);
            let in_function = i > 0 && reach[i - 1] > start as u64;
            let has_record = records.get(i).is_some_and(|&(off, _)| next.is_none_or(|n| off < n));
            (!has_record && !in_function).then(|| (start, next.unwrap_or(isec.size) - start))
        })
        .collect()
}

/// Whether a subsection is one of a code section, which ld-prime gives
/// an entry whatever its unwind info (see bare_code_records): an
/// input's, a -sectcreate option's, or the empty one it keeps
/// __dyld_lazy_load alive from.
fn is_code_subsec<E: Target>(ctx: &Context<E>, isec: &crate::input_sections::InputSection) -> bool {
    let is_own = |id: u32| ctx.isecs.get(id as usize).is_some_and(|k| std::ptr::eq(k, isec));
    let is_sectcreate = || {
        (ctx.sectcreate_inputs.iter())
            .any(|a| matches!(a.place, InputPlace::Isec(id) if is_own(id)))
    };
    isec.is_alive()
        && isec.replacement == crate::input_sections::NO_REPLACEMENT
        && (!ctx.is_internal(isec.file as usize)
            || is_own(ctx.lazy_helpers.keep_alive)
            || is_sectcreate())
        && isec
            .output_section()
            .is_some_and(|id| ctx.chunk_header(id).flags & S_ATTR_PURE_INSTRUCTIONS != 0)
}
