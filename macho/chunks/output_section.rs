//! An output section of the image: the concatenation of the input
//! subsections assigned to it, the range-extension thunks placed among
//! them, and the linker-synthesized tail after them.

use rayon::prelude::*;

use crate::arch::Target;
use crate::chunks::{ChunkHeader, ChunkId, OutputSectionId};
use crate::context::Context;
use crate::input_sections::InputSectionId;
use crate::objc::DataField;
use crate::symbol::SymbolId;
use crate::symbol_moves::MoveOption;

/// A range-extension thunk: a block of jump entries placed inside an
/// output section so that branches whose targets are further than the
/// instruction's reach can hop through it.
#[derive(Debug)]
pub struct Thunk {
    /// Offset of the thunk within the output section.
    pub offset: u64,
    pub syms: Vec<SymbolId>,
}

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
    /// passes::check_tlv_template).
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
        let data = isec.data();
        if data.is_empty() {
            return;
        }
        let own = &mut slice[..data.len()];
        own.copy_from_slice(data);
        let base = osec.hdr.addr + isec.offset as u64;
        E::apply_relocs(ctx, ctx.isec_relocs(m as usize), m as usize, base, own);
    });

    // The range-extension thunks, between the members.
    for thunk in &osec.thunks {
        let off = thunk.offset as usize;
        let end = off + thunk.syms.len() * E::THUNK_SIZE as usize;
        E::write_thunk(ctx, osec.hdr.addr + thunk.offset, &thunk.syms, &mut buf[off..end]);
    }

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
        Tail::ObjcMethname => {
            let data = &ctx.objc_stubs.methname_data;
            tail[..data.len()].copy_from_slice(data);
        }
        // Written above, each record at its offset in the section.
        Tail::DataBlobs | Tail::ObjcMethlists => {}
        Tail::ObjcSelrefs => {
            let stubs = &ctx.objc_stubs;
            for i in 0..stubs.symbols.len() {
                // Its selector's name, in the tail of __objc_methname.
                let methname = ctx.output_section(stubs.methname.unwrap());
                let val = methname.hdr.addr + methname.tail_off + stubs.methname_offs[i];
                tail[i * 8..i * 8 + 8].copy_from_slice(&val.to_le_bytes());
            }
            let n = stubs.symbols.len();
            for (j, &name) in stubs.extra_selrefs.iter().enumerate() {
                let val = ctx.isec_addr(name as usize);
                tail[(n + j) * 8..(n + j) * 8 + 8].copy_from_slice(&val.to_le_bytes());
            }
        }
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
