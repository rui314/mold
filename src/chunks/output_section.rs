//! An output section of the image: the concatenation of the input
//! subsections assigned to it, the range-extension thunks placed among
//! them, and the linker-synthesized tail after them.

use rayon::prelude::*;

use crate::chunks::{ChunkHeader, ChunkId, OutputSectionId};
use crate::context::Context;
use crate::input_sections::InputSectionId;
use crate::objc::{DataField, objc_ref_addr};
use crate::symbol::SymbolId;
use crate::symbol_moves::MoveOption;
use crate::target::Target;

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
    /// The name the section ranks by among those of its segment (see
    /// output_sections::output_section_rank), which its first member
    /// took along through moves and renames; None for one that ranks
    /// by its type alone.
    pub rank_name: Option<(&'static str, &'static str)>,
    /// A section the linker synthesizes under this one's name, which
    /// joins it (see output_sections::merge_synthetic_sections) at
    /// `synthetic_off`, after the members and the tail.
    pub synthetic: Option<ChunkId>,
    pub synthetic_off: u64,
}

impl OutputSection {
    pub fn new(segname: &'static str, sectname: &str) -> Self {
        Self {
            hdr: ChunkHeader::new(segname, sectname),
            members: Vec::new(),
            thunks: Vec::new(),
            tail: Tail::None,
            tail_off: 0,
            has_blobs: false,
            has_tlv_data: false,
            moved: None,
            rank_name: None,
            synthetic: None,
            synthetic_off: 0,
        }
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, id: OutputSectionId, buf: &mut [u8]) {
    let osec = ctx.output_section(id);
    for thunk in &osec.thunks {
        let off = thunk.offset as usize;
        let end = off + thunk.syms.len() * E::THUNK_SIZE as usize;
        E::write_thunk(ctx, osec.hdr.addr + thunk.offset, &thunk.syms, &mut buf[off..end]);
    }
    // Subsections copy and relocate in parallel, as in mold: each
    // occupies a disjoint slice of the output section (relocations only
    // ever write within their own subsection), so the work distributes
    // freely. A pointer wrapper stands in for the aliasing split rayon
    // can't express directly.
    struct BufPtr(*mut u8);
    unsafe impl Sync for BufPtr {}
    let bufp = BufPtr(buf.as_mut_ptr());
    let bufp = &bufp;
    osec.members.par_iter().for_each(|&id| {
        let isec = &ctx.isecs[id];
        if isec.data().is_empty() {
            return;
        }
        let off = isec.offset as usize;
        // SAFETY: subsections' [offset, +size) ranges are disjoint by
        // layout, so each iteration touches its own slice.
        let slice = unsafe { std::slice::from_raw_parts_mut(bufp.0.add(off), isec.data().len()) };
        slice.copy_from_slice(isec.data());
        let base = osec.hdr.addr + isec.offset as u64;
        E::apply_relocs(ctx, ctx.isec_relocs(id as usize), id as usize, base, slice);
    });

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
                let val = ctx.objc_methname_addr(i);
                tail[i * 8..i * 8 + 8].copy_from_slice(&val.to_le_bytes());
            }
            let n = stubs.symbols.len();
            for (j, &name) in stubs.extra_selrefs.iter().enumerate() {
                let val = ctx.isec_addr(name as usize);
                tail[(n + j) * 8..(n + j) * 8 + 8].copy_from_slice(&val.to_le_bytes());
            }
        }
    }

    // A synthesized section of the same name, last.
    if let Some(chunk) = osec.synthetic {
        let off = osec.synthetic_off as usize;
        let size = ctx.chunk_header(chunk).size as usize;
        crate::chunks::copy_buf(ctx, chunk, &mut buf[off..off + size]);
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
                    let addr = if r.import(ctx).is_some() { 0 } else { objc_ref_addr(ctx, *r) };
                    buf[at..at + 8].copy_from_slice(&addr.to_le_bytes());
                    at += 8;
                }
            }
        }
    }
}
