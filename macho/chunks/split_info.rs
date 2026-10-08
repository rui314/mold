//! LC_SEGMENT_SPLIT_INFO: every reference the image makes from one
//! section into another, or into the mach header, so that the dyld
//! shared cache builder, or kmutil building a kernel collection, can
//! slide the segments apart and fix the references up. ld-prime 27037
//! writes it in the V2 format on both targets.
//!
//! The entries are ld-prime's. An absolute pointer or a distance (a
//! SUBTRACTOR pair) in data counts wherever it points, its own section
//! included; arm64's adrp, the ldr or add under it and its branches
//! count when they reach another section, and so does an x86-64 32-bit
//! displacement, which outside code counts anywhere. A reference dyld
//! binds (into another image) or to an absolute symbol needs no entry.
//! Besides the inputs' relocations, the linker's own references count:
//! __stubs and __stub_helper to their pointers, the lazy pointers to
//! __stub_helper, a GOT slot to a symbol of this image, __objc_stubs,
//! the synthesized selector references and Objective-C records, method
//! lists, __init_offsets and __unwind_info (image offsets), and
//! __eh_frame's records.

use rayon::prelude::*;

use crate::arch::{SplitRef, Target};
use crate::chunks::init_offsets::InitFunc;
use crate::chunks::{
    ChunkHeader, ChunkId, delay_init, got, lazy_helpers, lazy_ptrs, objc_methlist, objc_stubs,
    output_section, stub_helper, stubs,
};
use crate::context::Context;
use crate::input_files::FileId;
use crate::input_sections::{InputSection, Reloc, RelocTarget};
use crate::macho_consts::*;
use crate::objc::ObjcRef;
use crate::symbol::SymbolId;
use crate::util::encode_uleb;

#[derive(Debug)]
pub struct SplitInfoSection {
    pub hdr: ChunkHeader,
    pub contents: Vec<u8>,
}

impl SplitInfoSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::linkedit();
        hdr.p2align = 3;
        Self { hdr, contents: Vec::new() }
    }
}

impl Default for SplitInfoSection {
    fn default() -> Self {
        Self::new()
    }
}

/// One reference, in the V2 format's terms. The fields are in the
/// order the format groups references by: (from, to) section, then
/// target offset, then kind.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Entry {
    from_sect: u8,
    to_sect: u8,
    to_off: u64,
    kind: u8,
    from_off: u64,
}

/// A place in the image: a section's ordinal (0 for the mach header)
/// and an offset in it.
pub(crate) type Place = (u8, u64);

pub(crate) fn push(out: &mut Vec<Entry>, from: Place, kind: u8, to: Option<Place>) {
    if let Some((to_sect, to_off)) = to {
        out.push(Entry { from_sect: from.0, to_sect, to_off, kind, from_off: from.1 });
    }
}

pub fn construct<E: Target>(ctx: &Context<E>) -> Vec<u8> {
    if !ctx.args.shared_region {
        return Vec::new();
    }
    let places = Places::new(ctx);
    let mut entries: Vec<Entry> = ctx
        .output_sections
        .par_iter()
        .flat_map(|osec| {
            osec.members.par_iter().flat_map_iter(|&id| {
                let mut v = Vec::new();
                places.isec_entries(id as usize, &mut v);
                v
            })
        })
        .collect();
    // The references of the linker's own chunks, each of which lists
    // its own.
    stubs::split_info_entries(&places, &mut entries);
    lazy_ptrs::split_info_entries(&places, &mut entries);
    stub_helper::split_info_entries(&places, &mut entries);
    got::split_info_entries(&places, &mut entries);
    output_section::split_info_entries(&places, &mut entries);
    lazy_helpers::split_info_entries(&places, &mut entries);
    delay_init::split_info_entries(&places, &mut entries);
    objc_stubs::split_info_entries(&places, &mut entries);
    objc_methlist::split_info_entries(&places, &mut entries);
    places.table_entries(&mut entries);
    places.unwind_entries(&mut entries);
    places.eh_frame_entries(&mut entries);
    entries.par_sort_unstable();
    encode(&entries)
}

/// Encodes the V2 format:
///   0x7f <count> FromToSection+ 0, padded to 8 bytes
///   FromToSection :== <from-sect> <to-sect> <count> ToOffset+
///   ToOffset      :== <to-offset-delta> <count> FromOffset+
///   FromOffset    :== <kind> <count> <from-offset-delta>+
fn encode(entries: &[Entry]) -> Vec<u8> {
    let mut buf = vec![DYLD_CACHE_ADJ_V2_FORMAT];
    let sects: Vec<&[Entry]> =
        entries.chunk_by(|a, b| (a.from_sect, a.to_sect) == (b.from_sect, b.to_sect)).collect();
    encode_uleb(&mut buf, sects.len() as u64);
    for sect in sects {
        encode_uleb(&mut buf, sect[0].from_sect as u64);
        encode_uleb(&mut buf, sect[0].to_sect as u64);
        let targets: Vec<&[Entry]> = sect.chunk_by(|a, b| a.to_off == b.to_off).collect();
        encode_uleb(&mut buf, targets.len() as u64);
        let mut last_to = 0u64;
        for target in targets {
            encode_uleb(&mut buf, target[0].to_off.wrapping_sub(last_to));
            last_to = target[0].to_off;
            let kinds: Vec<&[Entry]> = target.chunk_by(|a, b| a.kind == b.kind).collect();
            encode_uleb(&mut buf, kinds.len() as u64);
            for kind in kinds {
                encode_uleb(&mut buf, kind[0].kind as u64);
                encode_uleb(&mut buf, kind.len() as u64);
                let mut last_from = 0u64;
                for e in kind {
                    encode_uleb(&mut buf, e.from_off - last_from);
                    last_from = e.from_off;
                }
            }
        }
    }
    buf.push(0);
    buf.resize(buf.len().next_multiple_of(8), 0);
    buf
}

/// Resolves what the image refers to into places, as the output's
/// writers resolve them into addresses.
pub(crate) struct Places<'a, E: Target> {
    pub(crate) ctx: &'a Context<E>,
    header_addr: u64,
    /// The sections with contents, by address: (start, end, ordinal).
    sects: Vec<(u64, u64, u8)>,
    /// Each section's address, by ordinal.
    starts: Vec<u64>,
    /// The layout-boundary symbols (section$start$..., segment$end$...),
    /// each at the section it bounds.
    boundaries: hashbrown::HashMap<SymbolId, Place>,
}

impl<'a, E: Target> Places<'a, E> {
    fn new(ctx: &'a Context<E>) -> Self {
        let mut sects: Vec<(u64, u64, u8)> = ctx
            .chunks
            .iter()
            .map(|&id| ctx.chunk_header(id))
            .filter(|h| h.is_sect && h.size > 0)
            .map(|h| (h.addr, h.addr + h.size, h.sect_idx))
            .collect();
        sects.sort_unstable();
        let mut starts = vec![0; 256];
        for hdr in ctx.chunks.iter().map(|&id| ctx.chunk_header(id)).filter(|h| h.is_sect) {
            starts[hdr.sect_idx as usize] = hdr.addr;
        }
        let mut places = Self {
            ctx,
            header_addr: ctx.mach_header.hdr.addr,
            sects,
            starts,
            boundaries: Default::default(),
        };
        places.boundaries = ctx
            .boundary_syms
            .iter()
            .filter_map(|(id, is_start, seg, sect)| {
                Some((*id, places.boundary(*is_start, seg, *sect)?))
            })
            .collect();
        places
    }

    /// A section$ symbol's section, at its start or end; a segment$
    /// symbol's first section at its start, or last at its end.
    fn boundary(&self, is_start: bool, seg: &[u8], sect: Option<&[u8]>) -> Option<Place> {
        let ctx = self.ctx;
        let mut hdrs = ctx.chunks.iter().map(|&id| ctx.chunk_header(id)).filter(|h| {
            h.is_sect && h.segname == seg && sect.is_none_or(|sect| h.sectname == sect)
        });
        let hdr = if is_start { hdrs.next()? } else { hdrs.last()? };
        let end = match sect {
            Some(_) => hdr.addr + hdr.size,
            None => {
                let seg = ctx.segments.iter().find(|s| s.name == seg)?;
                seg.cmd.vmaddr + seg.cmd.vmsize
            }
        };
        Some((hdr.sect_idx, if is_start { 0 } else { end - hdr.addr }))
    }

    /// Where offset `off` of chunk `id` lies.
    pub(crate) fn chunk(&self, id: ChunkId, off: u64) -> Place {
        let hdr = self.ctx.chunk_header(id);
        (hdr.sect_idx, hdr.addr - self.starts[hdr.sect_idx as usize] + off)
    }

    /// Where address `addr`, in chunk `id`, lies.
    pub(crate) fn chunk_addr(&self, id: ChunkId, addr: u64) -> Place {
        let hdr = self.ctx.chunk_header(id);
        (hdr.sect_idx, addr - self.starts[hdr.sect_idx as usize])
    }

    /// Where a subsection lies, if it is laid out.
    pub(crate) fn isec(&self, id: usize) -> Option<Place> {
        let isec = &self.ctx.isecs[self.ctx.isecs.resolve(id)];
        let chunk = isec.output_section()?;
        (isec.offset != u32::MAX).then(|| self.chunk(chunk, isec.offset as u64))
    }

    /// Where a symbol's address lies, as Symbol::addr resolves it
    /// (a dylib symbol at its stub); None for an absolute symbol. The
    /// linker's own sectionless symbols are the layout boundaries and
    /// the mach header's names (___dso_handle, __mh_*_header).
    pub(crate) fn sym(&self, id: SymbolId) -> Option<Place> {
        let ctx = self.ctx;
        let symbols = &ctx.symbols;
        let sym = &symbols[id];
        match sym.file()? {
            FileId::Dylib(_) => {
                if let Some(idx) = sym.lazy_stub_idx(symbols) {
                    Some(self.lazy_helper(idx))
                } else if let Some(idx) = sym.delay_stub_idx(symbols) {
                    Some(self.chunk(ChunkId::DelayStubs, delay_init::stub_offset::<E>(idx)))
                } else {
                    let idx = sym.stub_idx(symbols)?;
                    Some(self.chunk(ChunkId::Stubs, stubs::entry_offset::<E>(idx)))
                }
            }
            FileId::Obj(obj) => {
                if let Some(isec) = sym.input_section() {
                    let (n, off) = self.isec(isec as usize)?;
                    Some((n, off + sym.value))
                } else if let Some(idx) = sym.objc_stub_idx(symbols) {
                    let off = objc_stubs::entry_offset(ctx, idx);
                    Some(self.chunk(ChunkId::ObjcStubs, off))
                } else if ctx.is_internal(obj as usize) {
                    let header = (0, sym.value.wrapping_sub(self.header_addr));
                    Some(self.boundaries.get(&id).copied().unwrap_or(header))
                } else {
                    None
                }
            }
        }
    }

    /// Where a symbol dyld doesn't bind lies; None for an import.
    pub(crate) fn own_sym(&self, id: SymbolId) -> Option<Place> {
        if self.ctx.symbols[id].is_imported() { None } else { self.sym(id) }
    }

    /// A symbol's GOT slot, or a lazy dylib's symbol's __lazy_load_got
    /// slot.
    pub(crate) fn got_slot(&self, id: SymbolId) -> Place {
        let symbols = &self.ctx.symbols;
        let sym = &symbols[id];
        match sym.got_idx(symbols) {
            Some(idx) => self.got_index(idx as usize),
            None => {
                let addr = self.ctx.lazy_load_got.slot_addr(sym.lazy_got_idx(symbols).unwrap());
                self.chunk_addr(ChunkId::LazyLoadGot, addr)
            }
        }
    }

    /// Where __lazy_helpers entry `i` lies.
    pub(crate) fn lazy_helper(&self, i: u32) -> Place {
        self.chunk_addr(ChunkId::LazyHelpers, self.ctx.lazy_helpers.helper_addr(i as usize))
    }

    pub(crate) fn got_index(&self, i: usize) -> Place {
        self.chunk_addr(ChunkId::Got, self.ctx.got.slot_addr(i))
    }

    /// The place of an image offset (in __unwind_info): the section it
    /// lies in, or for the index's sentinel, one past the end of the
    /// last function, the section that function ends.
    fn image_offset(&self, off: u32, is_sentinel: bool) -> Option<Place> {
        let addr = self.header_addr + off as u64;
        let key = addr - if is_sentinel { 2 } else { 0 };
        let i = self.sects.partition_point(|&(start, _, _)| start <= key).checked_sub(1)?;
        let (start, end, n) = self.sects[i];
        (key < end).then_some((n, addr - start))
    }

    /// Where relocation `r` of a subsection points, as apply_reloc_alloc
    /// resolves it: at a stub or GOT slot, or at the target plus the
    /// addend; for a distance's positive term, at the target itself,
    /// as ld64 takes it. None for a reference dyld binds.
    fn reloc_target(&self, isec: &InputSection, r: &Reloc, with_addend: bool) -> Option<Place> {
        let ctx = self.ctx;
        let addend = if with_addend { r.addend } else { 0 };
        let id = match r.target() {
            RelocTarget::Section(idx) => {
                let (n, off) = self.isec(idx as usize)?;
                return Some((n, off.wrapping_add_signed(addend)));
            }
            RelocTarget::Sym(idx) => ctx.objs[isec.file as usize].symbols[idx as usize],
        };
        let sym = &ctx.symbols[id];
        if r.ty == E::RELOC_GOTPC || E::RELOC_GOT_LOADS.contains(&r.ty) && !sym.can_relax_got(ctx) {
            return Some(self.got_slot(id));
        }
        if r.is_func_call::<E>() {
            if sym.is_interposable(ctx)
                && let Some(stub) = sym.stub_idx(&ctx.symbols)
            {
                return Some(self.chunk(ChunkId::Stubs, stubs::entry_offset::<E>(stub)));
            }
            let (n, off) = self.sym(id)?;
            return Some((n, off.wrapping_add_signed(addend)));
        }
        let (n, off) = self.own_sym(id)?;
        Some((n, off.wrapping_add_signed(addend)))
    }

    /// The references of one input subsection's relocations.
    fn isec_entries(&self, id: usize, out: &mut Vec<Entry>) {
        let ctx = self.ctx;
        let isec = &ctx.isecs[id];
        let Some(chunk) = isec.output_section().filter(|_| isec.is_alive()) else {
            return;
        };
        let hdr = ctx.chunk_header(chunk);
        let file = &ctx.objs[isec.file as usize];
        let rels = isec.rels(file);
        let mut i = 0;
        while i < rels.len() {
            let r = &rels[i];
            let from = (hdr.sect_idx, isec.offset as u64 + r.offset as u64);
            let pointer = if r.size == 8 {
                DYLD_CACHE_ADJ_V2_POINTER_64
            } else {
                DYLD_CACHE_ADJ_V2_POINTER_32
            };
            match E::split_ref(r.ty) {
                // The UNSIGNED record that follows names the target.
                SplitRef::Subtractor => {
                    i += 1;
                    let kind = if r.size == 8 {
                        DYLD_CACHE_ADJ_V2_DELTA_64
                    } else {
                        DYLD_CACHE_ADJ_V2_DELTA_32
                    };
                    push(out, from, kind, self.reloc_target(isec, &rels[i], false));
                }
                // A thread-local's descriptor holds an offset into the
                // thread-local template, which doesn't move.
                SplitRef::Pointer if r.refers_to_tls(ctx, file) => {}
                SplitRef::Pointer => push(out, from, pointer, self.reloc_target(isec, r, true)),
                split => {
                    // A GOT load of a lazy or delay-init dylib's symbol
                    // that calls a helper instead: an arm64 site is a
                    // branch now, and ld-prime keeps an x86-64 one at
                    // its old displacement's place.
                    let site = (id as u32, r.offset);
                    let lazy = ctx.lazy_helpers.sites.get(&site).map(|&i| self.lazy_helper(i));
                    let delay =
                        ctx.delay_init.sites.get(&site).map(|&i| self.delay_helper(i as usize));
                    let (split, to) = match lazy.or(delay) {
                        Some(helper) if split == SplitRef::Page => {
                            (SplitRef::Branch26, Some(helper))
                        }
                        Some(helper) => (split, Some(helper)),
                        None => (split, self.reloc_target(isec, r, true)),
                    };
                    let kind = match split {
                        SplitRef::Page => DYLD_CACHE_ADJ_V2_ARM64_ADRP,
                        SplitRef::PageOff => DYLD_CACHE_ADJ_V2_ARM64_OFF12,
                        SplitRef::Branch26 => DYLD_CACHE_ADJ_V2_ARM64_BR26,
                        _ => DYLD_CACHE_ADJ_V2_DELTA_32,
                    };
                    if to.is_some_and(|to| to.0 != from.0) {
                        push(out, from, kind, to);
                    }
                }
            }
            i += 1;
        }
    }

    /// An address the linker's own code materializes PC-relatively at
    /// `from`, when it reaches another section.
    pub(crate) fn pcrel(&self, out: &mut Vec<Entry>, from: Place, to: Option<Place>) {
        if to.is_some_and(|to| to.0 != from.0) {
            for (i, &kind) in E::SPLIT_PCREL_KINDS.iter().enumerate() {
                push(out, (from.0, from.1 + 4 * i as u64), kind, to);
            }
        }
    }

    /// Where __delay_helper's load helper `i` lies.
    pub(crate) fn delay_helper(&self, i: usize) -> Place {
        self.chunk_addr(ChunkId::DelayHelper, self.ctx.delay_init.helper_addr(i))
    }

    pub(crate) fn objc_ref(&self, r: ObjcRef) -> Option<Place> {
        match r {
            ObjcRef::Isec(isec, off) => self.isec(isec as usize).map(|(n, o)| (n, o + off)),
            ObjcRef::Sym(id, addend) => {
                self.own_sym(id).map(|(n, o)| (n, o.wrapping_add_signed(addend)))
            }
            ObjcRef::TailSelref(i) => Some(self.selref(i)),
            ObjcRef::Null => None,
        }
    }

    /// Slot `i` of the synthesized selector references.
    pub(crate) fn selref(&self, i: usize) -> Place {
        let osec = self.ctx.output_section(self.ctx.objc_stubs.selrefs.unwrap());
        (osec.hdr.sect_idx, osec.tail_off + i as u64 * 8)
    }

    /// __init_offsets: an image offset per initializer.
    fn table_entries(&self, out: &mut Vec<Entry>) {
        let ctx = self.ctx;
        if !ctx.chunks.contains(&ChunkId::InitOffsets) {
            return;
        }
        for (i, &func) in ctx.init_offsets.init_funcs.iter().enumerate() {
            let InitFunc::Local(isec, off) = func else { continue };
            let from = self.chunk(ChunkId::InitOffsets, i as u64 * 4);
            let to = self.isec(isec).map(|(n, o)| (n, o + off));
            push(out, from, DYLD_CACHE_ADJ_V2_IMAGE_OFF_32, to);
        }
    }

    /// __unwind_info's image offsets: the personalities' GOT slots,
    /// the first function of each second-level page and the end of
    /// the last one, and the (function, LSDA) pairs.
    fn unwind_entries(&self, out: &mut Vec<Entry>) {
        let ctx = self.ctx;
        if !ctx.chunks.contains(&ChunkId::UnwindInfo) {
            return;
        }
        let buf = &ctx.unwind_info.contents;
        let word = |off: usize| u32::from_le_bytes(buf[off..off + 4].try_into().unwrap());
        let from = |off: usize| self.chunk(ChunkId::UnwindInfo, off as u64);
        let kind = DYLD_CACHE_ADJ_V2_IMAGE_OFF_32;
        let personalities = word(12) as usize;
        for (i, &id) in ctx.unwind_info.personalities.iter().enumerate() {
            push(out, from(personalities + 4 * i), kind, Some(self.got_slot(id)));
        }
        let (index, count) = (word(20) as usize, word(24) as usize);
        for i in 0..count {
            let off = index + 12 * i;
            push(out, from(off), kind, self.image_offset(word(off), i + 1 == count));
        }
        if count > 0 {
            let lsdas = word(index + 8) as usize..word(index + 12 * (count - 1) + 8) as usize;
            for off in lsdas.step_by(4) {
                push(out, from(off), kind, self.image_offset(word(off), false));
            }
        }
    }

    /// __eh_frame: each CIE's personality pointer (to its GOT slot),
    /// and each FDE's function and LSDA. (An FDE's CIE pointer stays in
    /// the section.)
    fn eh_frame_entries(&self, out: &mut Vec<Entry>) {
        let ctx = self.ctx;
        if !ctx.chunks.contains(&ChunkId::EhFrame) {
            return;
        }
        let at = |off: u64| self.chunk(ChunkId::EhFrame, off);
        for cie in ctx.cies.iter().filter(|cie| cie.is_alive) {
            if let Some(id) = cie.personality {
                let from = at((cie.output_offset + cie.personality_offset) as u64);
                push(out, from, DYLD_CACHE_ADJ_V2_DELTA_32, Some(self.got_slot(id)));
            }
        }
        for fde in &ctx.fdes {
            let off = fde.output_offset as u64;
            let cie = &ctx.cies[fde.cie as usize];
            let func = self.isec(fde.isec as usize).map(|(n, o)| (n, o + fde.func_offset as u64));
            let kind = match cie.pc_size() {
                4 => DYLD_CACHE_ADJ_V2_DELTA_32,
                _ => DYLD_CACHE_ADJ_V2_DELTA_64,
            };
            push(out, at(off + 8), kind, func);
            if let Some((isec, lsda_off)) = fde.lsda {
                let pos = fde.lsda_pos(cie.pc_size()) as u64;
                let kind = match cie.lsda_size() {
                    8 => DYLD_CACHE_ADJ_V2_DELTA_64,
                    _ => DYLD_CACHE_ADJ_V2_DELTA_32,
                };
                let to = self.isec(isec as usize).map(|(n, o)| (n, o + lsda_off as u64));
                push(out, at(off + pos), kind, to);
            }
        }
    }
}
