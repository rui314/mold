//! The LC_DYLD_CHAINED_FIXUPS payload in __LINKEDIT: the modern
//! replacement for the rebase and bind opcode streams, with the fixup
//! chains it describes threaded through the data sections.

use rayon::prelude::*;

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::fatal;
use crate::input_files::FileId;
use crate::macho::*;
use crate::symbol::SymbolId;
use crate::target::RelocClass;
use crate::target::Target;

#[derive(Debug)]
pub struct ChainedFixupsSection {
    pub hdr: ChunkHeader,
    /// The encoded payload, built during layout.
    pub contents: Vec<u8>,
    /// Every dynamic fixup location, sorted by address: (address,
    /// bound symbol or None for a rebase, addend).
    pub fixups: Vec<(u64, Option<SymbolId>, u64)>,
    /// The import table: (symbol, table addend), in ld-prime's order;
    /// and each entry's index.
    pub imports: Vec<(SymbolId, u64)>,
    pub ordinals: ImportOrdinals,
    /// Set when an x86-64 image laid out for chained fixups gets classic
    /// dyld info instead, for an unaligned pointer.
    pub disabled: bool,
    /// The unaligned pointers check_pointer_alignment found, reported
    /// once relocations are applied (report_unaligned_chain_pointer,
    /// warn_unaligned_pointers).
    pub unaligned: std::sync::Mutex<Vec<(u32, u64)>>,
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
            unaligned: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl Default for ChainedFixupsSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let data = &ctx.chained_fixups.contents;
    buf[..data.len()].copy_from_slice(data);
}

/// Builds the LC_DYLD_CHAINED_FIXUPS payload. Instead of opcode
/// streams, chained fixups store, per page of each segment, the offset
/// of the first fixup; each 64-bit fixup word in the data itself then
/// encodes its target (a rebase value or an import ordinal) plus the
/// distance to the next fixup in the page, forming a chain dyld walks.
/// Builds the chained-fixups payload; returns the encoded bytes, the
/// collected fixups, the import table and each import's index, for the
/// caller to store on the context.
pub type ChainedFixups = (
    Vec<u8>,
    Vec<(u64, Option<crate::symbol::SymbolId>, u64)>,
    Vec<(crate::symbol::SymbolId, u64)>,
    ImportOrdinals,
);

/// A fixup: its address, the symbol it binds (None for a rebase), the
/// addend, and the start of the atom holding it.
type Fixup = (u64, Option<SymbolId>, u64, u64);

/// The import-table addend of a bind: addends up to 255 are carried
/// inline in the fixup word and share the symbol's addend-0 entry.
fn table_addend(addend: u64) -> u64 {
    if addend <= MAX_INLINE_ADDEND { 0 } else { addend }
}

/// The import table, in ld-prime's order: an entry per distinct
/// (symbol, table addend), numbered as it is first met walking the
/// binds atom by atom in address order and, within an atom, from the
/// highest offset down. A GOT slot is an atom of its own.
fn import_table(fixups: &[Fixup]) -> (Vec<(SymbolId, u64)>, ImportOrdinals) {
    let mut binds: Vec<&Fixup> = fixups.iter().filter(|f| f.1.is_some()).collect();
    binds.sort_by(|a, b| a.3.cmp(&b.3).then(b.0.cmp(&a.0)));
    let mut imports = Vec::new();
    let mut ordinals = ImportOrdinals::new();
    for &&(_, sym, addend, _) in &binds {
        let key = (sym.unwrap(), table_addend(addend));
        ordinals.entry(key).or_insert_with(|| {
            imports.push(key);
            imports.len() - 1
        });
    }
    (imports, ordinals)
}

/// The pointer format of the chains. A rebase target is a VM address
/// under DYLD_CHAINED_PTR_64 and an offset from the image's load
/// address under DYLD_CHAINED_PTR_64_OFFSET, which dyld reads from
/// macOS 12 on. ld-prime writes the latter for a macOS 12 (or firmware)
/// target on every architecture and output kind, and for a -static
/// image, which no dyld reads, whatever its target; the former only
/// when -fixup_chains forces chains on an older one.
fn pointer_format<E: Target>(ctx: &Context<E>) -> u16 {
    let macos12 = crate::macho::encode_version(12, 0, 0);
    if ctx.args.static_link
        || crate::macho::targets_macos(ctx.args.platform, ctx.args.platform_minos, macos12)
    {
        DYLD_CHAINED_PTR_64_OFFSET
    } else {
        DYLD_CHAINED_PTR_64
    }
}

/// Returns None if the image must have classic dyld info instead (see
/// check_pointer_alignment).
pub fn build_chained_fixups<E: Target>(ctx: &Context<E>) -> Option<ChainedFixups> {
    // An image with nothing to fix up still gets the payload (a
    // header and a starts table with no pages), as ld64 writes it:
    // dyld reads the format from the load command, and its absence
    // would mean classic dyld info.
    let (with_atoms, suspects) = collect_fixups(ctx);
    let unaligned = with_atoms.iter().any(|&(addr, ..)| !addr.is_multiple_of(8));
    if !check_pointer_alignment(ctx, suspects, true, unaligned) {
        return None;
    }
    let (dynsyms, ordinals) = import_table(&with_atoms);
    let fixups: Vec<(u64, Option<SymbolId>, u64)> =
        with_atoms.into_iter().map(|(addr, sym, addend, _)| (addr, sym, addend)).collect();

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
    let pad = |buf: &mut Vec<u8>, align: usize| buf.resize(buf.len().next_multiple_of(align), 0);

    let mut buf = Vec::new();
    // dyld_chained_fixups_header; the offsets are backpatched.
    push32(&mut buf, 0); // fixups_version
    push32(&mut buf, 0); // starts_offset
    push32(&mut buf, 0); // imports_offset
    push32(&mut buf, 0); // symbols_offset
    push32(&mut buf, dynsyms.len() as u32);
    push32(&mut buf, import_format);
    push32(&mut buf, 0); // symbols_format: uncompressed
    pad(&mut buf, 8);

    // dyld_chained_starts_in_image
    let starts_offset = buf.len();
    buf[4..8].copy_from_slice(&(starts_offset as u32).to_le_bytes());
    // One per segment command: a -preload image's __LINKEDIT has none.
    let seg_count = ctx.segments.len() - usize::from(ctx.args.preload);
    push32(&mut buf, seg_count as u32);
    let seg_info_table = buf.len();
    for _ in 0..seg_count {
        push32(&mut buf, 0);
    }

    // Per-segment page tables, each 8-aligned. A segment's offset
    // counts from the image's own address, which -image_base may move.
    let image_base = ctx.mach_header.hdr.addr;
    for (seg_idx, seg) in ctx.segments.iter().enumerate() {
        let lo = fixups.partition_point(|&(a, _, _)| a < seg.cmd.vmaddr);
        let hi = fixups.partition_point(|&(a, _, _)| a < seg.cmd.vmaddr + seg.cmd.vmsize);
        if lo == hi {
            continue;
        }
        let fx = &fixups[lo..hi];

        pad(&mut buf, 8);
        let off = buf.len() - starts_offset;
        let ent = seg_info_table + seg_idx * 4;
        buf[ent..ent + 4].copy_from_slice(&(off as u32).to_le_bytes());

        // A -segalign below 4KB makes arm64 chain pages no dyld reads:
        // ld-prime lays the image out to the end all the same, and
        // reports the first segment, unless an unaligned pointer in a
        // chain fails the link first (see check_pointer_alignment).
        let page_size = chain_page_size(ctx);
        if !matches!(page_size, 0x1000 | 0x4000) {
            if ctx.chained_fixups.unaligned.lock().unwrap().is_empty() {
                crate::layout_error!(
                    "chained fixups, page_size not 4KB or 16KB in segment #{seg_idx}"
                );
            }
            break;
        }
        let npages = ((fx.last().unwrap().0 + 1 - seg.cmd.vmaddr).div_ceil(page_size)) as usize;
        // The record is 22 bytes of fields plus one u16 per page; its
        // size counts just those, without padding.
        push32(&mut buf, 22 + npages as u32 * 2);
        push16(&mut buf, page_size as u16);
        push16(&mut buf, pointer_format(ctx));
        // A layout in error (reported by print_final_layout) may put a
        // segment below the image base; the table is never written then.
        push64(&mut buf, seg.cmd.vmaddr.wrapping_sub(image_base));
        push32(&mut buf, 0); // max_valid_pointer
        push16(&mut buf, npages as u16);
        let mut j = 0;
        for i in 0..npages {
            let page_addr = seg.cmd.vmaddr + i as u64 * page_size;
            while j < fx.len() && fx[j].0 < page_addr {
                j += 1;
            }
            if j < fx.len() && fx[j].0 < page_addr + page_size {
                push16(&mut buf, (fx[j].0 - page_addr) as u16);
            } else {
                push16(&mut buf, DYLD_CHAINED_PTR_START_NONE);
            }
        }
    }

    // Import table, aligned only as its entries need: 4 bytes, or 8
    // for 64-bit addends.
    pad(&mut buf, if import_format == DYLD_CHAINED_IMPORT_ADDEND64 { 8 } else { 4 });
    let imports_offset = buf.len();
    buf[8..12].copy_from_slice(&(imports_offset as u32).to_le_bytes());
    // Each import has a name string of its own after a leading NUL,
    // repeated for a symbol imported with several addends.
    let mut name_offs = Vec::with_capacity(dynsyms.len());
    let mut nameoff: u32 = 1;
    for &(sym, _) in &dynsyms {
        name_offs.push(nameoff);
        nameoff += ctx.symbols[sym].name().len() as u32 + 1;
    }
    for (i, &(sym, addend)) in dynsyms.iter().enumerate() {
        let s = &ctx.symbols[sym];
        // An import names its dylib; one of this image's own weak
        // definitions is bound by weak lookup (ordinal -3), which
        // makes dyld search every loaded image for the coalesced
        // winner, a class bound to the image itself names it (0), and
        // a -flat_namespace export is a flat lookup (-2).
        let ordinal_bits = |bits: u32| -> u64 {
            let special = |ordinal: i32| (ordinal as i64 as u64) & ((1u64 << bits) - 1);
            match s.file() {
                Some(FileId::Dylib(dylib)) if !ctx.binds_weak_lookup(sym) => {
                    ctx.chained_import_ordinal(dylib, bits)
                }
                _ if ctx.binds_to_self(sym) => BIND_SPECIAL_DYLIB_SELF as u64,
                _ if ctx.is_flat_export(sym) && !ctx.binds_weak_lookup(sym) => {
                    special(BIND_SPECIAL_DYLIB_FLAT_LOOKUP)
                }
                _ => special(BIND_SPECIAL_DYLIB_WEAK_LOOKUP),
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
    buf.push(0);
    for &(sym, _) in &dynsyms {
        buf.extend_from_slice(ctx.symbols[sym].name().as_bytes());
        buf.push(0);
    }
    pad(&mut buf, 8);

    Some((buf, fixups, dynsyms, ordinals))
}

/// The pages the fixup chains of a segment are cut into, from its start:
/// a chain stays in one page, which dyld takes to be 4 KiB or 16 KiB
/// long. ld-prime cuts an arm64 image's in its segment alignment (and
/// refuses any other with fixups), an x86-64 image's in 4 KiB pages
/// whatever its segment alignment.
fn chain_page_size<E: Target>(ctx: &Context<E>) -> u64 {
    if E::CPUTYPE == CPU_TYPE_X86_64 { 0x1000 } else { ctx.args.segment_align }
}

/// Writes the fixup chains into the copied output: every fixup word is
/// rewritten to encode its payload plus the 4-byte-stride distance to
/// the next fixup in the same page.
pub fn write_fixup_chains<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let page_shift = chain_page_size(ctx).trailing_zeros();
    // What a rebase target counts from: zero for a VM address, the
    // image's own address for an offset.
    let target_base = match pointer_format(ctx) {
        DYLD_CHAINED_PTR_64_OFFSET => ctx.mach_header.hdr.addr,
        _ => 0,
    };

    for seg in &ctx.segments {
        let lo = ctx.chained_fixups.fixups.partition_point(|&(a, _, _)| a < seg.cmd.vmaddr);
        let hi = ctx
            .chained_fixups
            .fixups
            .partition_point(|&(a, _, _)| a < seg.cmd.vmaddr + seg.cmd.vmsize);
        let fx = &ctx.chained_fixups.fixups[lo..hi];
        // Pages count from the segment's start.
        let page = |addr: u64| (addr - seg.cmd.vmaddr) >> page_shift;

        for (i, &(addr, sym, addend)) in fx.iter().enumerate() {
            let next = match fx.get(i + 1) {
                Some(&(next_addr, _, _)) if page(next_addr) == page(addr) => (next_addr - addr) / 4,
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
                    // dyld_chained_ptr_64_rebase; the word currently
                    // holds the absolute target address, its top byte
                    // (high8) carried separately.
                    let val = u64::from_le_bytes(buf[off..off + 8].try_into().unwrap());
                    let high8 = val >> 56;
                    let target = (val & 0x00ff_ffff_ffff_ffff).wrapping_sub(target_base);
                    if target >> 36 != 0 {
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
                    target | (high8 << 36) | (next << 51)
                }
            };
            buf[off..off + 8].copy_from_slice(&word.to_le_bytes());
        }
    }
}

/// Collects the fixups, with the pointers of subsections aligned less
/// than a pointer and those at an address no multiple of 8, as
/// (subsection, address) pairs, for check_pointer_alignment.
fn collect_fixups<E: Target>(ctx: &Context<E>) -> (Vec<Fixup>, Vec<(u32, u64)>) {
    // Every subsection's fixups are independent; collect them on all
    // cores and sort the union in parallel, as mold does.
    let suspects = std::sync::Mutex::new(Vec::new());
    let mut fixups: Vec<Fixup> = ctx
        .isecs
        .par_iter()
        .enumerate()
        .filter(|(_, isec)| {
            isec.is_alive() && isec.replacement == crate::input_sections::NO_REPLACEMENT
        })
        .flat_map_iter(|(id, isec)| {
            let base = ctx.chunk_header(isec.output_section().unwrap()).addr + isec.offset as u64;
            let suspects = &suspects;
            crate::input_files::isec_relocs_of(&ctx.objs, isec).iter().filter_map(move |rel| {
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
                let fixup = match ctx.reloc_target_sym(isec.file as usize, rel) {
                    Some(id) if ctx.is_swift_force_load_ref(id) => None,
                    Some(id) if ctx.binds_at_runtime(id) || ctx.binds_to_self(id) => {
                        Some((addr, Some(id), rel.addend as u64, base))
                    }
                    _ => {
                        if !ctx.reloc_target_is_tls(isec.file as usize, rel) {
                            Some((addr, None, 0, base))
                        } else {
                            None
                        }
                    }
                };
                if fixup.is_some() && (isec.p2align < 3 || !addr.is_multiple_of(8)) {
                    suspects.lock().unwrap().push((id as u32, addr));
                }
                fixup
            })
        })
        .collect();

    {
        for (i, &id) in ctx.got.got_syms.iter().enumerate() {
            if ctx.is_absolute_symbol(id) && !ctx.binds_at_runtime(id) {
                continue;
            }
            let sym = Some(id).filter(|&id| ctx.binds_at_runtime(id));
            let slot = ctx.got.slot_addr(i);
            fixups.push((slot, sym, 0, slot));
        }
    }
    for i in 0..ctx.objc_stubs.symbols.len() + ctx.objc_stubs.extra_selrefs.len() {
        let slot = ctx.objc_selref_addr(i);
        fixups.push((slot, None, 0, slot));
    }
    for (addr, _) in super::rebase_info::data_blob_pointers(ctx) {
        fixups.push((addr, None, 0, addr));
    }
    // The pointers of an image nothing slides keep their addresses:
    // its chains hold only binds.
    if super::rebase_info::is_never_slid(ctx) {
        fixups.retain(|&(_, sym, _, _)| sym.is_some());
    }

    fixups.par_sort_unstable_by_key(|&(addr, _, _, _)| addr);
    (fixups, suspects.into_inner().unwrap())
}

/// The largest addend a chained bind can carry inline; anything bigger
/// goes into the import table.
const MAX_INLINE_ADDEND: u64 = 255;

/// Checks the pointers of an image with classic dyld info as
/// check_pointer_alignment does.
pub fn check_classic_pointers<E: Target>(ctx: &Context<E>) {
    if checks_pointer_alignment(ctx) {
        check_pointer_alignment(ctx, collect_fixups(ctx).1, false, false);
    }
}

/// Whether ld-prime checks the alignment of the pointers dyld fixes up:
/// in an image dyld loads, with chained fixups, or with classic dyld
/// info for a deployment target that gives chained fixups by default.
fn checks_pointer_alignment<E: Target>(ctx: &Context<E>) -> bool {
    !ctx.args.without_dyld()
        && (ctx.args.fixup_chains
            || crate::macho::is_new_os(
                E::NAME,
                ctx.args.output_type,
                ctx.args.platform,
                ctx.args.platform_minos,
            ))
}

/// ld-prime wants each pointer dyld fixes up 8-aligned, as a fixup
/// chain's links are words: it warns of every atom aligned less than a
/// pointer that holds one, then, once relocations are applied, of every
/// unaligned pointer where the image has classic dyld info. With chained
/// fixups, arm64 fails the link at the first unaligned pointer of a
/// chain (of the atoms in address order, each one's from the last, the
/// order of an assembler's relocations), and x86-64 gives chains up for
/// classic dyld info instead, whose header the load commands fit in as
/// laid out (see header_pad), on an unaligned pointer of its own too
/// (`unaligned`: a GOT slot a -segalign below 8 moved off 8 bytes).
/// `suspects` are the pointers collect_fixups gives; the unaligned ones
/// are left for report_unaligned_chain_pointer and
/// warn_unaligned_pointers. Returns false for that fallback.
fn check_pointer_alignment<E: Target>(
    ctx: &Context<E>,
    mut suspects: Vec<(u32, u64)>,
    chained: bool,
    unaligned: bool,
) -> bool {
    if !checks_pointer_alignment(ctx) || suspects.is_empty() && !unaligned {
        return true;
    }
    let atom_addr = |id: u32| ctx.isec_addr(id as usize);
    suspects.sort_unstable_by_key(|&(id, addr)| (atom_addr(id), id, addr));

    let mut atoms: Vec<u32> = suspects.iter().map(|&(id, _)| id).collect();
    atoms.dedup();
    for id in atoms {
        let p2align = ctx.isecs[id as usize].p2align;
        if p2align < 3 {
            crate::warn!(
                "alignment ({}) of atom {} is too small and may result in unaligned pointers ",
                1 << p2align,
                atom_location(ctx, id, None)
            );
        }
    }

    suspects.retain(|&(_, addr)| !addr.is_multiple_of(8));
    if chained && E::CPUTYPE == CPU_TYPE_ARM64 {
        // A pointer in a read-only segment is a text relocation, which
        // no chain holds. One 8-aligned in its segment is fine, though a
        // -segalign below 8 moved the segment off 8 bytes: the chain
        // pages fail the link then (see build_chained_fixups).
        suspects.retain(|&(_, addr)| !ctx.text_reloc_ranges.iter().any(|r| r.contains(&addr)));
        suspects.retain(|&(_, addr)| !offset_in_segment(ctx, addr).is_multiple_of(8));
        if let Some(&(first, _)) = suspects.first() {
            let &last = suspects.iter().rfind(|&&(id, _)| id == first).unwrap();
            ctx.chained_fixups.unaligned.lock().unwrap().push(last);
        }
        return true;
    }
    if suspects.is_empty() && !unaligned {
        return true;
    }
    if chained {
        crate::warn!("disabling chained fixups because of unaligned pointers");
    }
    *ctx.chained_fixups.unaligned.lock().unwrap() = suspects;
    !chained
}

/// Fails the link on the unaligned pointer check_pointer_alignment found
/// in an arm64 chain, as ld-prime does after applying relocations: once
/// the text relocations are listed, before they would fail it. Returns
/// whether there was one.
pub fn report_unaligned_chain_pointer<E: Target>(ctx: &Context<E>) -> bool {
    if !ctx.use_chained_fixups() {
        return false;
    }
    let Some(&(id, addr)) = ctx.chained_fixups.unaligned.lock().unwrap().first() else {
        return false;
    };
    crate::error!("pointer not aligned in {}", atom_location(ctx, id, Some(addr)));
    true
}

/// Warns of each unaligned pointer check_pointer_alignment found in an
/// image with classic dyld info, as ld-prime does when it encodes them:
/// only once nothing has failed the link, and only of one off 8 bytes
/// in its segment - not of one a -segalign below 8 moved off with the
/// segment, which gives chained fixups up all the same.
pub fn warn_unaligned_pointers<E: Target>(ctx: &Context<E>) {
    if !ctx.use_chained_fixups() {
        for &(id, addr) in ctx.chained_fixups.unaligned.lock().unwrap().iter() {
            if !offset_in_segment(ctx, addr).is_multiple_of(8) {
                crate::warn!("pointer not aligned in {}", atom_location(ctx, id, Some(addr)));
            }
        }
    }
}

/// An address's offset from the start of the segment that holds it.
fn offset_in_segment<E: Target>(ctx: &Context<E>, addr: u64) -> u64 {
    let seg = ctx
        .segments
        .iter()
        .find(|seg| (seg.cmd.vmaddr..seg.cmd.vmaddr + seg.cmd.vmsize).contains(&addr));
    addr - seg.map_or(0, |seg| seg.cmd.vmaddr)
}

/// A place in an atom as ld-prime names it in a diagnostic: 'name' of
/// the atom, +0xoffset of `addr` in it (if not its start), and its
/// file's real path in parentheses. An atom is named by a symbol at its
/// start, an exported one first.
fn atom_location<E: Target>(ctx: &Context<E>, isec: u32, addr: Option<u64>) -> String {
    let off = addr.map_or(0, |addr| addr - ctx.isec_addr(isec as usize));
    ctx.atom_ref(isec as usize, off as u32)
}
