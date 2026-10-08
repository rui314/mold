//! The Objective-C passes: the rewrites ld-prime makes to an image's
//! Objective-C metadata, in the order the driver runs them.
//!
//! - coalesce_objc_refs keeps one of the selector references, class
//!   references and CFStrings the compiler emits once per object.
//! - create_objc_msgsend_stubs and scan_objc_stubs synthesize the
//!   _objc_msgSend$<selector> stubs, with their selector references
//!   (chunks/objc_stubs.rs writes them).
//! - fold_objc_classrefs turns class references into GOT loads (macOS
//!   15 on).
//! - convert_objc_method_lists rewrites the method lists in the
//!   relative form (macOS 11 on; chunks/objc_methlist.rs writes them).
//! - merge_objc_categories merges the categories of a class defined in
//!   the image into the class.
//!
//! The passes read the metadata the way dyld sees it, through
//! relocations: a pointer field is the 8-byte relocation at its offset
//! (objc_pointer_at), leading to the subsection and offset it points
//! at (objc_ref_location). What they synthesize refers to its targets
//! by ObjcRef, resolved to an address when the output is written.

use rayon::prelude::*;

use crate::arch::Target;
use crate::chunks::got::add_classref_stand_ins;
use crate::chunks::objc_methlist::add_relative_method_list;
use crate::context::Context;
use crate::input_files::{
    DataField, FileId, add_data_blob, add_synthetic_section, redirect_symbols_to_replacements,
};
use crate::input_sections::{Reloc, RelocTarget};
use crate::macho::*;
use crate::symbol::{NEEDS_GOT, NEEDS_STUB};

/// A reference held by a rewritten method-list entry, resolved to an
/// address when the list is written.
#[derive(Clone, Copy, Debug)]
pub enum ObjcRef {
    /// A subsection plus offset.
    Isec(u32, u64),
    /// A symbol plus addend.
    Sym(crate::symbol::SymbolId, i64),
    /// Slot `n` of the synthesized selector references in the
    /// __objc_selrefs tail (the objc stubs' slots come first).
    TailSelref(usize),
    Null,
}

#[derive(Clone, Copy, Debug)]
pub struct ObjcMethod {
    /// The selector reference the entry points at (a slot holding the
    /// uniqued selector), not the selector string.
    pub name: ObjcRef,
    pub types: ObjcRef,
    pub imp: ObjcRef,
}

#[derive(Debug)]
pub struct ObjcMethList {
    /// The synthetic subsection standing for the rewritten list in
    /// __TEXT,__objc_methlist.
    pub isec: u32,
    pub methods: Vec<ObjcMethod>,
}

impl ObjcRef {
    /// The import a reference is to, which dyld binds.
    pub fn import<E: Target>(self, ctx: &Context<E>) -> Option<crate::symbol::SymbolId> {
        match self {
            ObjcRef::Sym(id, _) if ctx.symbols[id].is_imported() => Some(id),
            _ => None,
        }
    }

    /// The address a reference in a synthesized record or a rewritten
    /// method list resolves to, once the output is laid out.
    pub fn addr<E: Target>(self, ctx: &Context<E>) -> u64 {
        match self {
            ObjcRef::Isec(isec, off) => ctx.isecs[isec as usize].addr(ctx) + off,
            ObjcRef::Sym(id, addend) => (ctx.symbols[id].addr(ctx) as i64 + addend) as u64,
            ObjcRef::TailSelref(n) => ctx.objc_stubs.selref_addr(ctx, n),
            ObjcRef::Null => 0,
        }
    }
}

/// A new __TEXT,__objc_methlist section of the internal object, for
/// method lists rewritten in the relative form.
fn add_methlist_section<E: Target>(ctx: &mut Context<E>) -> (u32, u32) {
    let hdr = MachSection {
        sectname: bytes_to_name(b"__objc_methlist"),
        segname: bytes_to_name(b"__TEXT"),
        p2align: 2,
        flags: S_REGULAR,
        ..Default::default()
    };
    add_synthetic_section(ctx, hdr)
}

/// The relocation of the pointer field at `off` in a subsection, the
/// 8-byte one there, as (object, index into its relocation arena).
/// A subsection's relocations are sorted by offset, so a binary search
/// finds it: a method list's fields are looked up one by one, and a
/// linear search made long lists quadratic.
fn objc_pointer_reloc<E: Target>(ctx: &Context<E>, isec: u32, off: u64) -> Option<(usize, usize)> {
    let sec = &ctx.isecs[isec as usize];
    if ctx.is_internal(sec.file as usize) {
        return None;
    }
    let rels = sec.rels(&ctx.objs[sec.file as usize]);
    let start = rels.partition_point(|r| (r.offset as u64) < off);
    let k = rels[start..]
        .iter()
        .take_while(|r| r.offset as u64 == off)
        .position(|r| r.size == 8 && !r.is_pcrel && !r.is_subtracted)?;
    Some((sec.file as usize, sec.rel_offset as usize + start + k))
}

/// The pointer stored at `off` in a subsection: the target of the
/// 8-byte relocation there, if any.
fn objc_pointer_at<E: Target>(ctx: &Context<E>, isec: u32, off: u64) -> Option<ObjcRef> {
    let (obj, k) = objc_pointer_reloc(ctx, isec, off)?;
    let rel = &ctx.objs[obj].relocs[k];
    if rel.ty != E::RELOC_UNSIGNED {
        return None;
    }
    Some(match rel.target() {
        RelocTarget::Sym(idx) => ObjcRef::Sym(ctx.objs[obj].symbols[idx as usize], rel.addend),
        RelocTarget::Section(t) => ObjcRef::Isec(t, rel.addend as u64),
    })
}

/// A reference's location as (live subsection, offset), for data
/// defined in this link; None for an import or an absolute.
fn objc_ref_location<E: Target>(ctx: &Context<E>, r: ObjcRef) -> Option<(u32, u64)> {
    let (isec, off) = match r {
        ObjcRef::Isec(isec, off) => (isec, off),
        ObjcRef::Sym(id, addend) => {
            let sym = &ctx.symbols[id];
            let isec = sym.input_section()?;
            (isec, (sym.value as i64 + addend) as u64)
        }
        _ => return None,
    };
    let isec = ctx.isecs.resolve(isec as usize) as u32;
    if !ctx.isecs[isec as usize].is_alive() {
        return None;
    }
    Some((isec, off))
}

/// The pointers in the 8-byte slots of a list section such as
/// __objc_classlist (None where a slot has none).
pub(crate) fn list_entries<E: Target>(
    ctx: &Context<E>,
    isec: u32,
) -> impl Iterator<Item = Option<ObjcRef>> {
    let size = ctx.isecs[isec as usize].size as u64;
    (0..size).step_by(8).map(move |off| objc_pointer_at(ctx, isec, off))
}

/// The live subsections of the input sections named one of `names`, in
/// subsection order, each with the index of its section's name. The
/// objects' section headers say which sections those are, a file at a
/// time in parallel, so that only their subsections are visited (found
/// by address among the object's), not all of the link's.
fn subsecs_of_sections<E: Target>(ctx: &Context<E>, names: &[&[u8]]) -> Vec<(u32, usize)> {
    let mut found: Vec<(u32, usize)> = (ctx.objs.par_iter().enumerate())
        .filter(|&(i, _)| !ctx.is_internal(i))
        .flat_map_iter(|(_, obj)| {
            let sects = obj.sect_hdrs.iter().enumerate().filter_map(|(shndx, hdr)| {
                Some((shndx as u32, hdr, names.iter().position(|&name| hdr.sectname_is(name))?))
            });
            sects.flat_map(move |(shndx, hdr, kind)| {
                let isec = |id: &u32| &ctx.isecs[*id as usize];
                let start =
                    obj.subsecs.partition_point(|id| (isec(id).input_addr as u64) < hdr.addr);
                obj.subsecs[start..]
                    .iter()
                    .take_while(move |id| isec(id).input_addr as u64 <= hdr.addr + hdr.size)
                    .filter(move |id| isec(id).shndx == shndx && isec(id).is_alive())
                    .map(move |&id| (id, kind))
            })
        })
        .collect();
    found.sort_unstable();
    found
}

/// A class's ro data: class_t.data at offset 32, whose low two bits a
/// Swift class uses as flags (FAST_IS_SWIFT_STABLE), so the record
/// itself sits at the pointer with those bits cleared.
fn objc_class_ro<E: Target>(ctx: &Context<E>, cls: (u32, u64)) -> Option<(u32, u64)> {
    let (isec, off) =
        objc_pointer_at(ctx, cls.0, cls.1 + 32).and_then(|r| objc_ref_location(ctx, r))?;
    Some((isec, off & !3))
}

/// The C string a reference points at, if it is in the image.
fn objc_cstring_at<E: Target>(ctx: &Context<E>, r: Option<ObjcRef>) -> Option<&'static [u8]> {
    let (isec, off) = objc_ref_location(ctx, r?)?;
    let data = ctx.isecs[isec as usize].contents();
    let bytes = data.get(off as usize..)?;
    let end = bytes.iter().position(|&b| b == 0)?;
    Some(&bytes[..end])
}

/// Coalesces the Objective-C reference records the compiler emits
/// once per object: __objc_selrefs entries naming the same selector
/// (of the literal-pointer type, see has_unnamed_subsecs),
/// __objc_classrefs entries naming the same class, and identical
/// __cfstring constants. ld64 keeps one of each in a final link
/// (NetNewsWire's debug dylib had 592 selector references too many);
/// the first copy wins and the rest redirect to it, like merged
/// literals. (A -r link leaves them all to the final link.) From macOS
/// 15 on, class references are left to fold_objc_classrefs, which
/// coalesces those nothing refers to. __objc_superrefs and
/// __objc_protorefs entries of one class or protocol coalesce too, but
/// for those a symbol names (see mark_labeled_literals).
///
/// The records' keys are found in parallel, in subsection order, then
/// the first of each key is kept in a serial walk.
pub fn coalesce_objc_refs<E: Target>(ctx: &mut Context<E>) {
    // The sections ref_key reads.
    let sects: [&[u8]; 5] = [
        b"__objc_selrefs",
        b"__objc_classrefs",
        b"__objc_superrefs",
        b"__objc_protorefs",
        b"__cfstring",
    ];
    let keys: Vec<(u32, RefKey)> = (subsecs_of_sections(ctx, &sects).par_iter())
        .filter_map(|&(i, _)| Some((i, ref_key(ctx, i as usize)?)))
        .collect();

    let mut first: hashbrown::HashMap<RefKey, u32> = hashbrown::HashMap::with_capacity(keys.len());
    let mut folds: Vec<(usize, u32)> = Vec::new();
    for (i, key) in keys {
        match first.entry(key) {
            hashbrown::hash_map::Entry::Occupied(e) => folds.push((i as usize, *e.get())),
            hashbrown::hash_map::Entry::Vacant(e) => {
                e.insert(i);
            }
        }
    }
    if folds.is_empty() {
        return;
    }
    for (loser, winner) in folds {
        let p2align = ctx.isecs[loser].p2align;
        ctx.isecs[loser].replacement = winner;
        let w = &mut ctx.isecs[winner as usize];
        w.p2align = w.p2align.max(p2align);
    }
    redirect_symbols_to_replacements(ctx);
}

/// What a reference record's pointer refers to: a place in a subsection
/// (where identical content has already been merged), or a symbol
/// defined elsewhere.
#[derive(Hash, PartialEq, Eq)]
enum RefTarget {
    At(usize, i64),
    Sym(crate::symbol::SymbolId, i64),
}

/// What makes two reference records of a section the same, for
/// coalesce_objc_refs.
#[derive(Hash, PartialEq, Eq)]
enum RefKey {
    Sel(RefTarget),
    Class(crate::symbol::SymbolId),
    Super(RefTarget),
    Proto(RefTarget),
    CfString(Vec<u8>, Vec<(u32, RefTarget)>),
}

/// What relocation `rel` of object `obj` refers to (see RefTarget).
fn ref_target<E: Target>(ctx: &Context<E>, obj: usize, rel: &Reloc) -> RefTarget {
    match rel.target() {
        RelocTarget::Section(t) => RefTarget::At(ctx.isecs.resolve(t as usize), rel.addend),
        RelocTarget::Sym(idx) => {
            let sym_id = ctx.objs[obj].symbols[idx as usize];
            let sym = &ctx.symbols[sym_id];
            match sym.input_section() {
                Some(isec) => {
                    RefTarget::At(ctx.isecs.resolve(isec as usize), sym.value as i64 + rel.addend)
                }
                None => RefTarget::Sym(sym_id, rel.addend),
            }
        }
    }
}

/// The key subsection `i` coalesces by, if it is a reference record
/// coalesce_objc_refs coalesces.
fn ref_key<E: Target>(ctx: &Context<E>, i: usize) -> Option<RefKey> {
    let isec = &ctx.isecs[i];
    if !isec.is_emitted() || ctx.is_internal(isec.file as usize) {
        return None;
    }
    let obj = isec.file as usize;
    let h = isec.hdr(&ctx.objs[obj]);
    if h.segname() != b"__DATA" {
        return None;
    }
    let rels = isec.rels(&ctx.objs[obj]);
    let plain_ptr = |rel: &Reloc| {
        rel.ty == E::RELOC_UNSIGNED && rel.size == 8 && !rel.is_pcrel && !rel.is_subtracted
    };
    match h.sectname() {
        b"__objc_selrefs" if h.section_type() != S_LITERAL_POINTERS => None,
        b"__objc_selrefs" => {
            if isec.size != 8 || rels.len() != 1 || !plain_ptr(&rels[0]) {
                return None;
            }
            Some(RefKey::Sel(ref_target(ctx, obj, &rels[0])))
        }
        b"__objc_classrefs" if folds_objc_classrefs(ctx) => None,
        b"__objc_classrefs" => {
            let idx = pointer_target(ctx, i)?;
            Some(RefKey::Class(ctx.objs[obj].symbols[idx as usize]))
        }
        b"__objc_superrefs" | b"__objc_protorefs" => {
            if isec.is_labeled() || isec.size != 8 || rels.len() != 1 || !plain_ptr(&rels[0]) {
                return None;
            }
            let target = ref_target(ctx, obj, &rels[0]);
            if h.sectname() == b"__objc_superrefs" {
                Some(RefKey::Super(target))
            } else {
                Some(RefKey::Proto(target))
            }
        }
        b"__cfstring" => {
            if isec.size != 32 || !rels.iter().all(plain_ptr) {
                return None;
            }
            let mut targets: Vec<(u32, RefTarget)> =
                rels.iter().map(|rel| (rel.offset, ref_target(ctx, obj, rel))).collect();
            targets.sort_by_key(|t| t.0);
            // The relocated fields hold per-object addends (x86-64
            // embeds the target's address); the targets stand for them.
            let mut bytes = isec.contents().to_vec();
            for rel in rels {
                let (a, b) = (rel.offset as usize, rel.offset as usize + rel.size as usize);
                bytes[a..b].fill(0);
            }
            Some(RefKey::CfString(bytes, targets))
        }
        _ => None,
    }
}

/// Synthesizes _objc_msgSend$<selector> stubs. With selector stubs
/// (the default since Xcode 14), the compiler calls these
/// linker-provided symbols instead of setting up the selector argument
/// itself; each stub loads the interned selector and tail-calls
/// _objc_msgSend. ld-prime lays the stubs out sorted by selector,
/// bytewise, and their selector references in the same order.
pub fn create_objc_msgsend_stubs<E: Target>(ctx: &mut Context<E>) {
    let internal = ctx.internal_obj.expect("internal object not created yet") as u32;
    let mut stubs: Vec<(u32, &'static [u8])> = Vec::new();
    for i in 0..ctx.symbols.syms.len() {
        let sym = &ctx.symbols[i];
        if sym.is_defined() || !sym.is_used() {
            continue;
        }
        if let Some(sel) = sym.name().strip_prefix(b"_objc_msgSend$") {
            stubs.push((i as u32, sel));
        }
    }
    stubs.sort_by(|a, b| a.1.cmp(b.1));
    for (idx, &(i, _)) in stubs.iter().enumerate() {
        ctx.symbols[i].set_file(FileId::Obj(internal));
        ctx.symbols.aux_mut(i).objc_stub_idx = idx as u32;
    }
    ctx.objc_stubs.symbols = stubs;

    if !ctx.objc_stubs.symbols.is_empty() {
        let id = ctx.symbols.intern(b"_objc_msgSend");
        ctx.symbols[id].set_used(true);
        ctx.objc_stubs.msgsend_sym = Some(id);

        // The stub machinery itself references _objc_msgSend; resolve
        // it now, since regular resolution has already run.
        if !ctx.symbols[id].is_defined()
            && let Some(dylib) =
                ctx.dylibs.iter().position(|d| d.exports.contains(&b"_objc_msgSend"[..]))
        {
            let sym = &mut ctx.symbols[id];
            sym.set_file(FileId::Dylib((dylib) as u32));
            sym.set_imported(true);
            sym.set_extern(true);
        }
    }
}

/// Drops the _objc_msgSend$<selector> stubs only code -dead_strip took
/// out called (their symbols unmarked by the live references, see
/// dead_strip::mark_live_references), as ld-prime makes none for them,
/// nor their selector references and names.
pub fn drop_dead_objc_stubs<E: Target>(ctx: &mut Context<E>) {
    let stubs = std::mem::take(&mut ctx.objc_stubs.symbols);
    let (live, dead): (Vec<_>, Vec<_>) =
        stubs.into_iter().partition(|&(id, _)| ctx.symbols[id].is_marked());
    for (id, _) in dead {
        ctx.symbols[id].clear_file();
    }
    for (idx, &(id, _)) in live.iter().enumerate() {
        ctx.symbols.aux_mut(id).objc_stub_idx = idx as u32;
    }
    if live.is_empty() {
        ctx.objc_stubs.msgsend_sym = None;
    }
    ctx.objc_stubs.symbols = live;
}

/// The synthesized objc stubs call _objc_msgSend through its GOT slot,
/// which other GOT loads of it share. Small stubs branch to it instead,
/// as a call in the code would: to its __stubs entry if it is imported.
/// (passes::scan_relocations makes the slot or the stub.)
pub fn scan_objc_stubs<E: Target>(ctx: &mut Context<E>) {
    if let Some(id) = ctx.objc_stubs.msgsend_sym {
        let sym = &ctx.symbols[id];
        if !ctx.args.objc_stubs_small {
            sym.add_flags(NEEDS_GOT);
        } else if sym.binds_as_import(ctx) || sym.binds_weak_lookup(ctx) {
            sym.add_flags(NEEDS_STUB);
        }
    }

    // Stub i loads slot i of the __objc_selrefs tail, which points at
    // its selector's name in the __objc_methname tail. (The runtime
    // uniques the selectors, so an input's reference to the same one
    // reads the same.)
    let stubs = &mut ctx.objc_stubs;
    for i in 0..stubs.symbols.len() {
        let sel = stubs.symbols[i].1;
        stubs.methname_offs.push(stubs.methname_data.len() as u64);
        stubs.methname_data.extend_from_slice(sel);
        stubs.methname_data.push(0);
    }
}

/// Folds __objc_classrefs into __got, as ld-prime does from a
/// deployment target of macOS 15 on. A class reference is an 8-byte
/// slot holding a class's address, which dyld fixes up - what a GOT
/// entry for the class symbol is. So the code that loads a class from
/// its slot (adrp/ldr on arm64, a RIP-relative mov on x86-64) is
/// retargeted at the class symbol as a GOT load: an imported class is
/// read from its GOT entry, which every reference in the image shares,
/// and the load of a class the image defines relaxes to computing its
/// address (adrp/add, lea), with no slot at all. The slots go, with
/// their local symbols (_OBJC_CLASSLIST_REFERENCES_$_n): the runtime
/// read them at load only to remap references to classes it swapped,
/// which macOS 15's dyld does through the GOT. A GOT load also reaches
/// a delay-init dylib's class through its load helper, which dlopen()s
/// the dylib first (see delay_init::create_delay_init), where a slot,
/// bound at launch, can't be delayed.
///
/// A reference that can't become a GOT load (one that takes the slot's
/// address, or a pointer to the slot) keeps the slot, replaced by a
/// stand-in for the class's GOT entry (see
/// chunks::got::add_classref_stand_ins). On arm64 the loads of a class
/// are rewritten only if each adrp of its slots is followed in its
/// subsection by one page-offset use before the next adrp of it (-O0
/// code can load twice through one adrp); if any reference of the
/// class, in any object, pairs up otherwise, all of them keep their
/// slots, and so read the GOT entry.
///
/// What folds is what is referenced: the slots of a class nothing
/// refers to through one stay in __objc_classrefs, coalesced as below
/// macOS 15, and the class gets no GOT entry. A slot that points at its
/// class section-relatively, as an x86-64 object's does at a class only
/// a temporary label names, stays too.
pub fn fold_objc_classrefs<E: Target>(ctx: &mut Context<E>) {
    if !folds_objc_classrefs(ctx) {
        return;
    }
    let ctx_ref: &Context<E> = ctx;
    let slots: Vec<_> =
        (0..ctx.objs.len()).into_par_iter().map(|i| classref_slots(ctx_ref, i)).collect();
    let uses: Vec<_> =
        (0..ctx.objs.len()).into_par_iter().map(|i| classref_uses(ctx_ref, i, &slots[i])).collect();
    let mut unpaired = hashbrown::HashSet::new();
    let offset_half: Vec<_> =
        (0..ctx.objs.len()).map(|i| pair_classref_uses(ctx, i, &uses[i], &mut unpaired)).collect();
    let referenced: hashbrown::HashSet<_> = uses.iter().flatten().map(|u| u.class).collect();

    let mut unreferenced = hashbrown::HashMap::new();
    let mut coalesced = false;
    let mut kept = Vec::new();
    for obj_idx in 0..ctx.objs.len() {
        // Rewrite the loads; note the slots something else refers to.
        // A pair's halves go together, as its offset half decides.
        let mut keep = hashbrown::HashSet::new();
        for u in &uses[obj_idx] {
            let data = ctx.isecs[u.isec as usize].contents();
            let relocs = &ctx.objs[obj_idx].relocs;
            let decider = offset_half[obj_idx].get(&u.k).copied().unwrap_or(u.k);
            let (rel, decider) = (relocs[u.k], relocs[decider]);
            match E::got_load_form(rel.ty, data, rel.offset) {
                Some(form)
                    if !unpaired.contains(&u.class)
                        && E::got_load_form(decider.ty, data, decider.offset).is_some() =>
                {
                    let r = &mut ctx.objs[obj_idx].relocs[u.k];
                    r.ty = form;
                    r.set_target(RelocTarget::Sym(slots[obj_idx][&u.slot].0));
                }
                _ => {
                    keep.insert(u.slot);
                }
            }
        }

        // The slots in input order: the GOT entries the classes get
        // follow it, not the hash map's order.
        let mut ordered: Vec<(u32, crate::symbol::SymbolId)> =
            slots[obj_idx].iter().map(|(&slot, &(_, class))| (slot, class)).collect();
        ordered.sort_unstable_by_key(|&(slot, _)| slot);
        for (slot, class) in ordered {
            if !referenced.contains(&class) {
                let first = *unreferenced.entry(class).or_insert(slot);
                if first != slot {
                    ctx.isecs[slot as usize].replacement = first;
                    coalesced = true;
                }
                continue;
            }
            // The loads rewritten above give the class a GOT entry if
            // it needs one, as passes::scan_relocations finds; a slot
            // that stays reads the entry, so it needs one anyway.
            if keep.contains(&slot) {
                ctx.symbols[class].add_flags(NEEDS_GOT);
                kept.push((slot, class));
            } else {
                ctx.isecs[slot as usize].kill();
            }
        }
    }
    add_classref_stand_ins(ctx, kept);
    if coalesced {
        redirect_symbols_to_replacements(ctx);
    }
}

/// Whether the link folds class references into __got (see
/// fold_objc_classrefs): one for macOS 15, iOS 18, visionOS 2 or later,
/// of an image dyld loads (ld-prime optimizes the Objective-C of no
/// other).
fn folds_objc_classrefs<E: Target>(ctx: &Context<E>) -> bool {
    !ctx.args.without_dyld() && ctx.args.targets(&crate::cmdline::VERSION_2024_FALL)
}

/// The symbol, by its index in the object, that subsection `i` is a
/// plain pointer to: 8 bytes an 8-byte absolute relocation of the
/// symbol fills, with no addend.
fn pointer_target<E: Target>(ctx: &Context<E>, i: usize) -> Option<u32> {
    let isec = &ctx.isecs[i];
    let [rel] = isec.rels(&ctx.objs[isec.file as usize]) else { return None };
    let RelocTarget::Sym(idx) = rel.target() else { return None };
    let plain = rel.ty == E::RELOC_UNSIGNED
        && ctx.isecs[i].size == 8
        && rel.size == 8
        && !rel.is_pcrel
        && !rel.is_subtracted
        && rel.addend == 0;
    plain.then_some(idx)
}

/// An object's class reference slots that point at a symbol: slot
/// subsection -> the class symbol, by its index in the object and
/// globally.
fn classref_slots<E: Target>(
    ctx: &Context<E>,
    obj_idx: usize,
) -> hashbrown::HashMap<u32, (u32, crate::symbol::SymbolId)> {
    let obj = &ctx.objs[obj_idx];
    if !obj.is_reachable {
        return hashbrown::HashMap::new();
    }
    obj.subsecs
        .iter()
        .filter_map(|&i| {
            let isec = &ctx.isecs[i];
            let h = isec.hdr(obj);
            if !isec.is_emitted() || h.segname() != b"__DATA" || h.sectname() != b"__objc_classrefs"
            {
                return None;
            }
            let idx = pointer_target(ctx, i as usize)?;
            Some((i, (idx, obj.symbols[idx as usize])))
        })
        .collect()
}

/// A reference to a class reference slot: relocation `k` of the object
/// (an index into its relocations), in subsection `isec`.
struct ClassrefUse {
    isec: u32,
    k: usize,
    slot: u32,
    class: crate::symbol::SymbolId,
}

/// The references an object's code and data make to its class
/// reference slots, in subsection and then relocation order.
fn classref_uses<E: Target>(
    ctx: &Context<E>,
    obj_idx: usize,
    slots: &hashbrown::HashMap<u32, (u32, crate::symbol::SymbolId)>,
) -> Vec<ClassrefUse> {
    let mut uses = Vec::new();
    if slots.is_empty() {
        return uses;
    }
    let obj = &ctx.objs[obj_idx];
    for &i in &obj.subsecs {
        let isec = &ctx.isecs[i];
        if !isec.is_alive() || slots.contains_key(&i) {
            continue;
        }
        for (k, rel) in isec.rels(obj).iter().enumerate() {
            let slot = match rel.target() {
                RelocTarget::Section(t) => t,
                RelocTarget::Sym(idx) => {
                    let sym = &ctx.symbols[obj.symbols[idx as usize]];
                    match sym.input_section() {
                        Some(t) if sym.value == 0 => t,
                        _ => continue,
                    }
                }
            };
            if rel.addend == 0
                && let Some(&(_, class)) = slots.get(&slot)
            {
                uses.push(ClassrefUse { isec: i, k: isec.rel_offset as usize + k, slot, class });
            }
        }
    }
    uses
}

/// Pairs an object's page-and-offset references to class slots (arm64's
/// adrp then ldr or add), each page half with the next offset half of
/// the same class in its subsection, and maps the page half of each
/// pair to its offset half, by relocation index. A class with a half
/// left over joins `unpaired`.
fn pair_classref_uses<E: Target>(
    ctx: &Context<E>,
    obj_idx: usize,
    uses: &[ClassrefUse],
    unpaired: &mut hashbrown::HashSet<crate::symbol::SymbolId>,
) -> hashbrown::HashMap<usize, usize> {
    let mut offset_half = hashbrown::HashMap::new();
    let mut open: hashbrown::HashMap<crate::symbol::SymbolId, usize> = hashbrown::HashMap::new();
    for (n, u) in uses.iter().enumerate() {
        match E::page_pair_half(ctx.objs[obj_idx].relocs[u.k].ty) {
            Some(true) => {
                if open.insert(u.class, u.k).is_some() {
                    unpaired.insert(u.class);
                }
            }
            Some(false) => match open.remove(&u.class) {
                Some(page) => {
                    offset_half.insert(page, u.k);
                }
                None => {
                    unpaired.insert(u.class);
                }
            },
            None => {}
        }
        if uses.get(n + 1).is_none_or(|next| next.isec != u.isec) {
            unpaired.extend(open.drain().map(|(class, _)| class));
        }
    }
    offset_half
}

/// Rewrites the Objective-C method lists in the relative form, as
/// ld64 does from a deployment target of macOS 11 (its
/// -objc_relative_method_lists). A classic entry is three pointers
/// (selector string, type string, implementation), each a fixup dyld
/// must apply and the runtime must then unique; a relative entry is
/// three 32-bit self-relative offsets, the first to a selector
/// reference slot (already uniqued by dyld), needing no fixups at
/// all, and the lists move to read-only __TEXT,__objc_methlist.
///
/// The lists are found the way the runtime finds them: through
/// __objc_classlist (class and metaclass ro data), __objc_catlist,
/// __objc_protolist (all four lists of a protocol) and Swift's
/// __objc_clsrolist and __objc_catlist2 (the categories of classes
/// the runtime reaches through a Swift class stub, which Swift puts
/// in __objc_data). A selector with no reference in any input gets
/// one synthesized in the __objc_selrefs tail. A list is left alone
/// when it is not the whole of its subsection or not in the classic
/// 24-byte form.
pub fn convert_objc_method_lists<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.relocatable || !ctx.args.objc_relative_method_lists {
        return;
    }
    let lists = runtime_method_lists(ctx);
    if lists.is_empty() {
        return;
    }

    // The lists are read in parallel; their selectors then get their
    // references, and the lists their subsections, in list order.
    let classic: Vec<_> = lists.par_iter().map(|&list| classic_methods(ctx, list)).collect();
    let mut selrefs = SelrefFinder::new(ctx);
    let sect = add_methlist_section(ctx);
    let mut offset: u64 = 0;
    let mut repoint: hashbrown::HashMap<u32, u32> = hashbrown::HashMap::new();
    for (list, methods) in lists.into_iter().zip(classic) {
        let Some(methods) = methods else { continue };
        let methods = (methods.into_iter())
            .map(|(sel, types, imp)| ObjcMethod { name: selrefs.get(ctx, sel), types, imp })
            .collect();
        let synth = add_relative_method_list(ctx, sect, &mut offset, methods);
        ctx.isecs[list as usize].replacement = synth;
        repoint.insert(list, synth);
    }

    // The lists' own symbols (__OBJC_$_INSTANCE_METHODS_Foo ...) follow
    // them into __objc_methlist.
    ctx.symbols.syms.par_iter_mut().for_each(|sym| {
        if let Some(isec) = sym.input_section()
            && let Some(&synth) = repoint.get(&isec)
        {
            sym.set_input_section(Some(synth));
        }
    });
}

/// Every method list the runtime would visit, each once, in the order
/// the list sections lead to them. Blocks of the list subsections are
/// followed in parallel, one not knowing what the others visit, and the
/// first visit of each list counts.
fn runtime_method_lists<E: Target>(ctx: &Context<E>) -> Vec<u32> {
    // The sections visit_list_section follows.
    let sects: [&[u8]; 7] = [
        b"__objc_classlist",
        b"__objc_nlclslist",
        b"__objc_catlist",
        b"__objc_catlist2",
        b"__objc_nlcatlist",
        b"__objc_protolist",
        b"__objc_clsrolist",
    ];
    let found: Vec<Vec<u32>> = (subsecs_of_sections(ctx, &sects).par_chunks(16))
        .map(|block| {
            let mut found = MethodListFinder::default();
            for &(i, _) in block {
                found.visit_list_section(ctx, i);
            }
            found.lists
        })
        .collect();
    let mut seen = hashbrown::HashSet::new();
    found.into_iter().flatten().filter(|&list| seen.insert(list)).collect()
}

/// The method lists found so far, and the classes visited.
#[derive(Default)]
struct MethodListFinder {
    lists: Vec<u32>,
    seen: hashbrown::HashSet<u32>,
    classes_seen: hashbrown::HashSet<(u32, u64)>,
}

impl MethodListFinder {
    /// Notes the method lists subsection `i` leads to if it is one of a
    /// list section's.
    fn visit_list_section<E: Target>(&mut self, ctx: &Context<E>, i: u32) {
        let isec = &ctx.isecs[i];
        if !isec.is_alive() || ctx.is_internal(isec.file as usize) {
            return;
        }
        let h = isec.hdr(&ctx.objs[isec.file as usize]);
        if !h.segname().starts_with(b"__DATA") {
            return;
        }
        let records = || list_entries(ctx, i).filter_map(|r| objc_ref_location(ctx, r?));
        match h.sectname() {
            b"__objc_classlist" | b"__objc_nlclslist" => {
                for cls in records() {
                    self.visit_class(ctx, cls);
                }
            }
            b"__objc_catlist" | b"__objc_catlist2" | b"__objc_nlcatlist" => {
                // category_t: name, cls, instanceMethods, classMethods.
                for cat in records() {
                    self.note(ctx, cat, 16);
                    self.note(ctx, cat, 24);
                }
            }
            b"__objc_protolist" => {
                // protocol_t: isa, name, protocols, then the four method
                // lists.
                for proto in records() {
                    for field in [24, 32, 40, 48] {
                        self.note(ctx, proto, field);
                    }
                }
            }
            b"__objc_clsrolist" => {
                // class_ro_t: baseMethods at 32.
                for ro in records() {
                    self.note(ctx, ro, 32);
                }
            }
            _ => {}
        }
    }

    /// Notes the method list a record's pointer field at `field` points
    /// at, if it is the start of a subsection.
    fn note<E: Target>(&mut self, ctx: &Context<E>, rec: (u32, u64), field: u64) {
        let list = objc_pointer_at(ctx, rec.0, rec.1 + field);
        if let Some((isec, 0)) = list.and_then(|r| objc_ref_location(ctx, r))
            && self.seen.insert(isec)
        {
            self.lists.push(isec);
        }
    }

    /// Notes a class's method list, then its metaclass's.
    fn visit_class<E: Target>(&mut self, ctx: &Context<E>, cls: (u32, u64)) {
        if !self.classes_seen.insert(cls) {
            return;
        }
        // class_t: isa, superclass, cache, vtable, data (the ro). A
        // Swift class's class_t starts past its metadata's prefix, not
        // at the start of its subsection.
        if let Some(ro) = objc_class_ro(ctx, cls) {
            // class_ro_t: baseMethods at 32.
            self.note(ctx, ro, 32);
        }
        if let Some(meta) =
            objc_pointer_at(ctx, cls.0, cls.1).and_then(|r| objc_ref_location(ctx, r))
        {
            self.visit_class(ctx, meta);
        }
    }
}

/// The selector reference a relative method-list entry points at, for
/// the selector string it names: an input's, else a new one in the
/// __objc_selrefs tail.
struct SelrefFinder {
    /// The inputs' selector references, by the selector string
    /// subsection they point at.
    input: hashbrown::HashMap<u32, u32>,
    /// The slots added to the tail, by selector string subsection.
    extra: hashbrown::HashMap<u32, usize>,
}

impl SelrefFinder {
    /// The inputs' selector references are found in parallel; the first
    /// to a selector string is the one its relative entries use.
    fn new<E: Target>(ctx: &Context<E>) -> Self {
        let refs: Vec<(u32, u32)> = (subsecs_of_sections(ctx, &[b"__objc_selrefs"]).par_iter())
            .filter_map(|&(i, _)| {
                let isec = &ctx.isecs[i as usize];
                if isec.size != 8
                    || isec.hdr(&ctx.objs[isec.file as usize]).section_type() != S_LITERAL_POINTERS
                {
                    return None;
                }
                match objc_ref_location(ctx, objc_pointer_at(ctx, i, 0)?) {
                    Some((name, 0)) => Some((name, ctx.isecs.resolve(i as usize) as u32)),
                    _ => None,
                }
            })
            .collect();
        let mut input = hashbrown::HashMap::with_capacity(refs.len());
        for (name, slot) in refs {
            input.entry(name).or_insert(slot);
        }
        Self { input, extra: hashbrown::HashMap::new() }
    }

    fn get<E: Target>(&mut self, ctx: &mut Context<E>, sel: u32) -> ObjcRef {
        if let Some(&slot) = self.input.get(&sel) {
            return ObjcRef::Isec(slot, 0);
        }
        let n = *self.extra.entry(sel).or_insert_with(|| {
            ctx.objc_stubs.extra_selrefs.push(sel);
            ctx.objc_stubs.extra_selrefs.len() - 1
        });
        ObjcRef::TailSelref(ctx.objc_stubs.symbols.len() + n)
    }
}

/// A classic method list's header: its entsize field, which holds flags
/// in its high bits, and its number of entries, if as many 24-byte
/// entries fill the rest of the list's subsection, `data`.
fn classic_list_header(data: &[u8]) -> Option<(u32, u64)> {
    let entsize_flags = u32::from_le_bytes(data.get(0..4)?.try_into().unwrap());
    let count = u32::from_le_bytes(data.get(4..8)?.try_into().unwrap()) as u64;
    (8 + 24 * count == data.len() as u64).then_some((entsize_flags, count))
}

/// The methods of a classic method list: each one's selector string
/// (a subsection), types and implementation; None if the list is not
/// the whole of its subsection in the classic 24-byte form, or an
/// entry's selector is not a string in the image - a list left absolute
/// then, whose selectors need no references.
fn classic_methods<E: Target>(ctx: &Context<E>, list: u32) -> Option<Vec<(u32, ObjcRef, ObjcRef)>> {
    // Not in the relative form already (the top flag), and 24 bytes an
    // entry.
    let (entsize_flags, count) = classic_list_header(ctx.isecs[list as usize].contents())?;
    if entsize_flags & 0x8000_0000 != 0 || entsize_flags & 0xffff != 24 {
        return None;
    }
    (0..count)
        .map(|i| {
            let at = 8 + 24 * i;
            let name = objc_pointer_at(ctx, list, at);
            let (sel, 0) = name.and_then(|r| objc_ref_location(ctx, r))? else { return None };
            let types = objc_pointer_at(ctx, list, at + 8).unwrap_or(ObjcRef::Null);
            let imp = objc_pointer_at(ctx, list, at + 16).unwrap_or(ObjcRef::Null);
            Some((sel, types, imp))
        })
        .collect()
}

/// Merges the categories of a class defined in the image into the
/// class itself, as ld64 does unless -no_objc_category_merging:
/// the runtime then has no categories to attach at load. The merged
/// method list holds the categories' methods, last category first,
/// then the class's own (a category's method precedes the class's,
/// as after attachment); the protocol list likewise; the property
/// lists take the categories in order, then the class's. The
/// class_ro_t records are rewritten to point at the merged lists
/// (their symbols follow), the categories leave __objc_catlist, and a
/// class that absorbed a +load category joins __objc_nlclslist. A
/// category whose data is not in the expected shape is left alone.
/// Runs after the method lists have been rewritten in relative form,
/// when it merges those; with classic lists the merged list is a
/// classic one.
pub fn merge_objc_categories<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.relocatable || !ctx.args.objc_category_merging {
        return;
    }
    let relative = ctx.args.objc_relative_method_lists;
    let (mut classes, class_idx) = defined_classes(ctx);
    let (mut cats, catlists) = find_categories(ctx, &mut classes, &class_idx);
    if cats.is_empty() {
        return;
    }

    let mut writer = MergedListWriter::new(ctx, relative);
    let mut nonlazy_classes: Vec<(u32, u64)> = Vec::new();
    for class in &classes {
        if class.cats.is_empty() {
            continue;
        }
        // The runtime calls the first +load of a class's method list
        // alone: ld-prime leaves a class's categories be when merging
        // them would put more than one there.
        let loads = class.cats.iter().filter(|&&ci| cats[ci].nonlazy).count();
        if loads + usize::from(class.nonlazy) > 1 {
            continue;
        }

        // Gather everything first, and give up on the class if anything
        // is not in shape: nothing changes until everything checks out.
        let own = ListRefs::of_class(ctx, class);
        let cat_lists: Vec<ListRefs> =
            class.cats.iter().map(|&ci| ListRefs::of_category(ctx, cats[ci].isec)).collect();
        if !ro_rewritable(ctx, class) {
            continue;
        }
        let Some(lists) = merge_lists(ctx, &own, &cat_lists, relative) else { continue };

        // Write the merged lists, named after the class and its
        // categories, and drop the lists they supersede.
        let class_name = objc_cstring_at(ctx, objc_pointer_at(ctx, class.ro.0, class.ro.1 + 24))
            .unwrap_or_default();
        let cat_names: Vec<&[u8]> = class.cats.iter().map(|&ci| cats[ci].name).collect();
        let merged = writer.write(ctx, lists, &merged_list_suffix(class_name, &cat_names));
        drop_superseded_lists(ctx, &own, &merged, &cat_lists);

        // Point the class and its metaclass at new ro records holding
        // the merged lists.
        let ro = rewrite_ro(ctx, class.ro, merged.imethods, merged.protocols, merged.iprops);
        let meta_ro =
            rewrite_ro(ctx, class.meta_ro, merged.cmethods, merged.protocols, merged.cprops);
        retarget_class_data(ctx, class.cls, ro);
        retarget_class_data(ctx, class.meta, meta_ro);

        for &ci in &class.cats {
            ctx.isecs[cats[ci].isec as usize].kill();
            cats[ci].merged = true;
        }
        if !class.nonlazy && class.cats.iter().any(|&ci| cats[ci].nonlazy) {
            nonlazy_classes.push(class.cls);
        }
    }

    rebuild_category_lists(ctx, &catlists, &cats);

    // The rewritten method lists the merged ones superseded, dead now,
    // leave __objc_methlist, in one pass rather than one per list.
    ctx.objc_methlist.lists.retain(|l| ctx.isecs[l.isec as usize].is_alive());

    // Classes that absorbed a +load category become non-lazy.
    for cls in nonlazy_classes {
        add_data_blob(
            ctx,
            b"__objc_nlclslist",
            S_ATTR_NO_DEAD_STRIP,
            vec![DataField::Ptr(ObjcRef::Isec(cls.0, cls.1))],
        );
    }
}

/// A class defined in the image, by where its class_t, its metaclass's
/// and their class_ro_t records are.
struct DefinedClass {
    cls: (u32, u64),
    meta: (u32, u64),
    ro: (u32, u64),
    meta_ro: (u32, u64),
    /// Listed in __objc_nlclslist, as a class with a +load is.
    nonlazy: bool,
    /// The categories on it that can merge, in __objc_catlist order.
    cats: Vec<usize>,
}

/// The classes __objc_classlist and __objc_nlclslist list, in the order
/// first listed, and their index by class_t location. The list
/// subsections are read in parallel; a class listed more than once (in
/// both lists) then becomes one.
fn defined_classes<E: Target>(
    ctx: &Context<E>,
) -> (Vec<DefinedClass>, hashbrown::HashMap<(u32, u64), usize>) {
    // class_t: isa (the metaclass), superclass, cache, vtable, data (the
    // ro).
    let read = |cls: (u32, u64), nonlazy: bool| {
        let ro = objc_class_ro(ctx, cls)?;
        let meta = objc_ref_location(ctx, objc_pointer_at(ctx, cls.0, cls.1)?)?;
        let meta_ro = objc_class_ro(ctx, meta)?;
        Some(DefinedClass { cls, meta, ro, meta_ro, nonlazy, cats: Vec::new() })
    };
    let lists = subsecs_of_sections(ctx, &[b"__objc_classlist", b"__objc_nlclslist"]);
    let listed: Vec<Vec<DefinedClass>> = (lists.par_iter())
        .map(|&(i, kind)| {
            let entries = list_entries(ctx, i).filter_map(|r| objc_ref_location(ctx, r?));
            entries.filter_map(|cls| read(cls, kind == 1)).collect()
        })
        .collect();

    let mut classes: Vec<DefinedClass> = Vec::new();
    let mut class_idx: hashbrown::HashMap<(u32, u64), usize> = hashbrown::HashMap::new();
    for class in listed.into_iter().flatten() {
        match class_idx.entry(class.cls) {
            hashbrown::hash_map::Entry::Occupied(e) => classes[*e.get()].nonlazy |= class.nonlazy,
            hashbrown::hash_map::Entry::Vacant(e) => {
                e.insert(classes.len());
                classes.push(class);
            }
        }
    }
    (classes, class_idx)
}

/// A category on a class defined in the image.
struct Category {
    /// Its category_t, a subsection of its own.
    isec: u32,
    name: &'static [u8],
    /// Listed in __objc_nlcatlist too, as a category with a +load is.
    nonlazy: bool,
    merged: bool,
}

/// A subsection of __objc_catlist or __objc_nlcatlist: each entry's
/// pointer, and the category it names if one that can merge.
struct CategoryList {
    isec: u32,
    nonlazy: bool,
    entries: Vec<(ObjcRef, Option<usize>)>,
}

/// The categories on classes defined in the image, in __objc_catlist
/// order, each noted on its class, and the category-list subsections.
/// A category sits in __objc_catlist and, if it has a +load, in
/// __objc_nlcatlist too; a list subsection may hold several.
fn find_categories<E: Target>(
    ctx: &Context<E>,
    classes: &mut [DefinedClass],
    class_idx: &hashbrown::HashMap<(u32, u64), usize>,
) -> (Vec<Category>, Vec<CategoryList>) {
    let mut cats: Vec<Category> = Vec::new();
    let mut cat_idx: hashbrown::HashMap<u32, usize> = hashbrown::HashMap::new();
    let mut lists = Vec::new();
    for (i, kind) in subsecs_of_sections(ctx, &[b"__objc_catlist", b"__objc_nlcatlist"]) {
        let nonlazy = kind == 1;
        let mut list = CategoryList { isec: i, nonlazy, entries: Vec::new() };
        for r in list_entries(ctx, i) {
            let r = r.unwrap_or(ObjcRef::Null);
            let ci = category_and_class(ctx, r, class_idx).and_then(|(cat, class)| {
                if let Some(&ci) = cat_idx.get(&cat) {
                    return Some(ci);
                }
                let name = objc_cstring_at(ctx, objc_pointer_at(ctx, cat, 0))?;
                cats.push(Category { isec: cat, name, nonlazy: false, merged: false });
                cat_idx.insert(cat, cats.len() - 1);
                classes[class].cats.push(cats.len() - 1);
                Some(cats.len() - 1)
            });
            if let Some(ci) = ci {
                cats[ci].nonlazy |= nonlazy;
            }
            list.entries.push((r, ci));
        }
        lists.push(list);
    }
    (cats, lists)
}

/// The category a category-list entry points at and the class it
/// extends, by its index among the defined classes, if that class is
/// defined in the image and the category_t is a subsection of its own,
/// long enough to have instance properties.
fn category_and_class<E: Target>(
    ctx: &Context<E>,
    r: ObjcRef,
    class_idx: &hashbrown::HashMap<(u32, u64), usize>,
) -> Option<(u32, usize)> {
    let (cat, off) = objc_ref_location(ctx, r)?;
    if off != 0 || ctx.isecs[cat as usize].size < 48 {
        return None;
    }
    // category_t: name, cls, ...
    let cls = objc_pointer_at(ctx, cat, 8)?;
    Some((cat, *class_idx.get(&objc_ref_location(ctx, cls)?)?))
}

/// The five lists a category adds to its class, and a class has of its
/// own, as references to them (None: no list).
#[derive(Clone, Copy, Default)]
struct ListRefs {
    imethods: Option<ObjcRef>,
    cmethods: Option<ObjcRef>,
    protocols: Option<ObjcRef>,
    iprops: Option<ObjcRef>,
    cprops: Option<ObjcRef>,
    /// A class's metaclass's protocol list: the class's own in clang's
    /// metadata, a copy of it in Swift's.
    meta_protocols: Option<ObjcRef>,
}

impl ListRefs {
    /// A category's lists. category_t: name, cls, instanceMethods,
    /// classMethods, protocols, instanceProperties, then
    /// _classProperties, which a record from an older compiler lacks.
    fn of_category<E: Target>(ctx: &Context<E>, cat: u32) -> Self {
        let field = |off| objc_pointer_at(ctx, cat, off);
        let has_class_props = ctx.isecs[cat as usize].size >= 56;
        Self {
            imethods: field(16),
            cmethods: field(24),
            protocols: field(32),
            iprops: field(40),
            cprops: if has_class_props { field(48) } else { None },
            meta_protocols: None,
        }
    }

    /// A class's own lists: its ro record's, and for the class methods
    /// and properties its metaclass's (class_ro_t: baseMethods at 32,
    /// baseProtocols at 40, baseProperties at 64).
    fn of_class<E: Target>(ctx: &Context<E>, class: &DefinedClass) -> Self {
        let (ro, meta_ro) = (class.ro, class.meta_ro);
        Self {
            imethods: objc_pointer_at(ctx, ro.0, ro.1 + 32),
            cmethods: objc_pointer_at(ctx, meta_ro.0, meta_ro.1 + 32),
            protocols: objc_pointer_at(ctx, ro.0, ro.1 + 40),
            iprops: objc_pointer_at(ctx, ro.0, ro.1 + 64),
            cprops: objc_pointer_at(ctx, meta_ro.0, meta_ro.1 + 64),
            meta_protocols: objc_pointer_at(ctx, meta_ro.0, meta_ro.1 + 40),
        }
    }
}

/// A class's lists merged with its categories', each None where no
/// category has a list of that kind (the class keeps its own).
struct MergedLists {
    imethods: Option<Vec<ObjcMethod>>,
    cmethods: Option<Vec<ObjcMethod>>,
    protocols: Option<Vec<ObjcRef>>,
    iprops: Option<Vec<(ObjcRef, ObjcRef)>>,
    cprops: Option<Vec<(ObjcRef, ObjcRef)>>,
}

/// Merges a class's own lists with its categories' (given in
/// __objc_catlist order), or returns None if a list is not in a shape
/// we can read or holds a reference a synthesized record cannot.
fn merge_lists<E: Target>(
    ctx: &Context<E>,
    own: &ListRefs,
    cats: &[ListRefs],
    relative: bool,
) -> Option<MergedLists> {
    let mut imethods = Vec::new();
    let mut cmethods = Vec::new();
    let mut protocols = Vec::new();
    let mut iprops = Vec::new();
    let mut cprops = Vec::new();

    // The methods and protocols of the category attached last come
    // first; the properties take the categories in order. The class's
    // own lists follow.
    for c in cats.iter().rev() {
        imethods.extend(read_method_list(ctx, c.imethods, relative)?);
        cmethods.extend(read_method_list(ctx, c.cmethods, relative)?);
        protocols.extend(read_protocol_list(ctx, c.protocols)?);
    }
    for c in cats {
        iprops.extend(read_property_list(ctx, c.iprops)?);
        cprops.extend(read_property_list(ctx, c.cprops)?);
    }
    imethods.extend(read_method_list(ctx, own.imethods, relative)?);
    cmethods.extend(read_method_list(ctx, own.cmethods, relative)?);
    protocols.extend(read_protocol_list(ctx, own.protocols)?);
    iprops.extend(read_property_list(ctx, own.iprops)?);
    cprops.extend(read_property_list(ctx, own.cprops)?);

    // A synthesized record's pointers are plain rebases (a relative
    // method list's entries are not pointers).
    let local = |r: ObjcRef| points_into_image(ctx, r);
    if !relative
        && !imethods
            .iter()
            .chain(&cmethods)
            .all(|m| local(m.name) && local(m.types) && local(m.imp))
    {
        return None;
    }
    if !protocols.iter().all(|&r| local(r))
        || !iprops.iter().chain(&cprops).all(|&(name, attrs)| local(name) && local(attrs))
    {
        return None;
    }

    let any = |list: fn(&ListRefs) -> Option<ObjcRef>| cats.iter().any(|c| list(c).is_some());
    Some(MergedLists {
        imethods: any(|c| c.imethods).then_some(imethods),
        cmethods: any(|c| c.cmethods).then_some(cmethods),
        protocols: any(|c| c.protocols).then_some(protocols),
        iprops: any(|c| c.iprops).then_some(iprops),
        cprops: any(|c| c.cprops).then_some(cprops),
    })
}

/// A method list's methods (none for no list), or None if the list is
/// not in a shape we can merge: with relative method lists, one that
/// convert_objc_method_lists rewrote; otherwise a classic one of
/// 24-byte entries.
fn read_method_list<E: Target>(
    ctx: &Context<E>,
    list: Option<ObjcRef>,
    relative: bool,
) -> Option<Vec<ObjcMethod>> {
    let Some(r) = list else { return Some(Vec::new()) };
    let (isec, off) = objc_ref_location(ctx, r)?;
    if off != 0 {
        return None;
    }
    let lists = &ctx.objc_methlist.lists;
    if let Ok(k) = lists.binary_search_by_key(&isec, |l| l.isec) {
        return Some(lists[k].methods.clone());
    }
    if relative {
        return None;
    }
    let (entsize_flags, count) = classic_list_header(ctx.isecs[isec as usize].contents())?;
    if entsize_flags != 24 {
        return None;
    }
    let mut methods = Vec::new();
    for i in 0..count {
        let at = 8 + 24 * i;
        methods.push(ObjcMethod {
            name: objc_pointer_at(ctx, isec, at)?,
            types: objc_pointer_at(ctx, isec, at + 8).unwrap_or(ObjcRef::Null),
            imp: objc_pointer_at(ctx, isec, at + 16).unwrap_or(ObjcRef::Null),
        });
    }
    Some(methods)
}

/// A protocol list's protocols (none for no list): a count (8 bytes),
/// then pointers.
fn read_protocol_list<E: Target>(ctx: &Context<E>, list: Option<ObjcRef>) -> Option<Vec<ObjcRef>> {
    let Some(r) = list else { return Some(Vec::new()) };
    let (isec, off) = objc_ref_location(ctx, r)?;
    let data = ctx.isecs[isec as usize].contents();
    let count = u64::from_le_bytes(data.get(off as usize..off as usize + 8)?.try_into().unwrap());
    (0..count).map(|i| objc_pointer_at(ctx, isec, off + 8 + 8 * i)).collect()
}

/// A property list's properties (none for no list): entsize (16),
/// count, then (name, attributes) pairs.
fn read_property_list<E: Target>(
    ctx: &Context<E>,
    list: Option<ObjcRef>,
) -> Option<Vec<(ObjcRef, ObjcRef)>> {
    let Some(r) = list else { return Some(Vec::new()) };
    let (isec, off) = objc_ref_location(ctx, r)?;
    let data = ctx.isecs[isec as usize].contents();
    let entsize = u32::from_le_bytes(data.get(off as usize..off as usize + 4)?.try_into().unwrap());
    let count =
        u32::from_le_bytes(data.get(off as usize + 4..off as usize + 8)?.try_into().unwrap())
            as u64;
    if entsize != 16 {
        return None;
    }
    (0..count)
        .map(|i| {
            let at = off + 8 + 16 * i;
            Some((
                objc_pointer_at(ctx, isec, at)?,
                objc_pointer_at(ctx, isec, at + 8).unwrap_or(ObjcRef::Null),
            ))
        })
        .collect()
}

/// Whether a reference is to data in this image (or null), which a
/// synthesized record can hold as a plain rebase.
fn points_into_image<E: Target>(ctx: &Context<E>, r: ObjcRef) -> bool {
    match r {
        ObjcRef::Null | ObjcRef::Isec(..) | ObjcRef::TailSelref(_) => true,
        ObjcRef::Sym(id, _) => {
            !ctx.symbols[id].is_imported() && ctx.symbols[id].input_section().is_some()
        }
    }
}

/// Whether a class's ro records can be replaced: the class's and the
/// metaclass's data pointers must be rewritable, and both records as
/// long as the fields through baseProperties.
fn ro_rewritable<E: Target>(ctx: &Context<E>, class: &DefinedClass) -> bool {
    let long_enough = |(isec, off): (u32, u64)| ctx.isecs[isec as usize].size as u64 >= off + 72;
    objc_pointer_reloc(ctx, class.cls.0, class.cls.1 + 32).is_some()
        && objc_pointer_reloc(ctx, class.meta.0, class.meta.1 + 32).is_some()
        && long_enough(class.ro)
        && long_enough(class.meta_ro)
}

/// What the names of a class's merged lists end in: the class's name
/// and its categories', "Foo(A|B)".
fn merged_list_suffix(class_name: &[u8], cat_names: &[&[u8]]) -> Vec<u8> {
    [class_name, b"(", &cat_names.join(&b'|'), b")"].concat()
}

/// Writes merged lists out: relative method lists after
/// convert_objc_method_lists's in __TEXT,__objc_methlist (in a section
/// of their own, made on first use), or classic ones as records like
/// the other lists.
struct MergedListWriter {
    relative: bool,
    methlist_sect: Option<(u32, u32)>,
    methlist_off: u64,
}

impl MergedListWriter {
    fn new<E: Target>(ctx: &Context<E>, relative: bool) -> Self {
        let last = ctx.objc_methlist.lists.last().map(|l| &ctx.isecs[l.isec as usize]);
        Self {
            relative,
            methlist_sect: None,
            methlist_off: last.map_or(0, |isec| isec.offset as u64 + isec.size as u64),
        }
    }

    /// Writes a class's merged lists and returns references to them.
    /// ld64 names the method and protocol lists after the class and its
    /// categories, __OBJC_$_INSTANCE_METHODS_Foo(A|B), `suffix` being
    /// "Foo(A|B)".
    fn write<E: Target>(
        &mut self,
        ctx: &mut Context<E>,
        lists: MergedLists,
        suffix: &[u8],
    ) -> ListRefs {
        let name = |ctx: &mut Context<E>, prefix: &str, isec: u32| {
            let name = crate::util::leak_bytes([prefix.as_bytes(), suffix].concat());
            ctx.extra_local_syms.push((name, isec));
        };
        let mut refs = ListRefs::default();
        if let Some(methods) = lists.imethods {
            let isec = self.method_list(ctx, methods);
            name(ctx, "__OBJC_$_INSTANCE_METHODS_", isec);
            refs.imethods = Some(ObjcRef::Isec(isec, 0));
        }
        if let Some(methods) = lists.cmethods {
            let isec = self.method_list(ctx, methods);
            name(ctx, "__OBJC_$_CLASS_METHODS_", isec);
            refs.cmethods = Some(ObjcRef::Isec(isec, 0));
        }
        if let Some(protocols) = lists.protocols {
            let isec = add_protocol_list(ctx, &protocols);
            name(ctx, "__OBJC_CLASS_PROTOCOLS_$_", isec);
            refs.protocols = Some(ObjcRef::Isec(isec, 0));
        }
        if let Some(props) = lists.iprops {
            refs.iprops = Some(ObjcRef::Isec(add_property_list(ctx, &props), 0));
        }
        if let Some(props) = lists.cprops {
            refs.cprops = Some(ObjcRef::Isec(add_property_list(ctx, &props), 0));
        }
        refs
    }

    fn method_list<E: Target>(&mut self, ctx: &mut Context<E>, methods: Vec<ObjcMethod>) -> u32 {
        if self.relative {
            let sect = *self.methlist_sect.get_or_insert_with(|| add_methlist_section(ctx));
            return add_relative_method_list(ctx, sect, &mut self.methlist_off, methods);
        }
        // entsize (24), count, then (name, types, imp) triples.
        let mut fields = vec![
            DataField::Bytes(24u32.to_le_bytes().to_vec()),
            DataField::Bytes((methods.len() as u32).to_le_bytes().to_vec()),
        ];
        for m in &methods {
            fields.extend([DataField::Ptr(m.name), DataField::Ptr(m.types), DataField::Ptr(m.imp)]);
        }
        // ld-prime writes a merged absolute list into __objc_data (the
        // protocol and property lists stay in __objc_const).
        add_data_blob(ctx, b"__objc_data", 0, fields)
    }
}

/// Writes a protocol list, as read_protocol_list reads it.
fn add_protocol_list<E: Target>(ctx: &mut Context<E>, protocols: &[ObjcRef]) -> u32 {
    let mut fields = vec![DataField::Bytes((protocols.len() as u64).to_le_bytes().to_vec())];
    fields.extend(protocols.iter().map(|&r| DataField::Ptr(r)));
    add_data_blob(ctx, b"__objc_const", 0, fields)
}

/// Writes a property list, as read_property_list reads it.
fn add_property_list<E: Target>(ctx: &mut Context<E>, props: &[(ObjcRef, ObjcRef)]) -> u32 {
    let mut fields = vec![
        DataField::Bytes(16u32.to_le_bytes().to_vec()),
        DataField::Bytes((props.len() as u32).to_le_bytes().to_vec()),
    ];
    for &(name, attrs) in props {
        fields.extend([DataField::Ptr(name), DataField::Ptr(attrs)]);
    }
    add_data_blob(ctx, b"__objc_const", 0, fields)
}

/// Drops the lists the merged ones supersede - the class's own of each
/// kind merged, and all of the categories' - as ld64's output keeps
/// only the merged lists (which carry the names). A method list
/// convert_objc_method_lists rewrote leaves __objc_methlist once every
/// class has merged.
fn drop_superseded_lists<E: Target>(
    ctx: &mut Context<E>,
    own: &ListRefs,
    merged: &ListRefs,
    cats: &[ListRefs],
) {
    if merged.imethods.is_some() {
        drop_list(ctx, own.imethods);
    }
    if merged.cmethods.is_some() {
        drop_list(ctx, own.cmethods);
    }
    // The metaclass points at the merged protocol list too.
    if merged.protocols.is_some() {
        drop_list(ctx, own.protocols);
        drop_list(ctx, own.meta_protocols);
    }
    if merged.iprops.is_some() {
        drop_list(ctx, own.iprops);
    }
    if merged.cprops.is_some() {
        drop_list(ctx, own.cprops);
    }
    for c in cats {
        drop_list(ctx, c.imethods);
        drop_list(ctx, c.cmethods);
        drop_list(ctx, c.protocols);
        drop_list(ctx, c.iprops);
        drop_list(ctx, c.cprops);
    }
}

fn drop_list<E: Target>(ctx: &mut Context<E>, list: Option<ObjcRef>) {
    if let Some((isec, 0)) = list.and_then(|r| objc_ref_location(ctx, r)) {
        ctx.isecs[isec as usize].kill();
    }
}

/// Writes a copy of a class_ro_t record pointing at the merged lists
/// given, in place of the record's own, and returns it. class_ro_t:
/// flags, instanceStart, instanceSize, reserved, then ivarLayout, name,
/// baseMethods, baseProtocols, ivars, weakIvarLayout, baseProperties -
/// and, when the flags carry RO_HAS_SWIFT_INITIALIZER (1 << 6), a
/// Swift class's metadata initializer pointer at 72, which the runtime
/// calls while realizing the class (dropping it from the rewritten
/// record sent NetNewsWire's AppDelegate into a garbage address in
/// objc_copyClassList).
fn rewrite_ro<E: Target>(
    ctx: &mut Context<E>,
    ro: (u32, u64),
    methods: Option<ObjcRef>,
    protocols: Option<ObjcRef>,
    props: Option<ObjcRef>,
) -> u32 {
    let data = ctx.isecs[ro.0 as usize].contents()[ro.1 as usize..ro.1 as usize + 16].to_vec();
    let flags = u32::from_le_bytes(data[0..4].try_into().unwrap());
    let len = if flags & (1 << 6) != 0 { 80 } else { 72 };
    let mut fields = vec![DataField::Bytes(data)];
    for field in (16..len).step_by(8) {
        let merged = match field {
            32 => methods,
            40 => protocols,
            64 => props,
            _ => None,
        };
        let r = merged.or_else(|| objc_pointer_at(ctx, ro.0, ro.1 + field));
        fields.push(DataField::Ptr(r.unwrap_or(ObjcRef::Null)));
    }

    // The new record goes where the old one was: Swift puts a class's
    // ro data in __objc_data (ld64's output keeps __DATA__TtC...
    // there), clang's in __objc_const.
    let isec = &ctx.isecs[ro.0 as usize];
    let sect: &[u8] = match isec.hdr(&ctx.objs[isec.file as usize]).sectname() {
        b"__objc_data" => b"__objc_data",
        _ => b"__objc_const",
    };
    let blob = add_data_blob(ctx, sect, 0, fields);
    let isec = &mut ctx.isecs[ro.0 as usize];
    if ro.1 == 0 && isec.size as u64 == len {
        // The record was a subsection of its own: replace it, so its
        // symbol names the new record too (ld64 keeps
        // __OBJC_CLASS_RO_$_Foo).
        isec.kill();
        isec.replacement = blob;
    }
    blob
}

/// Points a class's data field (class_t.data, at 32) at its new ro
/// record, keeping the flag bits a Swift class stores in the pointer's
/// low bits.
fn retarget_class_data<E: Target>(ctx: &mut Context<E>, cls: (u32, u64), ro: u32) {
    let (obj, k) = objc_pointer_reloc(ctx, cls.0, cls.1 + 32).unwrap();
    let rel = ctx.objs[obj].relocs[k];
    let flags = match rel.target() {
        RelocTarget::Sym(idx) => {
            let id = ctx.objs[obj].symbols[idx as usize];
            (ctx.symbols[id].value as i64 + rel.addend) & 3
        }
        RelocTarget::Section(_) => rel.addend & 3,
    };
    let rel = &mut ctx.objs[obj].relocs[k];
    rel.set_target(RelocTarget::Section(ro));
    rel.addend = flags;
}

/// Takes the merged categories out of the category lists: a list
/// subsection all of whose entries merged goes away, one with
/// survivors is rewritten with those, in its place.
fn rebuild_category_lists<E: Target>(
    ctx: &mut Context<E>,
    lists: &[CategoryList],
    cats: &[Category],
) {
    let merged = |ci: Option<usize>| ci.is_some_and(|ci| cats[ci].merged);
    for list in lists {
        if !list.entries.iter().any(|&(_, ci)| merged(ci)) {
            continue;
        }
        ctx.isecs[list.isec as usize].kill();
        let survivors: Vec<DataField> = (list.entries.iter())
            .filter(|&&(_, ci)| !merged(ci))
            .map(|&(r, _)| DataField::Ptr(r))
            .collect();
        // The list keeps its place: ld-prime drops the merged entries
        // from it, so the categories of the objects after it still come
        // after its others.
        if !survivors.is_empty() {
            let sect: &[u8] = if list.nonlazy { b"__objc_nlcatlist" } else { b"__objc_catlist" };
            let blob = add_data_blob(ctx, sect, S_ATTR_NO_DEAD_STRIP, survivors);
            ctx.isecs[list.isec as usize].replacement = blob;
        }
    }
}
