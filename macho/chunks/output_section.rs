//! An output section of the image: the concatenation of the input
//! subsections assigned to it, the range-extension thunks placed among
//! them, and the linker-synthesized tail after them.

use rayon::prelude::*;

use crate::arch::Target;
use crate::chunks::split_info::{Entry, Places, push};
use crate::chunks::symtab::{NamedEntry, local_msym};
use crate::chunks::{ChunkHeader, ChunkId, OutputSectionId};
use crate::context::Context;
use crate::input_files::{DataField, data_blob_pointers, is_listed_out};
use crate::input_sections::{InputSection, InputSectionId, Reloc};
use crate::macho::DYLD_CACHE_ADJ_V2_POINTER_64;
use crate::symbol_moves::MoveOption;
use crate::thunks::Thunk;
use crate::util::align_to;

/// Linker-synthesized data appended to an output section after its
/// input subsections. The Objective-C runtime reads exactly one
/// __objc_selrefs section per image (the selectors it uniques at load
/// time) and one __objc_methname, so the selector references and name
/// strings the _objc_msgSend$<selector> stubs need cannot form
/// sections of their own next to the compilers'; they are laid out as
/// the tail of the section of that name, which is created empty when
/// no input provides one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tail {
    None,
    /// Selector name strings for the synthesized objc stubs.
    ObjcMethname,
    /// Selector references (pointers into __objc_methname) loaded by the
    /// synthesized objc stubs.
    ObjcSelrefs,
    /// Objective-C data the linker synthesized (merged class data:
    /// class_ro_t records, protocol and property lists, classic method
    /// lists) in the section of that name.
    DataBlobs,
    /// Relative method lists -move_to_ro_segment took out of
    /// __TEXT,__objc_methlist (see chunks::objc_methlist).
    ObjcMethlists,
}

#[derive(Debug)]
pub struct OutputSection {
    pub hdr: ChunkHeader,
    /// The input subsections, in output order.
    pub members: Vec<InputSectionId>,
    pub thunks: Vec<Thunk>,
    pub tail: Tail,
    /// Offset of the tail within the section, set at layout.
    pub tail_off: u64,
    /// Whether synthesized records (data blobs) stand among the members
    /// in place of the input records they replace.
    pub has_blobs: bool,
    /// Whether thread-local data (input sections so typed) went here,
    /// which a rename may have put in a section of another type (see
    /// passes::set_osec_offsets).
    pub has_tlv_data: bool,
    /// For a section a symbol move made (see symbol_moves), the option
    /// that moved its first member.
    pub moved: Option<MoveOption>,
}

impl OutputSection {
    pub fn new(segname: &'static [u8], sectname: &'static [u8]) -> Self {
        Self {
            hdr: ChunkHeader::new(segname, sectname),
            members: Vec::new(),
            thunks: Vec::new(),
            tail: Tail::None,
            tail_off: 0,
            has_blobs: false,
            has_tlv_data: false,
            moved: None,
        }
    }
}

/// Appends a synthesized `tail` of `tail_size` bytes aligned to
/// 2^`p2align` to an output section, after its input subsections.
pub(crate) fn append_tail(osec: &mut OutputSection, p2align: u32, tail: Tail, tail_size: u64) {
    osec.hdr.p2align = osec.hdr.p2align.max(p2align);
    osec.tail = tail;
    osec.tail_off = align_to(osec.hdr.size, 1 << p2align);
    osec.hdr.size = osec.tail_off + tail_size;
}

/// Lays out an output section's members, each at its alignment after
/// the one before, and returns their offsets, in member order, and the
/// section's size, for compute_section_sizes to record. mold's layout,
/// which records the offsets itself.
pub fn layout<E: Target>(ctx: &Context<E>, osec: &OutputSection) -> (Vec<u64>, u64) {
    let mut offs = Vec::with_capacity(osec.members.len());
    let mut off = 0;
    for &id in &osec.members {
        let isec = &ctx.isecs[id];
        off = isec.align_offset(off);
        offs.push(off);
        off += isec.size as u64;
    }
    (offs, off)
}

/// Runs `f` in parallel on each member's bytes in `buf`, the output
/// section's contents, along with the bytes up to the next member, as
/// mold's for_each_member does: splitting `buf` at member offsets,
/// rather than indexing it by them, gives each member its own exclusive
/// slice.
fn for_each_member<E: Target>(
    ctx: &Context<E>,
    osec: &OutputSection,
    buf: &mut [u8],
    f: impl Fn(InputSectionId, &mut [u8]) + Sync,
) {
    let members = &osec.members;
    let offset = |i: usize| match members.get(i) {
        Some(&m) => ctx.isecs[m].offset as usize,
        None => osec.hdr.size as usize,
    };

    // The first member starts at its offset modulo its alignment (see
    // InputSection::align_offset), not necessarily at 0.
    rayon::iter::split((0..members.len(), &mut buf[offset(0)..]), |(range, buf)| {
        if range.len() <= 1 {
            return ((range, buf), None);
        }
        let mid = range.start + range.len() / 2;
        let (left, right) = buf.split_at_mut(offset(mid) - offset(range.start));
        ((range.start..mid, left), Some((mid..range.end, right)))
    })
    .for_each(|(range, mut buf)| {
        let mut pos = offset(range.start);
        for i in range {
            let end = offset(i + 1);
            let slice = buf.split_off_mut(..end - pos).unwrap();
            pos = end;
            f(members[i], slice);
        }
    });
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, id: OutputSectionId, buf: &mut [u8]) {
    let osec = ctx.output_section(id);
    // Subsections copy and relocate in parallel, as in mold: relocations
    // only ever write within their own subsection.
    for_each_member(ctx, osec, buf, |m, slice| {
        let isec = &ctx.isecs[m];
        let data = isec.contents();
        if data.is_empty() {
            return;
        }
        let own = &mut slice[..data.len()];
        own.copy_from_slice(data);
        let base = osec.hdr.addr + isec.offset as u64;
        let rels = isec.rels(&ctx.objs[isec.file as usize]);
        E::apply_reloc_alloc(ctx, rels, m as usize, base, own);
    });

    // The range-extension thunks, between the members.
    crate::thunks::copy_buf(ctx, osec, buf);

    // Synthesized Objective-C records, in the tail or among the members.
    if osec.tail == Tail::DataBlobs || osec.has_blobs {
        write_data_blobs(ctx, id, buf);
    }
    if osec.tail == Tail::ObjcMethlists {
        let chunk = ChunkId::Output(id);
        crate::chunks::objc_methlist::write_lists(ctx, chunk, osec.hdr.addr, buf);
    }

    // The linker-synthesized tail after the inputs.
    let tail = &mut buf[osec.tail_off as usize..];
    match osec.tail {
        Tail::None => {}
        Tail::ObjcMethname => crate::chunks::objc_stubs::write_methnames(ctx, tail),
        // Written above, each record at its offset in the section.
        Tail::DataBlobs | Tail::ObjcMethlists => {}
        Tail::ObjcSelrefs => crate::chunks::objc_stubs::write_selrefs(ctx, tail),
    }
}

/// Writes the synthesized records (data blobs) placed in an output
/// section, each at its offset in it.
fn write_data_blobs<E: Target>(ctx: &Context<E>, id: OutputSectionId, buf: &mut [u8]) {
    for b in ctx
        .data_blobs
        .iter()
        .filter(|b| ctx.isecs[b.isec as usize].output_section() == Some(ChunkId::Output(id)))
    {
        let mut at = ctx.isecs[b.isec as usize].offset as usize;
        for f in &b.fields {
            match f {
                DataField::Bytes(bytes) => {
                    buf[at..at + bytes.len()].copy_from_slice(bytes);
                    at += bytes.len();
                }
                DataField::Ptr(r) => {
                    // dyld fills in a pointer to an import.
                    let addr = if r.import(ctx).is_some() { 0 } else { r.addr(ctx) };
                    buf[at..at + 8].copy_from_slice(&addr.to_le_bytes());
                    at += 8;
                }
            }
        }
    }
}

/// The pointers (see Target::is_absrel) the relocations of a live
/// subsection write, each with its address.
pub(crate) fn pointer_relocs<'a, E: Target>(
    ctx: &'a Context<E>,
    isec: &'a InputSection,
) -> impl Iterator<Item = (u64, &'a Reloc)> + 'a {
    let base = ctx.chunk_header(isec.output_section().unwrap()).addr + isec.offset as u64;
    let rels = isec.rels(&ctx.objs[isec.file as usize]).iter();
    rels.filter(|rel| E::is_absrel(rel)).map(move |rel| (base + rel.offset as u64, rel))
}

/// The pointers of the output sections a loader must slide (see
/// rebase_info::rebase_locations): those written for absolute
/// relocations to local targets, and the synthesized records' pointer
/// fields into the image.
pub fn rebase_locations<E: Target>(ctx: &Context<E>, locs: &mut Vec<u64>) {
    // Pointers written for UNSIGNED relocations to local targets.
    for isec in ctx.isecs.iter() {
        if !isec.is_emitted() {
            continue;
        }
        let file = &ctx.objs[isec.file as usize];
        for (addr, rel) in pointer_relocs(ctx, isec) {
            // Pointers to thread-local data are thread-pointer-relative
            // offsets, not addresses, so they are not rebased.
            let target = rel.sym(file);
            let imported = target.is_some_and(|id| {
                ctx.symbols[id].binds_pointer(ctx) || ctx.symbols[id].is_dtrace_pointer_target()
            });
            let absolute = target.is_some_and(|id| ctx.symbols[id].is_absolute(ctx));
            if !imported && !absolute && !rel.refers_to_tls(ctx, file) {
                locs.push(addr);
            }
        }
    }

    // Pointer fields of the synthesized Objective-C records.
    for (addr, _) in data_blob_pointers(ctx) {
        locs.push(addr);
    }
}

/// The output sections' references for LC_SEGMENT_SPLIT_INFO but their
/// inputs' (see split_info::construct): the range-extension thunks' to
/// their targets, and the synthesized records' pointer fields.
pub(crate) fn split_info_entries<E: Target>(p: &Places<'_, E>, out: &mut Vec<Entry>) {
    let ctx = p.ctx;
    for osec in &ctx.output_sections {
        for thunk in &osec.thunks {
            for (i, &id) in thunk.syms.iter().enumerate() {
                let from = (osec.hdr.sect_idx, thunk.offset + i as u64 * E::THUNK_SIZE);
                p.pcrel(out, from, p.sym(id));
            }
        }
    }
    for blob in &ctx.data_blobs {
        let Some((n, mut at)) = p.isec(blob.isec as usize) else {
            continue;
        };
        for field in &blob.fields {
            match field {
                DataField::Bytes(bytes) => at += bytes.len() as u64,
                DataField::Ptr(r) => {
                    push(out, (n, at), DYLD_CACHE_ADJ_V2_POINTER_64, p.objc_ref(*r));
                    at += 8;
                }
            }
        }
    }
}

/// The local symbols of an output section's range-extension thunks'
/// entries, named as ld-prime names its branch islands (see
/// island_symbols).
pub fn populate_symtab<E: Target>(
    ctx: &Context<E>,
    id: OutputSectionId,
    out: &mut Vec<NamedEntry>,
) {
    let osec = ctx.output_section(id);
    for (addr, name) in island_symbols(ctx, osec) {
        if !is_listed_out(ctx, name) {
            out.push((name, local_msym(osec.hdr.sect_idx, addr), None));
        }
    }
}

/// The local symbols naming an output section's thunk entries, as
/// (address, name). ld-prime lists each branch island among the locals,
/// named after its target: "<target>.island" for the target's first
/// and "<target>.island<n>" for its n-th, in address order, which is
/// the order gather_thunk_addresses recorded them in.
pub fn island_symbols<E: Target>(
    ctx: &Context<E>,
    osec: &OutputSection,
) -> Vec<(u64, &'static [u8])> {
    use std::io::Write;

    let hdr = &osec.hdr;
    osec.thunks
        .par_iter()
        .flat_map_iter(|thunk| {
            let addr = |i: usize| hdr.addr + thunk.offset + i as u64 * E::THUNK_SIZE;
            // A thunk can have many thousands of entries; their names
            // share one allocation.
            let mut buf = Vec::new();
            let mut ends = Vec::with_capacity(thunk.syms.len());
            for (i, &sym) in thunk.syms.iter().enumerate() {
                let addrs = &ctx.symbols[sym].aux(&ctx.symbols).unwrap().thunk_addrs;
                let n = addrs.iter().position(|&a| a == addr(i)).unwrap() + 1;
                buf.extend_from_slice(ctx.symbols[sym].name());
                buf.extend_from_slice(b".island");
                if n > 1 {
                    write!(buf, "{n}").unwrap();
                }
                ends.push(buf.len());
            }
            let buf = crate::util::leak_bytes(buf);
            let mut start = 0;
            ends.into_iter().enumerate().map(move |(i, end)| {
                let name = &buf[start..end];
                start = end;
                (addr(i), name)
            })
        })
        .collect()
}
