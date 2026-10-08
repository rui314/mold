//! __TEXT,__unwind_info: the compact unwind table, generated from the
//! objects' __compact_unwind records.

use rayon::prelude::*;

use crate::arch::Target;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::input_files::UnwindRecord;
use crate::input_sections::InputSection;
use crate::macho::*;
use crate::symbol::SymbolId;

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
    let mut records = table_records(ctx);
    if records.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let personalities = assign_personalities(&mut records);

    // The table ends where the last function does, as in ld64, however
    // much of it its unwind record covers.
    let last = records.last().unwrap();
    let end = ctx.isec_addr(last.isec as usize) + ctx.isecs[last.isec as usize].size as u64;

    merge_records::<E>(&mut records);
    let pages = split_pages(ctx, &records);
    (write_table(ctx, &records, &personalities, &pages, end), personalities)
}

/// The address of a record's function.
fn func_addr<E: Target>(ctx: &Context<E>, rec: &UnwindRecord) -> u64 {
    ctx.isec_addr(rec.isec as usize) + rec.input_offset as u64
}

/// The records the table lists, sorted by address: those of the live
/// functions and those of the code with no unwind information (see
/// bare_code_records).
fn table_records<E: Target>(ctx: &Context<E>) -> Vec<UnwindRecord> {
    let mut records: Vec<UnwindRecord> = ctx
        .unwind_records
        .par_iter()
        .filter(|rec| {
            let isec = &ctx.isecs[rec.isec as usize];
            isec.is_emitted()
        })
        .cloned()
        .collect();
    records.extend(bare_code_records(ctx));

    // A DWARF-mode record's encoding holds its FDE's offset in
    // __eh_frame in the low 24 bits, or 0 if they can't hold it, as in
    // ld-prime (which warns, see lay_out_eh_frame): the unwinder then
    // looks for the FDE through the whole section. It takes the
    // personality and the LSDA from the FDE.
    for rec in &mut records {
        if let Some(fde) = rec.fde() {
            let off = ctx.fdes[fde].output_offset;
            rec.encoding = E::UNWIND_MODE_DWARF | if off <= MAX_FDE_OFFSET { off } else { 0 };
        }
    }

    // Of the entries of one address, the unwinder finds the last: an
    // empty subsection's of encoding 0 comes before the function
    // sharing its address, and of two records for a function (or one
    // at a section's end and the next section's first), the greater
    // encoding comes last.
    records.par_sort_by_key(|r| (func_addr(ctx, r), r.encoding));
    records
}

/// Gives each record with a personality routine the routine's 1-based
/// index in the table's array of them, in bits 28-29 of its encoding,
/// the routines indexed in order of first use by address; returns the
/// array.
fn assign_personalities(records: &mut [UnwindRecord]) -> Vec<SymbolId> {
    let mut personalities: Vec<SymbolId> = Vec::new();
    for rec in records {
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
    personalities
}

/// Merges consecutive records with identical contents. An entry has no
/// length - it covers the code up to the next one - so the padding
/// between two functions does not keep them apart. An x86-64 entry in
/// "stack immediate indirect" mode gives where the stack size is in the
/// function as an offset from the entry's start, so it can't cover a
/// second function (ld64's encodingCannotBeMerged).
fn merge_records<E: Target>(records: &mut Vec<UnwindRecord>) {
    let stack_ind = |enc: u32| {
        E::CPUTYPE == CPU_TYPE_X86_64 && enc & UNWIND_MODE_MASK == UNWIND_X86_64_MODE_STACK_IND
    };
    records.dedup_by(|rec, last| {
        !stack_ind(rec.encoding)
            && last.encoding == rec.encoding
            && last.personality() == rec.personality()
            && last.lsda().is_none()
            && rec.lsda().is_none()
    });
}

/// A second-level page's size limit, and the size of its header.
const PAGE_SIZE: usize = 4096;
const PAGE_HDR: usize = 12;

/// A compressed second-level page: a run of the table's records, and
/// their encodings in order of first use.
struct Page {
    records: std::ops::Range<usize>,
    encodings: Vec<u32>,
}

/// Splits the records into compressed second-level pages, of 32-bit
/// entries: a 24-bit offset from the page's first function and an 8-bit
/// index into the page's encodings, listed after the entries. A page
/// ends at 4096 bytes, at 2^24 bytes of code, or at 256 encodings.
fn split_pages<E: Target>(ctx: &Context<E>, records: &[UnwindRecord]) -> Vec<Page> {
    let mut pages = Vec::new();
    let mut start = 0;
    while start < records.len() {
        let first_addr = func_addr(ctx, &records[start]);
        let mut encs: Vec<u32> = Vec::new();
        let mut i = start;
        while i < records.len() {
            let enc = records[i].encoding;
            let new_enc = !encs.contains(&enc) as usize;
            if encs.len() + new_enc > 256
                || PAGE_HDR + (i - start + 1 + encs.len() + new_enc) * 4 > PAGE_SIZE
                || func_addr(ctx, &records[i]) - first_addr >= 1 << 24
            {
                break;
            }
            if new_enc == 1 {
                encs.push(enc);
            }
            i += 1;
        }
        pages.push(Page { records: start..i, encodings: encs });
        start = i;
    }
    pages
}

/// Writes the table: the header, no common encodings, the
/// personalities (zeros, which copy_buf patches), the first-level index
/// (a row per page and a terminator, at `end`), the LSDA index, then the
/// pages.
fn write_table<E: Target>(
    ctx: &Context<E>,
    records: &[UnwindRecord],
    personalities: &[SymbolId],
    pages: &[Page],
    end: u64,
) -> Vec<u8> {
    let base = ctx.mach_header.hdr.addr;
    let num_lsda = records.iter().filter(|r| r.lsda().is_some()).count();
    let personality_off = 28;
    let page1_off = personality_off + personalities.len() * 4;
    let lsda_off = page1_off + (pages.len() + 1) * 12;
    let page2_off = lsda_off + num_lsda * 8;

    let push32 = |buf: &mut Vec<u8>, val: u32| buf.extend_from_slice(&val.to_le_bytes());

    let mut buf = Vec::new();
    push32(&mut buf, UNWIND_SECTION_VERSION);
    push32(&mut buf, personality_off as u32);
    push32(&mut buf, 0);
    push32(&mut buf, personality_off as u32);
    push32(&mut buf, personalities.len() as u32);
    push32(&mut buf, page1_off as u32);
    push32(&mut buf, pages.len() as u32 + 1);

    // Personalities are image-relative pointers to the functions' GOT
    // slots, patched in by copy_buf.
    for &_sym in personalities {
        push32(&mut buf, 0);
    }

    // Each second-level page and its LSDA rows depend only on its own
    // records, so the pages are encoded in parallel; the first-level
    // index is then a serial walk over their lengths.
    let outs: Vec<EncodedPage> =
        pages.par_iter().map(|page| encode_page(ctx, records, page)).collect();

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
    push32(&mut page1, (end + 1).wrapping_sub(base) as u32);
    push32(&mut page1, 0);
    push32(&mut page1, (lsda_off + lsda.len()) as u32);

    buf.extend_from_slice(&page1);
    buf.extend_from_slice(&lsda);
    buf.extend_from_slice(&page2);
    buf
}

/// A second-level page, encoded, and its rows of the LSDA index
/// (function, LSDA), each an image offset.
struct EncodedPage {
    page2: Vec<u8>,
    lsda: Vec<u8>,
    /// The image offset of the page's first function.
    first: u32,
}

/// Encodes a compressed second-level page of the table's `records`.
fn encode_page<E: Target>(ctx: &Context<E>, records: &[UnwindRecord], page: &Page) -> EncodedPage {
    let base = ctx.mach_header.hdr.addr;
    let span = &records[page.records.clone()];
    let encs = &page.encodings;
    let push32 = |buf: &mut Vec<u8>, val: u32| buf.extend_from_slice(&val.to_le_bytes());
    let push16 = |buf: &mut Vec<u8>, val: u16| buf.extend_from_slice(&val.to_le_bytes());

    let mut lsda = Vec::new();
    for rec in span {
        if let Some((isec, off)) = rec.lsda() {
            push32(&mut lsda, func_addr(ctx, rec).wrapping_sub(base) as u32);
            push32(&mut lsda, (ctx.isec_addr(isec) + off as u64).wrapping_sub(base) as u32);
        }
    }

    let mut page2 = Vec::new();
    push32(&mut page2, UNWIND_SECOND_LEVEL_COMPRESSED);
    push16(&mut page2, PAGE_HDR as u16); // entries offset
    push16(&mut page2, span.len() as u16);
    push16(&mut page2, (PAGE_HDR + span.len() * 4) as u16); // encodings offset
    push16(&mut page2, encs.len() as u16);
    let page_base = func_addr(ctx, &span[0]);
    for rec in span {
        let idx = encs.iter().position(|&e| e == rec.encoding).unwrap() as u32;
        push32(&mut page2, (func_addr(ctx, rec) - page_base) as u32 | idx << 24);
    }
    for &enc in encs {
        push32(&mut page2, enc);
    }
    EncodedPage { page2, lsda, first: page_base.wrapping_sub(base) as u32 }
}

/// Whether the image has __unwind_info: whether any of its functions
/// has unwind info.
pub fn is_needed<E: Target>(ctx: &Context<E>) -> bool {
    !ctx.unwind_records.is_empty()
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

/// Records for the code that has no unwind information: every
/// subsection of a code section - an output section of pure
/// instructions, not one the assembler marked as holding some - gets an
/// entry, encoding 0 ("none") where no record of its own starts, so that
/// it does not fall under the unwind rules of the function before it -
/// an empty subsection too, such as the empty __text of an object with
/// only data. A section of an object without MH_SUBSECTIONS_VIA_SYMBOLS
/// is one subsection of several functions: the code past each record's
/// length up to the next record has no unwind information either.
///
/// Each subsection is looked at on its own, with its records (its range
/// of ctx.unwind_records), as sold reads a subsection's unwind records.
fn bare_code_records<E: Target>(ctx: &Context<E>) -> Vec<UnwindRecord> {
    ctx.isecs
        .par_iter()
        .enumerate()
        .filter(|&(_, isec)| is_code_subsec(ctx, isec))
        .flat_map_iter(|(i, isec)| {
            let start = isec.unwind_offset as usize;
            let recs = &ctx.unwind_records[start..start + isec.nunwind as usize];
            let unsplit =
                if ctx.objs[isec.file as usize].subsections_via_symbols { &[][..] } else { recs };
            let ends = (unsplit.iter())
                .map(|rec| rec.input_offset + rec.code_len)
                .filter(move |&off| off < isec.size);
            std::iter::once(0)
                .chain(ends)
                .filter(move |&off| !recs.iter().any(|rec| rec.input_offset == off))
                .map(move |off| bare_record(i as u32, off))
        })
        .collect()
}

/// The record of a piece of code with no unwind information.
fn bare_record(isec: u32, off: u32) -> UnwindRecord {
    use crate::input_files::UNWIND_NONE;
    UnwindRecord {
        isec,
        input_offset: off,
        code_len: 0,
        encoding: 0,
        personality_sym: UNWIND_NONE,
        lsda_isec: UNWIND_NONE,
        lsda_off: 0,
        fde_idx: UNWIND_NONE,
    }
}

/// Whether a subsection is one of a code section, which gets an entry
/// whatever its unwind info (see bare_code_records).
fn is_code_subsec<E: Target>(ctx: &Context<E>, isec: &InputSection) -> bool {
    isec.is_emitted()
        && isec
            .output_section()
            .is_some_and(|id| ctx.chunk_header(id).flags & S_ATTR_PURE_INSTRUCTIONS != 0)
}
