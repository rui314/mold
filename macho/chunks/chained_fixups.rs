//! The LC_DYLD_CHAINED_FIXUPS payload in __LINKEDIT: the modern
//! replacement for the rebase and bind opcode streams, with the fixup
//! chains it describes threaded through the data sections.

use rayon::prelude::*;

use crate::arch::Target;
use crate::chunks::{ChunkHeader, OutputSegment, rebase_info};
use crate::cmdline::Treatment;
use crate::context::Context;
use crate::fatal;
use crate::input_files::FileId;
use crate::macho::*;
use crate::symbol::SymbolId;

#[derive(Debug)]
pub struct ChainedFixupsSection {
    pub hdr: ChunkHeader,
    /// The encoded payload, built during layout.
    pub contents: Vec<u8>,
    /// Every dynamic fixup, sorted by address.
    pub fixups: Vec<Fixup>,
    /// The import table: (symbol, table addend), in the order the binds
    /// first name them; and each entry's index.
    pub imports: Vec<(SymbolId, u64)>,
    pub ordinals: ImportOrdinals,
    /// Set when an x86-64 image laid out for chained fixups gets classic
    /// dyld info instead, for an unaligned pointer.
    pub disabled: bool,
}

/// Each import's index in the table, by (symbol, table addend).
pub type ImportOrdinals = std::collections::HashMap<(SymbolId, u64), usize>;

impl ChainedFixupsSection {
    pub fn new() -> Self {
        Self {
            hdr: ChunkHeader::linkedit(),
            contents: Vec::new(),
            fixups: Vec::new(),
            imports: Vec::new(),
            ordinals: std::collections::HashMap::new(),
            disabled: false,
        }
    }
}

impl Default for ChainedFixupsSection {
    fn default() -> Self {
        Self::new()
    }
}

/// What build_chained_fixups makes, for the caller to store on the
/// context: the encoded payload, the fixups, the import table and each
/// import's index.
pub type ChainedFixups = (Vec<u8>, Vec<Fixup>, Vec<(SymbolId, u64)>, ImportOrdinals);

/// A fixup: its address, the symbol it binds (None for a rebase), and
/// the addend.
pub type Fixup = (u64, Option<SymbolId>, u64);

/// The import-table addend of a bind: addends up to 255 are carried
/// inline in the fixup word and share the symbol's addend-0 entry.
fn table_addend(addend: u64) -> u64 {
    if addend <= MAX_INLINE_ADDEND { 0 } else { addend }
}

/// The import table: an entry per distinct (symbol, table addend), in
/// the order the binds first name them.
fn import_table(fixups: &[Fixup]) -> (Vec<(SymbolId, u64)>, ImportOrdinals) {
    let mut imports = Vec::new();
    let mut ordinals = ImportOrdinals::new();
    for &(_, sym, addend) in fixups {
        let Some(sym) = sym else { continue };
        ordinals.entry((sym, table_addend(addend))).or_insert_with(|| {
            imports.push((sym, table_addend(addend)));
            imports.len() - 1
        });
    }
    (imports, ordinals)
}

/// The pointer format of the chains. A rebase target is a VM address
/// under DYLD_CHAINED_PTR_64 and an offset from the image's load
/// address under DYLD_CHAINED_PTR_64_OFFSET, which dyld reads from
/// macOS 12 and iOS 15 on. ld-prime writes the latter for such a (or a
/// firmware) target on every architecture and output kind, and for a
/// -static image, which no dyld reads, whatever its target; the former
/// where chains came earlier (iOS 13.4, see macho::is_new_os) or when
/// -fixup_chains forces them on an older OS.
pub(crate) fn pointer_format<E: Target>(ctx: &Context<E>) -> u16 {
    if ctx.args.static_link || ctx.args.targets(&crate::macho::VERSION_2021_FALL) {
        DYLD_CHAINED_PTR_64_OFFSET
    } else {
        DYLD_CHAINED_PTR_64
    }
}

fn push16(buf: &mut Vec<u8>, val: u16) {
    buf.extend_from_slice(&val.to_le_bytes());
}

fn push32(buf: &mut Vec<u8>, val: u32) {
    buf.extend_from_slice(&val.to_le_bytes());
}

fn push64(buf: &mut Vec<u8>, val: u64) {
    buf.extend_from_slice(&val.to_le_bytes());
}

fn pad(buf: &mut Vec<u8>, align: usize) {
    buf.resize(buf.len().next_multiple_of(align), 0);
}

/// Builds the LC_DYLD_CHAINED_FIXUPS payload. Instead of opcode
/// streams, chained fixups store, per page of each segment, the offset
/// of the first fixup; each 64-bit fixup word in the data itself then
/// encodes its target (a rebase value or an import ordinal) plus the
/// distance to the next fixup in the page, forming a chain dyld walks.
/// The payload is a header, the starts of the chains, the import table
/// and the imports' names. Returns None if the image must have classic
/// dyld info instead (see check_pointer_alignment).
pub fn build_chained_fixups<E: Target>(ctx: &Context<E>) -> Option<ChainedFixups> {
    // An image with nothing to fix up still gets the payload (a
    // header and a starts table with no pages), as ld64 writes it:
    // dyld reads the format from the load command, and its absence
    // would mean classic dyld info.
    let (fixups, unaligned) = collect_fixups(ctx);
    if !check_pointer_alignment(ctx, &fixups, unaligned, true) {
        return None;
    }
    let (imports, ordinals) = import_table(&fixups);
    let format = import_format(&imports);

    let mut buf = Vec::new();
    // dyld_chained_fixups_header; the offsets are backpatched.
    push32(&mut buf, 0); // fixups_version
    push32(&mut buf, 0); // starts_offset
    push32(&mut buf, 0); // imports_offset
    push32(&mut buf, 0); // symbols_offset
    push32(&mut buf, imports.len() as u32);
    push32(&mut buf, format);
    push32(&mut buf, 0); // symbols_format: uncompressed
    pad(&mut buf, 8);

    let starts_offset = buf.len();
    buf[4..8].copy_from_slice(&(starts_offset as u32).to_le_bytes());
    write_starts_in_image(ctx, &mut buf, &fixups);

    // The import table, aligned only as its entries need: 4 bytes, or 8
    // for 64-bit addends.
    pad(&mut buf, if format == DYLD_CHAINED_IMPORT_ADDEND64 { 8 } else { 4 });
    let imports_offset = buf.len();
    buf[8..12].copy_from_slice(&(imports_offset as u32).to_le_bytes());
    write_imports(ctx, &mut buf, &imports, format);

    // The imports' names, after a leading NUL.
    let symbols_offset = buf.len();
    buf[12..16].copy_from_slice(&(symbols_offset as u32).to_le_bytes());
    buf.push(0);
    for &(sym, _) in &imports {
        buf.extend_from_slice(ctx.symbols[sym].name());
        buf.push(0);
    }
    pad(&mut buf, 8);

    Some((buf, fixups, imports, ordinals))
}

/// The import table's format: the narrowest whose entries hold every
/// import's table addend.
fn import_format(imports: &[(SymbolId, u64)]) -> u32 {
    let max_addend = imports.iter().map(|&(_, a)| a).max().unwrap_or(0);
    if max_addend == 0 {
        DYLD_CHAINED_IMPORT
    } else if imports.iter().all(|&(_, a)| i32::try_from(a as i64).is_ok()) {
        DYLD_CHAINED_IMPORT_ADDEND
    } else {
        DYLD_CHAINED_IMPORT_ADDEND64
    }
}

/// Appends dyld_chained_starts_in_image: an entry per segment command,
/// the offset of the segment's starts table, which follow, each
/// 8-aligned, with the offset of each page's first fixup.
fn write_starts_in_image<E: Target>(ctx: &Context<E>, buf: &mut Vec<u8>, fixups: &[Fixup]) {
    let starts_offset = buf.len();
    // One per segment command: a -preload image's __LINKEDIT has none.
    let seg_count = ctx.segments.len() - usize::from(ctx.args.preload);
    push32(buf, seg_count as u32);
    let seg_info_table = buf.len();
    for _ in 0..seg_count {
        push32(buf, 0);
    }

    // A segment's offset counts from the image's own address, which
    // -image_base may move.
    let image_base = ctx.mach_header.hdr.addr;
    for (seg_idx, seg) in ctx.segments.iter().enumerate() {
        let fx = segment_fixups(fixups, seg);
        if fx.is_empty() {
            continue;
        }

        pad(buf, 8);
        let off = buf.len() - starts_offset;
        let ent = seg_info_table + seg_idx * 4;
        buf[ent..ent + 4].copy_from_slice(&(off as u32).to_le_bytes());

        // dyld reads chains only in 4 KiB or 16 KiB pages, which an
        // arm64 image's -segalign sets.
        let page_size = chain_page_size(ctx);
        if !matches!(page_size, 0x1000 | 0x4000) {
            crate::error!(
                "chained fixups need a -segalign of 0x1000 or 0x4000, not {page_size:#x}"
            );
            break;
        }
        let npages = ((fx.last().unwrap().0 + 1 - seg.cmd.vmaddr).div_ceil(page_size)) as usize;
        // The record is 22 bytes of fields plus one u16 per page; its
        // size counts just those, without padding.
        push32(buf, 22 + npages as u32 * 2);
        push16(buf, page_size as u16);
        push16(buf, pointer_format(ctx));
        // A layout in error may put a segment below the image base; the
        // table is never written then.
        push64(buf, seg.cmd.vmaddr.wrapping_sub(image_base));
        push32(buf, 0); // max_valid_pointer
        push16(buf, npages as u16);
        let mut j = 0;
        for i in 0..npages {
            let page_addr = seg.cmd.vmaddr + i as u64 * page_size;
            while j < fx.len() && fx[j].0 < page_addr {
                j += 1;
            }
            if j < fx.len() && fx[j].0 < page_addr + page_size {
                push16(buf, (fx[j].0 - page_addr) as u16);
            } else {
                push16(buf, DYLD_CHAINED_PTR_START_NONE);
            }
        }
    }
}

/// Appends the import table's entries in `format`. Each import names a
/// string of its own, repeated for a symbol imported with several
/// addends.
fn write_imports<E: Target>(
    ctx: &Context<E>,
    buf: &mut Vec<u8>,
    imports: &[(SymbolId, u64)],
    format: u32,
) {
    // The names follow a leading NUL.
    let mut name_off: u32 = 1;
    for &(sym, addend) in imports {
        let weak = ctx.symbols[sym].is_weak_ref() as u32;
        match format {
            DYLD_CHAINED_IMPORT => {
                let ordinal = import_ordinal(ctx, sym, 8) as u32;
                push32(buf, ordinal | (weak << 8) | (name_off << 9));
            }
            DYLD_CHAINED_IMPORT_ADDEND => {
                let ordinal = import_ordinal(ctx, sym, 8) as u32;
                push32(buf, ordinal | (weak << 8) | (name_off << 9));
                push32(buf, addend as u32);
            }
            _ => {
                let ordinal = import_ordinal(ctx, sym, 16);
                push64(buf, ordinal | ((weak as u64) << 16) | ((name_off as u64) << 32));
                push64(buf, addend);
            }
        }
        name_off += ctx.symbols[sym].name().len() as u32 + 1;
    }
}

/// The library ordinal an import's entry holds, in its `bits`. An
/// import names its dylib; one of this image's own weak definitions is
/// bound by weak lookup (ordinal -3), which makes dyld search every
/// loaded image for the coalesced winner, a class bound to the image
/// itself names it (0), and an interposable export is a flat lookup (-2)
/// under -flat_namespace and the image itself (0) otherwise.
fn import_ordinal<E: Target>(ctx: &Context<E>, sym: SymbolId, bits: u32) -> u64 {
    let special = |ordinal: i32| (ordinal as i64 as u64) & ((1u64 << bits) - 1);
    match ctx.symbols[sym].file() {
        Some(FileId::Dylib(dylib)) if !ctx.binds_weak_lookup(sym) => {
            ctx.chained_import_ordinal(dylib, bits)
        }
        _ if ctx.binds_to_self(sym) => BIND_SPECIAL_DYLIB_SELF as u64,
        _ if ctx.is_interposable_export(sym) && !ctx.binds_weak_lookup(sym) => {
            special(ctx.export_bind_ordinal())
        }
        _ if ctx.is_dtrace_pointer_target(sym) => special(BIND_SPECIAL_DYLIB_FLAT_LOOKUP),
        _ => special(BIND_SPECIAL_DYLIB_WEAK_LOOKUP),
    }
}

/// The pages the fixup chains of a segment are cut into, from its start:
/// a chain stays in one page, which dyld takes to be 4 KiB or 16 KiB
/// long. An arm64 image's are its segment alignment (no other is
/// accepted with fixups), an x86-64 image's 4 KiB whatever its segment
/// alignment.
fn chain_page_size<E: Target>(ctx: &Context<E>) -> u64 {
    if E::CPUTYPE == CPU_TYPE_X86_64 { 0x1000 } else { ctx.args.segment_align }
}

/// The farthest a fixup can be from the next one of its chain: the
/// 12-bit `next` field counts 4-byte strides.
const MAX_CHAIN_STRIDE: u64 = 0xfff * 4;

/// The chains of an image whose starts __TEXT,__chain_starts lists
/// (-fixup_chains_section): each segment's fixups chain on from the
/// first, a chain ending where the next fixup is out of its stride's
/// reach - none is cut at a page, as dyld would need. Returns each
/// chain's start, an offset from the image's address.
pub fn section_chain_starts<E: Target>(ctx: &Context<E>) -> Vec<u32> {
    // The first fixups of the chains of those in [start, end).
    fn chains(addrs: &[u64], start: u64, end: u64) -> impl Iterator<Item = u64> + '_ {
        let lo = addrs.partition_point(|&a| a < start);
        let hi = addrs.partition_point(|&a| a < end);
        (lo..hi)
            .filter(move |&i| i == lo || addrs[i] - addrs[i - 1] > MAX_CHAIN_STRIDE)
            .map(|i| addrs[i])
    }
    let (fixups, _) = collect_fixups(ctx);
    let addrs: Vec<u64> = fixups.iter().map(|&(addr, ..)| addr).collect();
    let base = ctx.mach_header.hdr.addr;
    (ctx.segments.iter())
        .flat_map(|seg| chains(&addrs, seg.cmd.vmaddr, seg.cmd.vmaddr + seg.cmd.vmsize))
        .map(|addr| addr.wrapping_sub(base) as u32)
        .collect()
}

/// Writes the fixup chains into the copied output: every fixup word is
/// rewritten to encode its payload plus the 4-byte-stride distance to
/// the next fixup of its chain, in the same page (or, under
/// -fixup_chains_section, in its stride's reach; see
/// section_chain_starts).
pub fn write_fixup_chains<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let page_shift = chain_page_size(ctx).trailing_zeros();
    // What a rebase target counts from: zero for a VM address, the
    // image's own address for an offset.
    let target_base = match pointer_format(ctx) {
        DYLD_CHAINED_PTR_64_OFFSET => ctx.mach_header.hdr.addr,
        _ => 0,
    };

    for seg in &ctx.segments {
        let fx = segment_fixups(&ctx.chained_fixups.fixups, seg);
        // Pages count from the segment's start.
        let page = |addr: u64| (addr - seg.cmd.vmaddr) >> page_shift;
        let chains_to = |addr: u64, next: u64| {
            if ctx.args.fixup_chains_section {
                next - addr <= MAX_CHAIN_STRIDE
            } else {
                page(next) == page(addr)
            }
        };

        for (i, &(addr, sym, addend)) in fx.iter().enumerate() {
            let next = match fx.get(i + 1) {
                Some(&(next_addr, _, _)) if chains_to(addr, next_addr) => (next_addr - addr) / 4,
                _ => 0,
            };
            if addr % 4 != 0 {
                fatal!("unaligned fixup; re-link with -no_fixup_chains");
            }

            let off = (seg.cmd.fileoff + (addr - seg.cmd.vmaddr)) as usize;
            let word = match sym {
                Some(sym) => {
                    // dyld_chained_ptr_64_bind
                    let ordinal = ctx.chained_fixups.ordinals[&(sym, table_addend(addend))] as u64;
                    let inline_addend = if addend <= MAX_INLINE_ADDEND { addend } else { 0 };
                    ordinal | (inline_addend << 24) | (next << 51) | (1 << 63)
                }
                None => {
                    let val = u64::from_le_bytes(buf[off..off + 8].try_into().unwrap());
                    rebase_word(ctx, addr, val, target_base) | (next << 51)
                }
            };
            buf[off..off + 8].copy_from_slice(&word.to_le_bytes());
        }
    }
}

/// The fixups, sorted by address, that lie in a segment.
fn segment_fixups<'a>(fixups: &'a [Fixup], seg: &OutputSegment) -> &'a [Fixup] {
    let lo = fixups.partition_point(|&(a, _, _)| a < seg.cmd.vmaddr);
    let hi = fixups.partition_point(|&(a, _, _)| a < seg.cmd.vmaddr + seg.cmd.vmsize);
    &fixups[lo..hi]
}

/// A rebase's dyld_chained_ptr_64_rebase word, but for the link to the
/// next fixup: the pointer at `addr` holds `val`, the absolute target
/// address with its top byte (high8), which the word carries apart;
/// the target counts from `target_base`, and must fit in 36 bits.
fn rebase_word<E: Target>(ctx: &Context<E>, addr: u64, val: u64, target_base: u64) -> u64 {
    let high8 = val >> 56;
    let target = (val & 0x00ff_ffff_ffff_ffff).wrapping_sub(target_base);
    if target >> 36 != 0 {
        let sect = ctx
            .chunks
            .iter()
            .map(|&id| ctx.chunk_header(id))
            .find(|hdr| hdr.addr <= addr && addr < hdr.addr + hdr.size)
            .map(|hdr| [hdr.segname, b",", hdr.sectname].concat())
            .unwrap_or_default();
        fatal!(
            "rebase target unencodable at {addr:#x} in {} (value {val:#x}); re-link with -no_fixup_chains",
            crate::error::raw(&sect)
        );
    }
    target | (high8 << 36)
}

/// Collects the fixups, sorted by address, and those of subsections at
/// an address no multiple of 8, as (subsection, address) pairs, for
/// check_pointer_alignment.
fn collect_fixups<E: Target>(ctx: &Context<E>) -> (Vec<Fixup>, Vec<(u32, u64)>) {
    // Every subsection's fixups are independent; collect them on all
    // cores and sort the union in parallel, as mold does.
    let unaligned = std::sync::Mutex::new(Vec::new());
    let mut fixups: Vec<Fixup> = ctx
        .isecs
        .par_iter()
        .enumerate()
        .filter(|(_, isec)| isec.is_emitted())
        .flat_map_iter(|(id, isec)| {
            let unaligned = &unaligned;
            rebase_info::pointer_relocs(ctx, isec).filter_map(move |(addr, rel)| {
                let fixup = match ctx.reloc_target_sym(isec.file as usize, rel) {
                    Some(id) if ctx.is_absolute_symbol(id) && !ctx.binds_at_runtime(id) => None,
                    Some(id)
                        if ctx.binds_at_runtime(id)
                            || ctx.binds_to_self(id)
                            || ctx.is_dtrace_pointer_target(id) =>
                    {
                        Some((addr, Some(id), rel.addend as u64))
                    }
                    _ if ctx.reloc_target_is_tls(isec.file as usize, rel) => None,
                    _ => Some((addr, None, 0)),
                };
                if fixup.is_some() && !addr.is_multiple_of(8) {
                    unaligned.lock().unwrap().push((id as u32, addr));
                }
                fixup
            })
        })
        .collect();

    for (i, &id) in ctx.got.got_syms.iter().enumerate() {
        if ctx.is_absolute_symbol(id) && !ctx.binds_at_runtime(id) {
            continue;
        }
        let sym = Some(id).filter(|&id| ctx.binds_at_runtime(id));
        let slot = ctx.got.slot_addr(i);
        fixups.push((slot, sym, 0));
    }
    for i in 0..ctx.objc_stubs.symbols.len() + ctx.objc_stubs.extra_selrefs.len() {
        let slot = ctx.objc_selref_addr(i);
        fixups.push((slot, None, 0));
    }
    for (addr, _) in rebase_info::data_blob_pointers(ctx) {
        fixups.push((addr, None, 0));
    }
    for (addr, id) in rebase_info::data_blob_binds(ctx) {
        fixups.push((addr, Some(id), 0));
    }
    // The pointers of an image nothing slides keep their addresses:
    // its chains hold only binds.
    if rebase_info::is_never_slid(ctx) {
        fixups.retain(|&(_, sym, _)| sym.is_some());
    }

    fixups.par_sort_unstable_by_key(|&(addr, _, _)| addr);
    (fixups, unaligned.into_inner().unwrap())
}

/// The largest addend a chained bind can carry inline; anything bigger
/// goes into the import table.
const MAX_INLINE_ADDEND: u64 = 255;

/// Checks the pointers of an image with classic dyld info as
/// check_pointer_alignment does.
pub fn check_classic_pointers<E: Target>(ctx: &Context<E>) {
    if ctx.args.unaligned_pointers != Treatment::Suppress {
        let (fixups, unaligned) = collect_fixups(ctx);
        check_pointer_alignment(ctx, &fixups, unaligned, false);
    }
}

/// Each pointer dyld fixes up should be 8-aligned, as the links of a
/// fixup chain are words. Reports the `unaligned` ones of subsections
/// (but text relocations, which report_text_relocs reports) as
/// -unaligned_pointers says: arm64 refuses them in chains. An x86-64
/// image laid out for chains gets classic dyld info instead, on any
/// unaligned fixup, saying so; its load commands grow by 16 bytes then,
/// which the header padding holds (see header_pad). Returns false for
/// that fallback.
fn check_pointer_alignment<E: Target>(
    ctx: &Context<E>,
    fixups: &[Fixup],
    mut unaligned: Vec<(u32, u64)>,
    chained: bool,
) -> bool {
    unaligned.retain(|&(_, addr)| !ctx.text_reloc_ranges.iter().any(|r| r.contains(&addr)));
    unaligned.sort_unstable_by_key(|&(_, addr)| addr);
    let fallback = chained
        && E::CPUTYPE == CPU_TYPE_X86_64
        && !ctx.args.without_dyld()
        && (!unaligned.is_empty() || fixups.iter().any(|&(addr, ..)| !addr.is_multiple_of(8)));
    if fallback {
        crate::warn!("disabling chained fixups because of unaligned pointers");
    }
    for (id, addr) in unaligned {
        let place = ctx.subsec_ref(id as usize, (addr - ctx.isec_addr(id as usize)) as u32);
        let place = crate::error::raw(&place);
        match ctx.args.unaligned_pointers {
            Treatment::Error => {
                crate::error!("pointer not aligned at {addr:#x} in {place}");
                break;
            }
            Treatment::Warning => crate::warn!("pointer not aligned at {addr:#x} in {place}"),
            Treatment::Suppress => break,
        }
    }
    !fallback
}
