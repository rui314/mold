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

use crate::chunks::init_offsets::InitFunc;
use crate::chunks::{ChunkHeader, ChunkId};
use crate::context::Context;
use crate::input_files::FileId;
use crate::input_sections::{InputSection, Reloc, RelocTarget};
use crate::macho_consts::*;
use crate::objc::{DataField, ObjcRef};
use crate::symbol::{NO_IDX, SymbolId};
use crate::target::{RelocClass, SplitRef, Target};
use crate::util::encode_uleb;

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

/// One reference, in the V2 format's terms. The fields are in the
/// order the format groups references by: (from, to) section, then
/// target offset, then kind.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Entry {
    from_sect: u8,
    to_sect: u8,
    to_off: u64,
    kind: u8,
    from_off: u64,
}

/// A place in the image: a section's ordinal (0 for the mach header)
/// and an offset in it.
type Place = (u8, u64);

fn push(out: &mut Vec<Entry>, from: Place, kind: u8, to: Option<Place>) {
    if let Some((to_sect, to_off)) = to {
        out.push(Entry { from_sect: from.0, to_sect, to_off, kind, from_off: from.1 });
    }
}

pub fn build<E: Target>(ctx: &Context<E>) -> Vec<u8> {
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
    places.stub_entries(&mut entries);
    places.lazy_entries(&mut entries);
    places.delay_entries(&mut entries);
    places.objc_entries(&mut entries);
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
struct Places<'a, E: Target> {
    ctx: &'a Context<E>,
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
    fn chunk(&self, id: ChunkId, off: u64) -> Place {
        let hdr = self.ctx.chunk_header(id);
        (hdr.sect_idx, hdr.addr - self.starts[hdr.sect_idx as usize] + off)
    }

    /// Where a subsection lies, if it is laid out.
    fn isec(&self, id: usize) -> Option<Place> {
        let isec = &self.ctx.isecs[self.ctx.resolve_isec(id)];
        let chunk = isec.output_section()?;
        (isec.offset != u32::MAX).then(|| self.chunk(chunk, isec.offset as u64))
    }

    /// Where a symbol's address lies, as Context::sym_addr resolves it
    /// (a dylib symbol at its stub); None for an absolute symbol. The
    /// linker's own sectionless symbols are the layout boundaries and
    /// the mach header's names (___dso_handle, __mh_*_header).
    fn sym(&self, id: SymbolId) -> Option<Place> {
        let ctx = self.ctx;
        let sym = &ctx.symbols[id];
        let aux = ctx.sym_aux(id);
        match sym.file()? {
            FileId::Dylib(_) if aux.lazy_stub_idx != NO_IDX => {
                Some(self.lazy_helper(aux.lazy_stub_idx))
            }
            FileId::Dylib(_) if aux.delay_stub_idx != NO_IDX => Some(
                self.chunk(ChunkId::DelayStubs, aux.delay_stub_idx as u64 * E::DELAY_STUB_SIZE),
            ),
            FileId::Dylib(_) => (aux.stub_idx != NO_IDX)
                .then(|| self.chunk(ChunkId::Stubs, aux.stub_idx as u64 * E::STUB_SIZE)),
            FileId::Obj(obj) => {
                if let Some(isec) = sym.input_section() {
                    let (n, off) = self.isec(isec as usize)?;
                    Some((n, off + sym.value))
                } else if aux.objc_stub_idx != NO_IDX {
                    Some(
                        self.chunk(
                            ChunkId::ObjcStubs,
                            aux.objc_stub_idx as u64 * ctx.objc_stub_size(),
                        ),
                    )
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
    fn own_sym(&self, id: SymbolId) -> Option<Place> {
        if self.ctx.symbols[id].is_imported() { None } else { self.sym(id) }
    }

    /// A symbol's GOT slot, or a lazy dylib's symbol's __lazy_load_got
    /// slot.
    fn got_slot(&self, id: SymbolId) -> Place {
        let aux = self.ctx.sym_aux(id);
        if aux.got_idx == NO_IDX && aux.lazy_got_idx != NO_IDX {
            return self.chunk(ChunkId::LazyLoadGot, aux.lazy_got_idx as u64 * 8);
        }
        self.got_index(aux.got_idx as usize)
    }

    /// Where __lazy_helpers entry `i` lies.
    fn lazy_helper(&self, i: u32) -> Place {
        let offset = self.ctx.lazy_helpers.helpers[i as usize].offset;
        self.chunk(ChunkId::LazyHelpers, offset as u64)
    }

    fn got_index(&self, i: usize) -> Place {
        self.chunk(ChunkId::Got, i as u64 * 8)
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

    /// Where relocation `r` of a subsection points, as apply_relocs
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
        match E::classify_reloc(r.ty) {
            RelocClass::Got => return Some(self.got_slot(id)),
            RelocClass::GotLoad | RelocClass::Tlv if !ctx.can_relax_got(id) => {
                return Some(self.got_slot(id));
            }
            RelocClass::Branch => {
                let stub = ctx.sym_aux(id).stub_idx;
                if ctx.is_interposable(id) && stub != NO_IDX {
                    return Some(self.chunk(ChunkId::Stubs, stub as u64 * E::STUB_SIZE));
                }
                let (n, off) = self.sym(id)?;
                return Some((n, off.wrapping_add_signed(addend)));
            }
            _ => {}
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
        let rels = ctx.isec_relocs(id);
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
                SplitRef::Pointer if ctx.reloc_target_is_tls(isec.file as usize, r) => {}
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
    fn pcrel(&self, out: &mut Vec<Entry>, from: Place, to: Option<Place>) {
        if to.is_some_and(|to| to.0 != from.0) {
            for (i, &kind) in E::SPLIT_PCREL_KINDS.iter().enumerate() {
                push(out, (from.0, from.1 + 4 * i as u64), kind, to);
            }
        }
    }

    /// __stubs, the lazy pointers and __stub_helper, the GOT, and the
    /// range-extension thunks.
    fn stub_entries(&self, out: &mut Vec<Entry>) {
        let ctx = self.ctx;
        let has = |id| ctx.chunks.contains(&id);
        if has(ChunkId::Stubs) {
            for (i, &id) in ctx.stubs.symbols.iter().enumerate() {
                let slot = if ctx.args.lazy_binding && !ctx.binds_weak_lookup(id) {
                    let lazy = ctx.stubs.lazy.binary_search(&(i as u32)).unwrap();
                    self.chunk(ChunkId::LazyPtrs, lazy as u64 * 8)
                } else {
                    self.got_slot(id)
                };
                let from = self.chunk(ChunkId::Stubs, i as u64 * E::STUB_SIZE + E::STUB_REF_OFF);
                self.pcrel(out, from, Some(slot));
            }
        }
        if has(ChunkId::LazyPtrs) {
            for i in 0..ctx.stubs.lazy.len() as u64 {
                let helper = E::STUB_HELPER_HEADER_SIZE + i * E::STUB_HELPER_ENTRY_SIZE;
                let to = self.chunk(ChunkId::StubHelper, helper);
                push(
                    out,
                    self.chunk(ChunkId::LazyPtrs, i * 8),
                    DYLD_CACHE_ADJ_V2_POINTER_64,
                    Some(to),
                );
            }
        }
        if has(ChunkId::StubHelper) {
            let helper = &ctx.stub_helper;
            let [private, binder] = E::STUB_HELPER_REF_OFFS;
            let to = self.isec(helper.dyld_private_isec as usize);
            self.pcrel(out, self.chunk(ChunkId::StubHelper, private), to);
            let to = helper.dyld_stub_binder.map(|id| self.got_slot(id));
            self.pcrel(out, self.chunk(ChunkId::StubHelper, binder), to);
        }
        for (i, &id) in ctx.got.got_syms.iter().enumerate() {
            push(out, self.got_index(i), DYLD_CACHE_ADJ_V2_POINTER_64, self.own_sym(id));
        }
        for osec in &ctx.output_sections {
            for thunk in &osec.thunks {
                for (i, &id) in thunk.syms.iter().enumerate() {
                    let from = (osec.hdr.sect_idx, thunk.offset + i as u64 * E::THUNK_SIZE);
                    self.pcrel(out, from, self.sym(id));
                }
            }
        }
    }

    /// The lazy-load helpers' references: to the flag word and slot
    /// they check and load, to the arguments and the stub of the call
    /// of __dyld_lazy_load, and to the code after the site they return
    /// to (see LazyTarget).
    fn lazy_entries(&self, out: &mut Vec<Entry>) {
        use crate::chunks::lazy_helpers::{LazyTarget, LazyUse};
        let ctx = self.ctx;
        let lazy = &ctx.lazy_helpers;
        let Some(lazy_load) = lazy.dyld_lazy_load else { return };
        let stub =
            self.chunk(ChunkId::Stubs, ctx.sym_aux(lazy_load).stub_idx as u64 * E::STUB_SIZE);
        for (i, h) in lazy.helpers.iter().enumerate() {
            let (n, at) = self.lazy_helper(i as u32);
            for (off, kind, to) in E::lazy_helper_refs(h.kind) {
                let to = match to {
                    LazyTarget::Flag => self.isec(h.flag as usize),
                    LazyTarget::Slot => Some(self.chunk(ChunkId::LazyLoadGot, h.slot as u64 * 8)),
                    LazyTarget::Header => Some((0, 0)),
                    LazyTarget::LazyLoad => Some(stub),
                    LazyTarget::Site => match h.kind {
                        LazyUse::Load { site: Some((isec, off)), .. } => {
                            self.isec(isec as usize).map(|(n, o)| (n, o + off as u64 + 4))
                        }
                        _ => None,
                    },
                };
                push(out, (n, at + off as u64), kind, to);
            }
        }
    }

    /// Where __delay_helper's load helper `i` lies.
    fn delay_helper(&self, i: usize) -> Place {
        let offset = self.ctx.delay_init.helpers[i].offset;
        self.chunk(ChunkId::DelayHelper, offset as u64)
    }

    /// The delay-init stubs' and helpers' references to other sections
    /// (see DelayTarget).
    fn delay_entries(&self, out: &mut Vec<Entry>) {
        use crate::chunks::delay_init::{DelayCode, DelayTarget, DelayUse};
        let ctx = self.ctx;
        let delay = &ctx.delay_init;
        let Some(dlopen) = delay.dlopen_sym else { return };
        let dlopen_stub = ctx.sym_aux(dlopen).stub_idx as u64 * E::STUB_SIZE;
        let dlopen_helper =
            |i: u32| self.chunk(ChunkId::DelayHelper, delay.dlopens[i as usize].offset as u64);
        let mut push_refs = |from: Place, code: DelayCode, resolve: &dyn Fn(DelayTarget) -> _| {
            for (off, kind, to) in E::delay_refs(code) {
                let from = (from.0, from.1 + off as u64);
                let to: Option<Place> = resolve(to);
                if to.is_some_and(|to| to.0 != from.0) {
                    push(out, from, kind, to);
                }
            }
        };
        for (i, stub) in delay.stubs.iter().enumerate() {
            let from = self.chunk(ChunkId::DelayStubs, i as u64 * E::DELAY_STUB_SIZE);
            let flag = delay.dlopens[stub.dlopen as usize].flag;
            push_refs(from, DelayCode::Stub, &|to| match to {
                DelayTarget::Flag => self.isec(flag as usize),
                DelayTarget::Slot => Some(self.got_index(stub.got as usize)),
                DelayTarget::DlopenHelper => Some(dlopen_helper(stub.dlopen)),
                _ => None,
            });
        }
        for (i, h) in delay.helpers.iter().enumerate() {
            let flag = delay.dlopens[h.dlopen as usize].flag;
            push_refs(self.delay_helper(i), DelayCode::Helper(h.kind), &|to| match to {
                DelayTarget::Flag => self.isec(flag as usize),
                DelayTarget::Slot => Some(self.got_slot(h.sym)),
                DelayTarget::DlopenHelper => Some(dlopen_helper(h.dlopen)),
                DelayTarget::Site => match h.kind {
                    DelayUse::Load { site: Some((isec, off)), .. } => {
                        self.isec(isec as usize).map(|(n, o)| (n, o + off as u64 + 4))
                    }
                    _ => None,
                },
                _ => None,
            });
        }
        for (i, d) in delay.dlopens.iter().enumerate() {
            push_refs(dlopen_helper(i as u32), DelayCode::Dlopen, &|to| match to {
                DelayTarget::Flag => self.isec(d.flag as usize),
                DelayTarget::Name => self.isec(d.string as usize),
                DelayTarget::Dlopen => Some(self.chunk(ChunkId::Stubs, dlopen_stub)),
                _ => None,
            });
        }
    }

    fn objc_ref(&self, r: ObjcRef) -> Option<Place> {
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
    fn selref(&self, i: usize) -> Place {
        let osec = self.ctx.output_section(self.ctx.objc_stubs.selrefs.unwrap());
        (osec.hdr.sect_idx, osec.tail_off + i as u64 * 8)
    }

    /// __objc_stubs, the selector references synthesized for them and
    /// for method lists, the synthesized Objective-C records, and the
    /// relative method lists.
    fn objc_entries(&self, out: &mut Vec<Entry>) {
        let ctx = self.ctx;
        let stubs = &ctx.objc_stubs;
        if ctx.chunks.contains(&ChunkId::ObjcStubs) {
            let [sel, msgsend] = E::OBJC_STUB_REF_OFFS;
            let msgsend_slot = self.got_slot(stubs.msgsend_sym.unwrap());
            for i in 0..stubs.symbols.len() {
                let at = i as u64 * ctx.objc_stub_size();
                self.pcrel(out, self.chunk(ChunkId::ObjcStubs, at + sel), Some(self.selref(i)));
                self.pcrel(out, self.chunk(ChunkId::ObjcStubs, at + msgsend), Some(msgsend_slot));
            }
        }
        if stubs.selrefs.is_some() {
            let n = stubs.symbols.len();
            for (i, &off) in stubs.methname_offs.iter().enumerate() {
                let methname = ctx.output_section(stubs.methname.unwrap());
                let name = (methname.hdr.sect_idx, methname.tail_off + off);
                push(out, self.selref(i), DYLD_CACHE_ADJ_V2_POINTER_64, Some(name));
            }
            for (j, &name) in stubs.extra_selrefs.iter().enumerate() {
                let to = self.isec(name as usize);
                push(out, self.selref(n + j), DYLD_CACHE_ADJ_V2_POINTER_64, to);
            }
        }
        for blob in &ctx.data_blobs {
            let Some((n, mut at)) = self.isec(blob.isec as usize) else {
                continue;
            };
            for field in &blob.fields {
                match field {
                    DataField::Bytes(bytes) => at += bytes.len() as u64,
                    DataField::Ptr(r) => {
                        push(out, (n, at), DYLD_CACHE_ADJ_V2_POINTER_64, self.objc_ref(*r));
                        at += 8;
                    }
                }
            }
        }
        for list in &ctx.objc_methlist.lists {
            let Some((n, base)) = self.isec(list.isec as usize) else {
                continue;
            };
            for (i, m) in list.methods.iter().enumerate() {
                for (k, r) in [m.name, m.types, m.imp].into_iter().enumerate() {
                    let from = (n, base + 8 + 12 * i as u64 + 4 * k as u64);
                    push(out, from, DYLD_CACHE_ADJ_V2_DELTA_32, self.objc_ref(r));
                }
            }
        }
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
                let pos = crate::chunks::eh_frame::lsda_pos(fde.data, cie.pc_size()) as u64;
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
