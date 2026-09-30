//! The Objective-C passes: the rewrites ld-prime makes to an image's
//! Objective-C metadata, in the order the driver runs them.
//!
//! - coalesce_objc_refs keeps one of the selector references, class
//!   references and CFStrings the compiler emits once per object.
//! - create_objc_msgsend_stubs and scan_objc_stubs synthesize the
//!   _objc_msgSend$<selector> stubs, whose selector references take
//!   over the inputs' (chunks/objc_stubs.rs writes them).
//! - fold_objc_classrefs turns class references into GOT entries
//!   (macOS 15 on).
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

use crate::context::Context;
use crate::input_files::FileId;
use crate::input_sections::{InputSection, RelocTarget};
use crate::macho::*;
use crate::passes::{add_got, data_seg, objc_refs_are_const, redirect_symbols_to_replacements};
use crate::target::RelocClass;
use crate::target::Target;
use crate::util::align_to;

/// Coalesces the Objective-C reference records the compiler emits
/// once per object: __objc_selrefs entries naming the same selector,
/// __objc_classrefs entries naming the same class, and identical
/// __cfstring constants. ld64 keeps one of each, in a -r output as in
/// a final link (NetNewsWire's RSCore prelink had 56 class references
/// where ld-prime's has 30, its debug dylib 592 selector references
/// too many); the first copy wins and the rest redirect to it, like
/// merged literals. A final link leaves class references to
/// fold_objc_classrefs, which turns them into GOT slots.
pub fn coalesce_objc_refs<E: Target>(ctx: &mut Context<E>) {
    // What a pointer relocation refers to: a place in a subsection
    // (where identical content has already been merged), or a symbol
    // defined elsewhere.
    #[derive(Hash, PartialEq, Eq)]
    enum Target {
        At(usize, i64),
        Sym(crate::symbol::SymbolId, i64),
    }
    #[derive(Hash, PartialEq, Eq)]
    enum Key {
        Sel(Target),
        Class(crate::symbol::SymbolId),
        CfString(Vec<u8>, Vec<(u32, Target)>),
    }
    let place = |ctx: &Context<E>, obj: usize, rel: &crate::input_sections::Reloc| -> Target {
        match rel.target() {
            RelocTarget::Section(t) => Target::At(ctx.resolve_isec(t as usize), rel.addend),
            RelocTarget::Sym(idx) => {
                let sym_id = ctx.objs[obj].symbols[idx as usize];
                let sym = &ctx.symbols[sym_id];
                match sym.input_section() {
                    Some(isec) => {
                        Target::At(ctx.resolve_isec(isec as usize), sym.value as i64 + rel.addend)
                    }
                    None => Target::Sym(sym_id, rel.addend),
                }
            }
        }
    };
    let mut first: hashbrown::HashMap<Key, u32> = hashbrown::HashMap::new();
    let mut folds: Vec<(usize, u32)> = Vec::new();
    for i in 0..ctx.isecs.len() {
        let isec = &ctx.isecs[i];
        if !isec.is_alive()
            || isec.replacement != crate::input_sections::NO_REPLACEMENT
            || ctx.is_internal(isec.file as usize)
        {
            continue;
        }
        let h = ctx.hdr_of(isec);
        if h.segname() != "__DATA" {
            continue;
        }
        let obj = isec.file as usize;
        let rels = ctx.isec_relocs(i);
        let plain_ptr = |rel: &crate::input_sections::Reloc| {
            E::classify_reloc(rel.r_type) == RelocClass::Plain
                && rel.size == 8
                && !rel.is_pcrel
                && !rel.is_subtracted
        };
        let key = match h.sectname() {
            "__objc_classrefs" if !ctx.args.relocatable && objc_refs_are_const(ctx) => continue,
            "__objc_selrefs" | "__objc_classrefs" => {
                if isec.size != 8 || rels.len() != 1 || !plain_ptr(&rels[0]) {
                    continue;
                }
                if h.sectname() == "__objc_classrefs" {
                    let RelocTarget::Sym(idx) = rels[0].target() else { continue };
                    if rels[0].addend != 0 {
                        continue;
                    }
                    Key::Class(ctx.objs[obj].symbols[idx as usize])
                } else {
                    Key::Sel(place(ctx, obj, &rels[0]))
                }
            }
            "__cfstring" => {
                if isec.size != 32 || !rels.iter().all(plain_ptr) {
                    continue;
                }
                let mut targets: Vec<(u32, Target)> =
                    rels.iter().map(|rel| (rel.offset, place(ctx, obj, rel))).collect();
                targets.sort_by_key(|t| t.0);
                // The relocated fields hold per-object addends (x86-64
                // embeds the target's address); the targets stand for
                // them.
                let mut bytes = isec.data().to_vec();
                for rel in rels {
                    let (a, b) = (rel.offset as usize, rel.offset as usize + rel.size as usize);
                    bytes[a..b].fill(0);
                }
                Key::CfString(bytes, targets)
            }
            _ => continue,
        };
        match first.entry(key) {
            hashbrown::hash_map::Entry::Occupied(e) => folds.push((i, *e.get())),
            hashbrown::hash_map::Entry::Vacant(e) => {
                e.insert(i as u32);
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

/// Synthesizes _objc_msgSend$<selector> stubs. With selector stubs
/// (the default since Xcode 14), the compiler calls these
/// linker-provided symbols instead of setting up the selector argument
/// itself; each stub loads the interned selector and tail-calls
/// _objc_msgSend. ld-prime lays the stubs out sorted by selector,
/// bytewise, and their selector references in the same order.
pub fn create_objc_msgsend_stubs<E: Target>(ctx: &mut Context<E>) {
    let internal = ctx.internal_obj.expect("internal object not created yet") as u32;
    let mut stubs: Vec<(u32, String)> = Vec::new();
    for i in 0..ctx.symbols.syms.len() {
        let sym = &ctx.symbols[i];
        if sym.is_defined() || !sym.is_used() {
            continue;
        }
        if let Some(sel) = sym.name().strip_prefix("_objc_msgSend$") {
            stubs.push((i as u32, sel.to_string()));
        }
    }
    stubs.sort_by(|a, b| a.1.cmp(&b.1));
    for (idx, &(i, _)) in stubs.iter().enumerate() {
        ctx.symbols[i].set_file(FileId::Obj(internal));
        ctx.sym_aux_mut(i).objc_stub_idx = idx as u32;
    }
    ctx.objc_stubs.symbols = stubs;

    if !ctx.objc_stubs.symbols.is_empty() {
        let id = ctx.symbols.intern("_objc_msgSend");
        ctx.symbols[id].set_is_used(true);
        ctx.objc_stubs.msgsend_sym = Some(id);

        // The stub machinery itself references _objc_msgSend; resolve
        // it now, since regular resolution has already run.
        if !ctx.symbols[id].is_defined()
            && let Some(dylib) = ctx.dylibs.iter().position(|d| d.exports.contains("_objc_msgSend"))
        {
            let sym = &mut ctx.symbols[id];
            sym.set_file(FileId::Dylib((dylib) as u32));
            sym.set_is_imported(true);
            sym.set_is_extern(true);
        }
    }
}

/// The synthesized objc stubs call _objc_msgSend through a GOT slot of
/// their own: ld-prime binds _objc_msgSend twice when a stub or a GOT
/// load elsewhere needs a slot for it as well.
pub fn scan_objc_stubs<E: Target>(ctx: &mut Context<E>) {
    if let Some(id) = ctx.objc_stubs.msgsend_sym {
        ctx.objc_stubs.msgsend_got_idx = ctx.got.got_syms.len() as u32;
        ctx.got.got_syms.push(id);
    }

    // Stub i loads slot i of the __objc_selrefs tail, which points at
    // its selector's name. ld-prime coalesces both with an input's of
    // the same selector and keeps its own: the slot replaces every
    // input selector reference to that selector (one slot per selector,
    // as the runtime uniques one __objc_selrefs), and the name is an
    // input's __objc_methname string where one spells it, only names
    // no input has going in the __objc_methname tail.
    let mut name_of: hashbrown::HashMap<&'static [u8], u32> = hashbrown::HashMap::new();
    let mut absorbed: Vec<(u32, u32)> = Vec::new();
    {
        let stub_of: hashbrown::HashMap<&[u8], u32> = (ctx.objc_stubs.symbols.iter().enumerate())
            .map(|(i, (_, sel))| (sel.as_bytes(), i as u32))
            .collect();
        for i in 0..ctx.isecs.len() {
            let isec = &ctx.isecs[i];
            if stub_of.is_empty()
                || !isec.is_alive()
                || ctx.is_internal(isec.file as usize)
                || isec.replacement != crate::input_sections::NO_REPLACEMENT
            {
                continue;
            }
            let h = ctx.hdr_of(isec);
            if h.sectname() == "__objc_methname" && h.section_type() == S_CSTRING_LITERALS {
                name_of.entry(cstring_of(isec.data())).or_insert(i as u32);
            } else if h.sectname() == "__objc_selrefs"
                && h.section_type() == S_LITERAL_POINTERS
                && isec.size == 8
                && let Some(target) = objc_pointer_at(ctx, i as u32, 0)
                && let Some((name, 0)) = objc_ref_location(ctx, target)
                && let Some(&stub) = stub_of.get(cstring_of(ctx.isecs[name as usize].data()))
            {
                absorbed.push((i as u32, stub));
            }
        }
    }
    absorb_selrefs(ctx, absorbed);

    let stubs = &mut ctx.objc_stubs;
    for i in 0..stubs.symbols.len() {
        let sel = stubs.symbols[i].1.as_bytes();
        let name = name_of.get(sel).copied();
        stubs.name_isec.push(name.unwrap_or(u32::MAX));
        stubs.methname_offs.push(stubs.methname_data.len() as u64);
        if name.is_none() {
            stubs.methname_data.extend_from_slice(sel);
            stubs.methname_data.push(0);
        }
    }
}

/// Replaces input selector references by the objc stub slots that take
/// them over: each by a synthetic subsection standing for its stub's
/// slot, placed once the __objc_selrefs tail is. `absorbed` pairs an
/// input selector reference with its stub.
fn absorb_selrefs<E: Target>(ctx: &mut Context<E>, absorbed: Vec<(u32, u32)>) {
    if absorbed.is_empty() {
        return;
    }
    let (file, shndx) = ctx.add_synthetic_section(MachSection {
        sectname: str_to_name("__objc_selrefs"),
        segname: str_to_name("__DATA"),
        p2align: 3,
        flags: S_LITERAL_POINTERS,
        ..Default::default()
    });
    let mut synth_of: hashbrown::HashMap<u32, u32> = hashbrown::HashMap::new();
    for (input, stub) in absorbed {
        let synth = match synth_of.get(&stub) {
            Some(&synth) => synth,
            None => {
                ctx.isecs.push(InputSection {
                    file,
                    shndx,
                    p2align: 3,
                    input_addr: 0,
                    size: 8,
                    contents: 0,
                    rel_offset: 0,
                    nrels: 0,
                    output_section: u32::MAX,
                    offset: u32::MAX,
                    flags: std::sync::atomic::AtomicU8::new(0),
                    replacement: crate::input_sections::NO_REPLACEMENT,
                    unwind_offset: 0,
                    nunwind: 0,
                });
                let synth = (ctx.isecs.len() - 1) as u32;
                synth_of.insert(stub, synth);
                ctx.objc_stubs.absorbed.push((synth, stub));
                synth
            }
        };
        ctx.isecs[input as usize].replacement = synth;
    }
}

/// A C string's bytes, up to its terminating NUL.
pub(crate) fn cstring_of(data: &[u8]) -> &[u8] {
    &data[..data.iter().position(|&b| b == 0).unwrap_or(data.len())]
}

/// Folds __objc_classrefs into __got, as ld-prime does from a
/// deployment target of macOS 15 on. A class reference is an 8-byte
/// slot holding a class's address, fixed up by dyld - exactly what a
/// GOT entry for the class symbol is. So the code that loads a class
/// from its slot (adrp/ldr on arm64, a RIP-relative mov on x86-64) is
/// retargeted at the class symbol as a GOT load: an imported class is
/// then read from its GOT entry, shared by every reference in the
/// image, and a class defined in the image relaxes to computing the
/// address directly (adrp/add, lea), needing no slot at all. The image
/// has no __objc_classrefs section and none of the slots' local
/// symbols (_OBJC_CLASSLIST_REFERENCES_$_n); the runtime only ever
/// read the section to remap references to swapped classes, which
/// macOS 15's dyld handles through the GOT. ld-prime turned
/// NetNewsWire's 581 class references into 168 GOT entries.
///
/// A reference that cannot become a GOT load (the slot's address
/// taken, or a pointer to it) keeps the slot: it is replaced by a
/// synthetic subsection standing for the class's GOT entry.
///
/// On arm64 ld-prime relaxes a class's loads only if each adrp of its
/// slot is followed, within the function, by one @PAGEOFF use before
/// the next adrp of it (-O0 code can load twice through one adrp). If
/// any reference of the class's, in any object, pairs up otherwise,
/// no reference is rewritten: the slot moves to the GOT and every
/// load reads it there.
pub fn fold_objc_classrefs<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.relocatable || !objc_refs_are_const(ctx) {
        return;
    }
    let slots: Vec<_> = (0..ctx.objs.len()).map(|i| classref_slots(ctx, i)).collect();
    let uses: Vec<_> = (0..ctx.objs.len()).map(|i| classref_uses(ctx, i, &slots[i])).collect();
    let mut unpaired = hashbrown::HashSet::new();
    let pairs: Vec<_> =
        (0..ctx.objs.len()).map(|i| pair_classref_uses(ctx, i, &uses[i], &mut unpaired)).collect();
    let mut got_hdr: Option<(u32, u32)> = None;
    for obj_idx in 0..ctx.objs.len() {
        let slots = &slots[obj_idx];
        if slots.is_empty() {
            continue;
        }

        // Retarget the loads; note the slots something else refers to.
        // A pair's halves go together, as its offset half decides.
        let mut keep: hashbrown::HashSet<u32> = hashbrown::HashSet::new();
        let partner: hashbrown::HashMap<usize, usize> =
            pairs[obj_idx].iter().flat_map(|&(page, off)| [(page, off), (off, off)]).collect();
        for u in &uses[obj_idx] {
            let rel = ctx.objs[obj_idx].relocs[u.k];
            let decider = ctx.objs[obj_idx].relocs[partner.get(&u.k).copied().unwrap_or(u.k)];
            let data = ctx.isecs[u.isec as usize].data();
            match (E::got_load_form(rel.r_type), E::got_load_form(decider.r_type)) {
                (Some(form), Some(dform))
                    if !unpaired.contains(&u.class)
                        && E::can_relax_got_load(data, decider.offset, dform) =>
                {
                    let r = &mut ctx.objs[obj_idx].relocs[u.k];
                    r.r_type = form;
                    r.set_target(RelocTarget::Sym(slots[&u.slot].0));
                }
                _ => {
                    keep.insert(u.slot);
                }
            }
        }

        // In input order: the GOT slots the classes get (and with
        // them the slot addresses every load encodes) follow the
        // object's class-reference order, not the hash map's, which
        // hashbrown seeds afresh for every process.
        let mut ordered: Vec<(u32, crate::symbol::SymbolId)> =
            slots.iter().map(|(&slot, &(_, class))| (slot, class)).collect();
        ordered.sort_unstable_by_key(|&(slot, _)| slot);
        for (slot, class) in ordered {
            if ctx.symbols[class].is_imported() || keep.contains(&slot) {
                add_got(ctx, class);
            }
            if !keep.contains(&slot) {
                ctx.isecs[slot as usize].set_alive(false);
                continue;
            }
            // A synthetic subsection standing for the GOT entry; not
            // alive, since the __got chunk writes the slot and the
            // slot's local symbol is not emitted.
            let (file, shndx) = *got_hdr.get_or_insert_with(|| {
                let (file, shndx) = ctx.add_synthetic_section(MachSection {
                    sectname: str_to_name("__got"),
                    segname: str_to_name(data_seg(ctx)),
                    p2align: 3,
                    flags: S_NON_LAZY_SYMBOL_POINTERS,
                    ..Default::default()
                });
                (file, shndx)
            });
            ctx.isecs.push(InputSection {
                file,
                shndx,
                p2align: 3,
                input_addr: 0,
                size: 8,
                contents: 0,
                rel_offset: 0,
                nrels: 0,
                output_section: u32::MAX,
                offset: u32::MAX,
                flags: std::sync::atomic::AtomicU8::new(0),
                replacement: crate::input_sections::NO_REPLACEMENT,
                unwind_offset: 0,
                nunwind: 0,
            });
            let synth = (ctx.isecs.len() - 1) as u32;
            ctx.isecs[slot as usize].replacement = synth;
            ctx.got.objc_classref_slots.push((synth, class));
        }
    }
}

/// An object's class-reference slots: slot subsection -> the class
/// symbol (its index in the object, and globally).
fn classref_slots<E: Target>(
    ctx: &Context<E>,
    obj_idx: usize,
) -> hashbrown::HashMap<u32, (u32, crate::symbol::SymbolId)> {
    let mut slots = hashbrown::HashMap::new();
    if !ctx.objs[obj_idx].is_alive {
        return slots;
    }
    for &i in &ctx.objs[obj_idx].subsecs {
        let isec = &ctx.isecs[i];
        if !isec.is_alive()
            || isec.replacement != crate::input_sections::NO_REPLACEMENT
            || isec.size != 8
        {
            continue;
        }
        let h = ctx.hdr_of(isec);
        if h.segname() != "__DATA" || h.sectname() != "__objc_classrefs" {
            continue;
        }
        let rels = ctx.isec_relocs(i as usize);
        if rels.len() != 1 {
            continue;
        }
        let rel = rels[0];
        let RelocTarget::Sym(idx) = rel.target() else { continue };
        if E::classify_reloc(rel.r_type) != RelocClass::Plain
            || rel.size != 8
            || rel.is_pcrel
            || rel.is_subtracted
            || rel.addend != 0
        {
            continue;
        }
        slots.insert(i, (idx, ctx.objs[obj_idx].symbols[idx as usize]));
    }
    slots
}

/// A reference to a class-reference slot: relocation `k` of the
/// object, in subsection `isec`.
struct ClassrefUse {
    isec: u32,
    k: usize,
    slot: u32,
    class: crate::symbol::SymbolId,
}

/// The references an object's code and data make to its class-reference
/// slots, in subsection and then address order.
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
        for k in isec.rel_offset as usize..(isec.rel_offset + isec.nrels) as usize {
            let rel = obj.relocs[k];
            let slot = match rel.target() {
                RelocTarget::Section(t) if rel.addend == 0 => t,
                RelocTarget::Sym(idx) => {
                    let sym = &ctx.symbols[obj.symbols[idx as usize]];
                    match sym.input_section() {
                        Some(t) if sym.value == 0 && rel.addend == 0 => t,
                        _ => continue,
                    }
                }
                _ => continue,
            };
            if let Some(&(_, class)) = slots.get(&slot) {
                uses.push(ClassrefUse { isec: i, k, slot, class });
            }
        }
    }
    uses
}

/// Pairs an object's page-and-offset references to class slots (arm64's
/// adrp then ldr or add), each page half with the next offset half of
/// the same class in its subsection, and returns the pairs' relocation
/// indices. A class with a half left over joins `unpaired`.
fn pair_classref_uses<E: Target>(
    ctx: &Context<E>,
    obj_idx: usize,
    uses: &[ClassrefUse],
    unpaired: &mut hashbrown::HashSet<crate::symbol::SymbolId>,
) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    let mut open: hashbrown::HashMap<crate::symbol::SymbolId, usize> = hashbrown::HashMap::new();
    for (n, u) in uses.iter().enumerate() {
        match E::page_pair_half(ctx.objs[obj_idx].relocs[u.k].r_type) {
            Some(true) => {
                if open.insert(u.class, u.k).is_some() {
                    unpaired.insert(u.class);
                }
            }
            Some(false) => match open.remove(&u.class) {
                Some(page) => pairs.push((page, u.k)),
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
    pairs
}

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

fn objc_relative_method_lists<E: Target>(ctx: &Context<E>) -> bool {
    // ld-prime converts method lists in every arm64 image, and on
    // x86-64 in dylibs and bundles only: an x86-64 executable keeps
    // the compiler's absolute lists at any deployment target.
    ctx.args.objc_relative_method_lists.unwrap_or_else(|| {
        (E::CPUTYPE == crate::macho::CPU_TYPE_ARM64 || ctx.args.output_type != MH_EXECUTE)
            && ctx.args.platform == crate::macho::PLATFORM_MACOS
            && ctx.args.platform_minos >= crate::macho::encode_version(11, 0, 0)
    })
}

/// A class's ro data: class_t.data at offset 32, whose low two bits a
/// Swift class uses as flags (FAST_IS_SWIFT_STABLE), so the record
/// itself sits at the pointer with those bits cleared.
fn objc_class_ro<E: Target>(ctx: &Context<E>, cls: (u32, u64)) -> Option<(u32, u64)> {
    let (isec, off) =
        objc_pointer_at(ctx, cls.0, cls.1 + 32).and_then(|r| objc_ref_location(ctx, r))?;
    Some((isec, off & !3))
}

/// The relocation of the pointer field at `off` in a subsection, as
/// (object, index into its relocation arena), for rewriting it.
fn objc_pointer_reloc<E: Target>(ctx: &Context<E>, isec: u32, off: u64) -> Option<(usize, usize)> {
    let sec = &ctx.isecs[isec as usize];
    if ctx.is_internal(sec.file as usize) {
        return None;
    }
    let k = ctx
        .isec_relocs(isec as usize)
        .iter()
        .position(|r| r.offset as u64 == off && r.size == 8 && !r.is_pcrel && !r.is_subtracted)?;
    Some((sec.file as usize, sec.rel_offset as usize + k))
}

/// The pointer stored at `off` in a subsection: the target of the
/// 8-byte relocation there, if any.
fn objc_pointer_at<E: Target>(ctx: &Context<E>, isec: u32, off: u64) -> Option<ObjcRef> {
    let sec = &ctx.isecs[isec];
    if ctx.is_internal(sec.file as usize) {
        return None;
    }
    let rel = ctx
        .isec_relocs(isec as usize)
        .iter()
        .find(|r| r.offset as u64 == off && r.size == 8 && !r.is_pcrel && !r.is_subtracted)?;
    if E::classify_reloc(rel.r_type) != RelocClass::Plain {
        return None;
    }
    Some(match rel.target() {
        RelocTarget::Sym(idx) => {
            ObjcRef::Sym(ctx.objs[sec.file as usize].symbols[idx as usize], rel.addend)
        }
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
    let isec = ctx.resolve_isec(isec as usize) as u32;
    if !ctx.isecs[isec as usize].is_alive() {
        return None;
    }
    Some((isec, off))
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
/// __objc_clsrolist. A selector with no reference in any input gets
/// one synthesized in the __objc_selrefs tail. A list is left alone
/// when it is not the whole of its subsection or not in the classic
/// 24-byte form.
pub fn convert_objc_method_lists<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.relocatable || !objc_relative_method_lists(ctx) {
        return;
    }

    // Selector references the inputs already have, by the selector
    // string subsection they point at.
    let mut selref_of: hashbrown::HashMap<u32, u32> = hashbrown::HashMap::new();
    for i in 0..ctx.isecs.len() {
        let isec = &ctx.isecs[i];
        if !isec.is_alive() || ctx.is_internal(isec.file as usize) || isec.size != 8 {
            continue;
        }
        let h = ctx.hdr_of(isec);
        if h.sectname() != "__objc_selrefs" || h.section_type() != S_LITERAL_POINTERS {
            continue;
        }
        let Some(target) = objc_pointer_at(ctx, i as u32, 0) else { continue };
        if let Some((name, 0)) = objc_ref_location(ctx, target) {
            let slot = ctx.resolve_isec(i) as u32;
            selref_of.entry(name).or_insert(slot);
        }
    }

    // Every method list the runtime would visit.
    let mut lists: Vec<u32> = Vec::new();
    let mut seen: hashbrown::HashSet<u32> = hashbrown::HashSet::new();
    let mut classes_seen: hashbrown::HashSet<(u32, u64)> = hashbrown::HashSet::new();
    let mut note = |ctx: &Context<E>, r: Option<ObjcRef>, lists: &mut Vec<u32>| {
        if let Some((isec, 0)) = r.and_then(|r| objc_ref_location(ctx, r))
            && seen.insert(isec)
        {
            lists.push(isec);
        }
    };
    fn visit_class<E: Target>(
        ctx: &Context<E>,
        cls: (u32, u64),
        classes_seen: &mut hashbrown::HashSet<(u32, u64)>,
        note: &mut impl FnMut(&Context<E>, Option<ObjcRef>, &mut Vec<u32>),
        lists: &mut Vec<u32>,
    ) {
        if !classes_seen.insert(cls) {
            return;
        }
        // class_t: isa, superclass, cache, vtable, data (the ro).
        if let Some(ro) = objc_class_ro(ctx, cls) {
            // class_ro_t: baseMethods at 32.
            note(ctx, objc_pointer_at(ctx, ro.0, ro.1 + 32), lists);
        }
        if let Some(meta) = objc_pointer_at(ctx, cls.0, 0).and_then(|r| objc_ref_location(ctx, r)) {
            visit_class(ctx, meta, classes_seen, note, lists);
        }
    }
    for i in 0..ctx.isecs.len() {
        let isec = &ctx.isecs[i];
        if !isec.is_alive() || ctx.is_internal(isec.file as usize) {
            continue;
        }
        let h = ctx.hdr_of(isec);
        if !h.segname().starts_with("__DATA") {
            continue;
        }
        match h.sectname() {
            "__objc_classlist" | "__objc_nlclslist" => {
                for off in (0..isec.size as u64).step_by(8) {
                    if let Some(cls) =
                        objc_pointer_at(ctx, i as u32, off).and_then(|r| objc_ref_location(ctx, r))
                    {
                        visit_class(ctx, cls, &mut classes_seen, &mut note, &mut lists);
                    }
                }
            }
            "__objc_catlist" | "__objc_nlcatlist" => {
                for off in (0..isec.size as u64).step_by(8) {
                    if let Some(cat) =
                        objc_pointer_at(ctx, i as u32, off).and_then(|r| objc_ref_location(ctx, r))
                    {
                        // category_t: name, cls, instanceMethods, classMethods.
                        note(ctx, objc_pointer_at(ctx, cat.0, cat.1 + 16), &mut lists);
                        note(ctx, objc_pointer_at(ctx, cat.0, cat.1 + 24), &mut lists);
                    }
                }
            }
            "__objc_protolist" => {
                for off in (0..isec.size as u64).step_by(8) {
                    if let Some(proto) =
                        objc_pointer_at(ctx, i as u32, off).and_then(|r| objc_ref_location(ctx, r))
                    {
                        // protocol_t: isa, name, protocols, then the four
                        // method lists.
                        for field in [24, 32, 40, 48] {
                            note(ctx, objc_pointer_at(ctx, proto.0, proto.1 + field), &mut lists);
                        }
                    }
                }
            }
            "__objc_clsrolist" => {
                for off in (0..isec.size as u64).step_by(8) {
                    if let Some(ro) =
                        objc_pointer_at(ctx, i as u32, off).and_then(|r| objc_ref_location(ctx, r))
                    {
                        note(ctx, objc_pointer_at(ctx, ro.0, ro.1 + 32), &mut lists);
                    }
                }
            }
            _ => {}
        }
    }
    if lists.is_empty() {
        return;
    }

    let (file, shndx) = ctx.add_synthetic_section(MachSection {
        sectname: str_to_name("__objc_methlist"),
        segname: str_to_name("__TEXT"),
        p2align: 2,
        flags: S_REGULAR,
        ..Default::default()
    });
    let mut extra_of: hashbrown::HashMap<u32, usize> = hashbrown::HashMap::new();
    let stub_of: hashbrown::HashMap<Vec<u8>, usize> = ctx
        .objc_stubs
        .symbols
        .iter()
        .enumerate()
        .map(|(i, (_, sel))| (sel.as_bytes().to_vec(), i))
        .collect();
    let mut repoint: hashbrown::HashMap<u32, u32> = hashbrown::HashMap::new();
    let mut offset: u64 = 0;
    for list in lists {
        let sec = &ctx.isecs[list as usize];
        let data = sec.data();
        if data.len() < 8 {
            continue;
        }
        let entsize_flags = u32::from_le_bytes(data[0..4].try_into().unwrap());
        let count = u32::from_le_bytes(data[4..8].try_into().unwrap()) as u64;
        if entsize_flags & 0x8000_0000 != 0
            || entsize_flags & 0xffff != 24
            || 8 + 24 * count != data.len() as u64
        {
            continue;
        }
        let mut methods = Vec::with_capacity(count as usize);
        let mut ok = true;
        for i in 0..count {
            let at = 8 + 24 * i;
            let name = objc_pointer_at(ctx, list, at);
            let types = objc_pointer_at(ctx, list, at + 8).unwrap_or(ObjcRef::Null);
            let imp = objc_pointer_at(ctx, list, at + 16).unwrap_or(ObjcRef::Null);
            let Some((sel, 0)) = name.and_then(|r| objc_ref_location(ctx, r)) else {
                ok = false;
                break;
            };
            let name = match selref_of.get(&sel) {
                Some(&slot) => ObjcRef::Isec(slot, 0),
                None => {
                    // A selector stub's slot serves the same selector
                    // (ld64 keeps one slot per selector); else a new
                    // one in the tail.
                    let data = ctx.isecs[sel as usize].data();
                    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
                    match stub_of.get(&data[..end]) {
                        Some(&i) => ObjcRef::TailSelref(i),
                        None => {
                            let n = *extra_of.entry(sel).or_insert_with(|| {
                                ctx.objc_stubs.extra_selrefs.push(sel);
                                ctx.objc_stubs.extra_selrefs.len() - 1
                            });
                            ObjcRef::TailSelref(ctx.objc_stubs.symbols.len() + n)
                        }
                    }
                }
            };
            methods.push(ObjcMethod { name, types, imp });
        }
        if !ok {
            continue;
        }
        let size = 8 + 12 * count;
        offset = align_to(offset, 4);
        ctx.isecs.push(InputSection {
            file,
            shndx,
            p2align: 2,
            input_addr: 0,
            size: size as u32,
            contents: 0,
            rel_offset: 0,
            nrels: 0,
            output_section: u32::MAX,
            offset: offset as u32,
            flags: InputSection::flags_placed(),
            replacement: crate::input_sections::NO_REPLACEMENT,
            unwind_offset: 0,
            nunwind: 0,
        });
        offset += size;
        let synth = (ctx.isecs.len() - 1) as u32;
        ctx.isecs[list as usize].replacement = synth;
        repoint.insert(list, synth);
        ctx.objc_methlist.lists.push(ObjcMethList { isec: synth, methods });
    }
    // The lists' own symbols (__OBJC_$_INSTANCE_METHODS_Foo ...) follow
    // them into __objc_methlist.
    for id in 0..ctx.symbols.syms.len() {
        if let Some(isec) = ctx.symbols[id].input_section()
            && let Some(&synth) = repoint.get(&isec)
        {
            ctx.symbols[id].set_input_section(Some(synth));
        }
    }
}

/// Appends a synthesized data record for an Objective-C rewrite and
/// returns its subsection; see merge_objc_categories.
pub type NewBlob<E> = dyn FnMut(&mut Context<E>, &'static str, Vec<DataField>) -> u32;

/// A field of a synthesized Objective-C data record.
#[derive(Clone, Debug)]
pub enum DataField {
    Bytes(Vec<u8>),
    /// An 8-byte pointer, rebased at load (or null).
    Ptr(ObjcRef),
}

/// A synthesized Objective-C data record, placed in the tail of the
/// output section `sect` (mapped to its segment like an input section
/// of that name) as the synthetic subsection `isec`.
#[derive(Debug)]
pub struct DataBlob {
    pub sect: &'static str,
    pub isec: u32,
    pub fields: Vec<DataField>,
}

impl DataBlob {
    pub fn size(&self) -> u64 {
        self.fields
            .iter()
            .map(|f| match f {
                DataField::Bytes(b) => b.len() as u64,
                DataField::Ptr(_) => 8,
            })
            .sum()
    }
}

/// The address a synthesized record's reference resolves to.
pub fn objc_ref_addr<E: Target>(ctx: &Context<E>, r: ObjcRef) -> u64 {
    match r {
        ObjcRef::Isec(isec, off) => ctx.isec_addr(isec as usize) + off,
        ObjcRef::Sym(id, addend) => (ctx.sym_addr(id) as i64 + addend) as u64,
        ObjcRef::TailSelref(n) => ctx.objc_selref_addr(n),
        ObjcRef::Null => 0,
    }
}

fn objc_cstring_at<E: Target>(ctx: &Context<E>, r: Option<ObjcRef>) -> Option<String> {
    let (isec, off) = objc_ref_location(ctx, r?)?;
    let data = ctx.isecs[isec as usize].data();
    let bytes = data.get(off as usize..)?;
    let end = bytes.iter().position(|&b| b == 0)?;
    Some(String::from_utf8_lossy(&bytes[..end]).into_owned())
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
/// category on a class from another image, or one whose data is not
/// in the expected shape, is left alone. Runs after the method lists
/// have been rewritten in relative form, when it merges those; with
/// classic lists the merged list is a classic one.
pub fn merge_objc_categories<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.relocatable || !ctx.args.objc_category_merging {
        return;
    }
    let relative = objc_relative_method_lists(ctx);

    // Classes defined here: class_t location -> (metaclass, ro, meta ro).
    struct Class {
        meta: (u32, u64),
        ro: (u32, u64),
        meta_ro: (u32, u64),
        nonlazy: bool,
    }
    let mut classes: hashbrown::HashMap<(u32, u64), Class> = hashbrown::HashMap::new();
    let mut class_order: Vec<(u32, u64)> = Vec::new();
    let mut nlclslist_sects = false;
    for i in 0..ctx.isecs.len() {
        let isec = &ctx.isecs[i];
        if !isec.is_alive() || ctx.is_internal(isec.file as usize) {
            continue;
        }
        let h = ctx.hdr_of(isec);
        let nonlazy = match h.sectname() {
            "__objc_classlist" => false,
            "__objc_nlclslist" => {
                nlclslist_sects = true;
                true
            }
            _ => continue,
        };
        for off in (0..isec.size as u64).step_by(8) {
            let Some(cls) =
                objc_pointer_at(ctx, i as u32, off).and_then(|r| objc_ref_location(ctx, r))
            else {
                continue;
            };
            let ro = objc_class_ro(ctx, cls);
            let meta = objc_pointer_at(ctx, cls.0, cls.1).and_then(|r| objc_ref_location(ctx, r));
            let meta_ro = meta.and_then(|m| objc_class_ro(ctx, m));
            let (Some(ro), Some(meta), Some(meta_ro)) = (ro, meta, meta_ro) else { continue };
            if !classes.contains_key(&cls) {
                class_order.push(cls);
            }
            let entry = classes.entry(cls).or_insert(Class { meta, ro, meta_ro, nonlazy: false });
            entry.nonlazy |= nonlazy;
        }
    }
    if classes.is_empty() {
        return;
    }

    // The categories, in __objc_catlist order, by class. A category
    // sits in __objc_catlist and, if it has a +load, in
    // __objc_nlcatlist too; a list subsection may hold several.
    struct Category {
        cat: (u32, u64),
        nonlazy: bool,
        merged: bool,
        name: String,
    }
    struct ListSect {
        isec: u32,
        nonlazy: bool,
        /// Each entry: the category it names, if mergeable.
        entries: Vec<Option<usize>>,
        /// The entries as references, for rebuilding the list.
        refs: Vec<ObjcRef>,
    }
    let mut cats: Vec<Category> = Vec::new();
    let mut cat_index: hashbrown::HashMap<(u32, u64), usize> = hashbrown::HashMap::new();
    let mut cats_of: hashbrown::HashMap<(u32, u64), Vec<usize>> = hashbrown::HashMap::new();
    let mut list_sects: Vec<ListSect> = Vec::new();
    for i in 0..ctx.isecs.len() {
        let isec = &ctx.isecs[i];
        if !isec.is_alive() || ctx.is_internal(isec.file as usize) {
            continue;
        }
        let h = ctx.hdr_of(isec);
        let nonlazy = match h.sectname() {
            "__objc_catlist" => false,
            "__objc_nlcatlist" => true,
            _ => continue,
        };
        let mut ls = ListSect { isec: i as u32, nonlazy, entries: Vec::new(), refs: Vec::new() };
        for off in (0..isec.size as u64).step_by(8) {
            let r = objc_pointer_at(ctx, i as u32, off).unwrap_or(ObjcRef::Null);
            ls.refs.push(r);
            let mergeable = (|| {
                let cat = objc_ref_location(ctx, r)?;
                if cat.1 != 0 || ctx.isecs[cat.0 as usize].size < 48 {
                    return None;
                }
                let cls = objc_pointer_at(ctx, cat.0, 8).and_then(|r| objc_ref_location(ctx, r))?;
                if !classes.contains_key(&cls) {
                    return None;
                }
                let idx = match cat_index.get(&cat) {
                    Some(&idx) => idx,
                    None => {
                        let name = objc_cstring_at(ctx, objc_pointer_at(ctx, cat.0, 0))?;
                        cats.push(Category { cat, nonlazy: false, merged: false, name });
                        cat_index.insert(cat, cats.len() - 1);
                        cats_of.entry(cls).or_default().push(cats.len() - 1);
                        cats.len() - 1
                    }
                };
                cats[idx].nonlazy |= nonlazy;
                Some(idx)
            })();
            ls.entries.push(mergeable);
        }
        list_sects.push(ls);
    }
    if cats_of.is_empty() {
        return;
    }

    // The methods of a list (already rewritten in relative form, or
    // classic), or None if the list is not in a shape we can merge.
    let methods_of = |ctx: &Context<E>, r: Option<ObjcRef>| -> Option<Vec<ObjcMethod>> {
        let Some(r) = r else { return Some(Vec::new()) };
        let (isec, off) = objc_ref_location(ctx, r)?;
        if off != 0 {
            return None;
        }
        let resolved = ctx.resolve_isec(isec as usize) as u32;
        if let Some(list) = ctx.objc_methlist.lists.iter().find(|l| l.isec == resolved) {
            return Some(list.methods.clone());
        }
        if relative {
            return None;
        }
        let data = ctx.isecs[isec as usize].data();
        if data.len() < 8 {
            return None;
        }
        let entsize_flags = u32::from_le_bytes(data[0..4].try_into().unwrap());
        let count = u32::from_le_bytes(data[4..8].try_into().unwrap()) as u64;
        if entsize_flags != 24 || 8 + 24 * count != data.len() as u64 {
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
    };
    // A protocol list: count (8 bytes) then pointers.
    let protocols_of = |ctx: &Context<E>, r: Option<ObjcRef>| -> Option<Vec<ObjcRef>> {
        let Some(r) = r else { return Some(Vec::new()) };
        let (isec, off) = objc_ref_location(ctx, r)?;
        let data = ctx.isecs[isec as usize].data();
        let count =
            u64::from_le_bytes(data.get(off as usize..off as usize + 8)?.try_into().unwrap());
        (0..count).map(|i| objc_pointer_at(ctx, isec, off + 8 + 8 * i)).collect()
    };
    // A property list: entsize (16), count, then (name, attributes).
    let properties_of = |ctx: &Context<E>, r: Option<ObjcRef>| -> Option<Vec<(ObjcRef, ObjcRef)>> {
        let Some(r) = r else { return Some(Vec::new()) };
        let (isec, off) = objc_ref_location(ctx, r)?;
        let data = ctx.isecs[isec as usize].data();
        let entsize =
            u32::from_le_bytes(data.get(off as usize..off as usize + 4)?.try_into().unwrap());
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
    };
    // A pointer field's reference must be to data in this image (or
    // null) for a synthesized record to hold it as a plain rebase.
    let local = |ctx: &Context<E>, r: ObjcRef| -> bool {
        match r {
            ObjcRef::Null | ObjcRef::Isec(..) | ObjcRef::TailSelref(_) => true,
            ObjcRef::Sym(id, _) => {
                !ctx.symbols[id].is_imported() && ctx.symbols[id].input_section().is_some()
            }
        }
    };

    let mut methlist_hdr: Option<(u32, u32)> = None;
    let mut methlist_off: u64 = ctx
        .objc_methlist
        .lists
        .last()
        .map(|l| ctx.isecs[l.isec as usize].offset as u64 + ctx.isecs[l.isec as usize].size as u64)
        .unwrap_or(0);
    let mut nonlazy_classes: Vec<(u32, u64)> = Vec::new();

    for cls in class_order {
        let Some(cat_ids) = cats_of.get(&cls) else { continue };
        let cat_ids = cat_ids.clone();
        let info = &classes[&cls];
        // Gather everything first; give up on the class if anything is
        // not in shape.
        struct Merged {
            imethods: Vec<ObjcMethod>,
            cmethods: Vec<ObjcMethod>,
            protocols: Vec<ObjcRef>,
            iprops: Vec<(ObjcRef, ObjcRef)>,
            cprops: Vec<(ObjcRef, ObjcRef)>,
            any_imethods: bool,
            any_cmethods: bool,
            any_protocols: bool,
            any_iprops: bool,
            any_cprops: bool,
        }
        let mut m = Merged {
            imethods: Vec::new(),
            cmethods: Vec::new(),
            protocols: Vec::new(),
            iprops: Vec::new(),
            cprops: Vec::new(),
            any_imethods: false,
            any_cmethods: false,
            any_protocols: false,
            any_iprops: false,
            any_cprops: false,
        };
        let mut ok = true;
        for &ci in cat_ids.iter().rev() {
            let (c, coff) = cats[ci].cat;
            let im = objc_pointer_at(ctx, c, coff + 16);
            let cm = objc_pointer_at(ctx, c, coff + 24);
            let pr = objc_pointer_at(ctx, c, coff + 32);
            match (methods_of(ctx, im), methods_of(ctx, cm), protocols_of(ctx, pr)) {
                (Some(a), Some(b), Some(p)) => {
                    m.any_imethods |= im.is_some();
                    m.any_cmethods |= cm.is_some();
                    m.any_protocols |= pr.is_some();
                    m.imethods.extend(a);
                    m.cmethods.extend(b);
                    m.protocols.extend(p);
                }
                _ => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            for &ci in cat_ids.iter() {
                let (c, coff) = cats[ci].cat;
                let ip = objc_pointer_at(ctx, c, coff + 40);
                let cp = if ctx.isecs[c as usize].size >= coff as u32 + 56 {
                    objc_pointer_at(ctx, c, coff + 48)
                } else {
                    None
                };
                match (properties_of(ctx, ip), properties_of(ctx, cp)) {
                    (Some(a), Some(b)) => {
                        m.any_iprops |= ip.is_some();
                        m.any_cprops |= cp.is_some();
                        m.iprops.extend(a);
                        m.cprops.extend(b);
                    }
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
        }
        // The class's own lists follow.
        let (ro, meta_ro) = (info.ro, info.meta_ro);
        let base_im = objc_pointer_at(ctx, ro.0, ro.1 + 32);
        let base_cm = objc_pointer_at(ctx, meta_ro.0, meta_ro.1 + 32);
        let base_pr = objc_pointer_at(ctx, ro.0, ro.1 + 40);
        let base_ip = objc_pointer_at(ctx, ro.0, ro.1 + 64);
        let base_cp = objc_pointer_at(ctx, meta_ro.0, meta_ro.1 + 64);
        if ok {
            match (
                methods_of(ctx, base_im),
                methods_of(ctx, base_cm),
                protocols_of(ctx, base_pr),
                properties_of(ctx, base_ip),
                properties_of(ctx, base_cp),
            ) {
                (Some(a), Some(b), Some(p), Some(ip), Some(cp)) => {
                    m.imethods.extend(a);
                    m.cmethods.extend(b);
                    m.protocols.extend(p);
                    m.iprops.extend(ip);
                    m.cprops.extend(cp);
                }
                _ => ok = false,
            }
        }
        if ok && !relative {
            ok = m
                .imethods
                .iter()
                .chain(&m.cmethods)
                .all(|x| local(ctx, x.name) && local(ctx, x.types) && local(ctx, x.imp));
        }
        if ok {
            ok = m.protocols.iter().all(|&r| local(ctx, r))
                && m.iprops.iter().chain(&m.cprops).all(|&(a, b)| local(ctx, a) && local(ctx, b));
        }
        // The class's and metaclass's data pointers must be rewritable
        // to point at new ro records. Nothing is changed until
        // everything checks out.
        let ro_ok = objc_pointer_reloc(ctx, cls.0, cls.1 + 32).is_some() && info.meta.1 == 0
            || objc_pointer_reloc(ctx, info.meta.0, info.meta.1 + 32).is_some();
        let ro_ok = ro_ok
            && ctx.isecs[ro.0 as usize].size as u64 >= ro.1 + 72
            && ctx.isecs[meta_ro.0 as usize].size as u64 >= meta_ro.1 + 72
            && objc_pointer_reloc(ctx, info.meta.0, info.meta.1 + 32).is_some();
        if !ok || !ro_ok {
            if std::env::var_os("MOLD_OBJC_DEBUG").is_some() {
                eprintln!(
                    "category merging: class at {:?} with {} categories skipped (lists in shape: {}, ro reachable: {})",
                    cls,
                    cat_ids.len(),
                    ok,
                    ro_ok
                );
            }
            continue;
        }

        // Emit the merged lists.
        let mut new_blob =
            |ctx: &mut Context<E>, sect: &'static str, fields: Vec<DataField>| -> u32 {
                let (file, shndx) = ctx.add_synthetic_section(MachSection {
                    sectname: str_to_name(sect),
                    segname: str_to_name("__DATA"),
                    p2align: 3,
                    flags: 0,
                    ..Default::default()
                });
                let blob = DataBlob { sect, isec: 0, fields };
                let size = blob.size();
                ctx.isecs.push(InputSection {
                    file,
                    shndx,
                    p2align: 3,
                    input_addr: 0,
                    size: size as u32,
                    contents: 0,
                    rel_offset: 0,
                    nrels: 0,
                    output_section: u32::MAX,
                    offset: 0,
                    flags: InputSection::flags_placed(),
                    replacement: crate::input_sections::NO_REPLACEMENT,
                    unwind_offset: 0,
                    nunwind: 0,
                });
                let isec = (ctx.isecs.len() - 1) as u32;
                ctx.data_blobs.push(DataBlob { isec, ..blob });
                isec
            };
        let mut new_methlist = |ctx: &mut Context<E>, methods: Vec<ObjcMethod>| -> u32 {
            if relative {
                let (file, shndx) = *methlist_hdr.get_or_insert_with(|| {
                    let (file, shndx) = ctx.add_synthetic_section(MachSection {
                        sectname: str_to_name("__objc_methlist"),
                        segname: str_to_name("__TEXT"),
                        p2align: 2,
                        flags: S_REGULAR,
                        ..Default::default()
                    });
                    (file, shndx)
                });
                let size = 8 + 12 * methods.len() as u64;
                methlist_off = align_to(methlist_off, 4);
                ctx.isecs.push(InputSection {
                    file,
                    shndx,
                    p2align: 2,
                    input_addr: 0,
                    size: size as u32,
                    contents: 0,
                    rel_offset: 0,
                    nrels: 0,
                    output_section: u32::MAX,
                    offset: methlist_off as u32,
                    flags: InputSection::flags_placed(),
                    replacement: crate::input_sections::NO_REPLACEMENT,
                    unwind_offset: 0,
                    nunwind: 0,
                });
                methlist_off += size;
                let isec = (ctx.isecs.len() - 1) as u32;
                ctx.objc_methlist.lists.push(ObjcMethList { isec, methods });
                isec
            } else {
                let mut fields = vec![
                    DataField::Bytes(24u32.to_le_bytes().to_vec()),
                    DataField::Bytes((methods.len() as u32).to_le_bytes().to_vec()),
                ];
                for m in &methods {
                    fields.push(DataField::Ptr(m.name));
                    fields.push(DataField::Ptr(m.types));
                    fields.push(DataField::Ptr(m.imp));
                }
                // ld-prime writes a merged absolute list into
                // __objc_data (the protocol and property lists stay in
                // __objc_const).
                new_blob(ctx, "__objc_data", fields)
            }
        };
        // The class's original lists and the categories' are dropped
        // (the merged list carries ld64's name); a superseded list
        // that is still referred to resolves to the merged one.
        let supersede = |ctx: &mut Context<E>, r: Option<ObjcRef>, merged: Option<u32>| {
            if let Some((isec, 0)) = r.and_then(|r| objc_ref_location(ctx, r)) {
                let resolved = ctx.resolve_isec(isec as usize);
                if Some(resolved as u32) != merged {
                    ctx.objc_methlist.lists.retain(|l| l.isec as usize != resolved);
                    ctx.isecs[resolved].set_alive(false);
                    if let Some(merged) = merged {
                        ctx.isecs[resolved].replacement = merged;
                        if resolved != isec as usize {
                            ctx.isecs[isec as usize].replacement = merged;
                        }
                    }
                }
            }
        };

        // ld64 names the merged lists after the class and its
        // categories: __OBJC_$_INSTANCE_METHODS_Foo(A|B).
        let class_name =
            objc_cstring_at(ctx, objc_pointer_at(ctx, ro.0, ro.1 + 24)).unwrap_or_default();
        let suffix = format!(
            "{}({})",
            class_name,
            cat_ids.iter().map(|&ci| cats[ci].name.as_str()).collect::<Vec<_>>().join("|")
        );
        let name_it = |ctx: &mut Context<E>, prefix: &str, isec: u32| {
            let name: &'static str = String::leak(format!("{prefix}{suffix}"));
            ctx.extra_local_syms.push((name, isec));
        };
        let imethods = if m.any_imethods {
            Some(new_methlist(ctx, std::mem::take(&mut m.imethods)))
        } else {
            None
        };
        let cmethods = if m.any_cmethods {
            Some(new_methlist(ctx, std::mem::take(&mut m.cmethods)))
        } else {
            None
        };
        let protocols = if m.any_protocols {
            let mut fields =
                vec![DataField::Bytes((m.protocols.len() as u64).to_le_bytes().to_vec())];
            fields.extend(m.protocols.iter().map(|&r| DataField::Ptr(r)));
            Some(new_blob(ctx, "__objc_const", fields))
        } else {
            None
        };
        if let Some(l) = imethods {
            name_it(ctx, "__OBJC_$_INSTANCE_METHODS_", l);
        }
        if let Some(l) = cmethods {
            name_it(ctx, "__OBJC_$_CLASS_METHODS_", l);
        }
        if let Some(l) = protocols {
            name_it(ctx, "__OBJC_CLASS_PROTOCOLS_$_", l);
        }
        let props = |ctx: &mut Context<E>, list: &[(ObjcRef, ObjcRef)]| -> u32 {
            let mut fields = vec![
                DataField::Bytes(16u32.to_le_bytes().to_vec()),
                DataField::Bytes((list.len() as u32).to_le_bytes().to_vec()),
            ];
            for &(n, a) in list {
                fields.push(DataField::Ptr(n));
                fields.push(DataField::Ptr(a));
            }
            new_blob(ctx, "__objc_const", fields)
        };
        let iprops = if m.any_iprops { Some(props(ctx, &m.iprops)) } else { None };
        let cprops = if m.any_cprops { Some(props(ctx, &m.cprops)) } else { None };

        if imethods.is_some() {
            supersede(ctx, base_im, None);
        }
        if cmethods.is_some() {
            supersede(ctx, base_cm, None);
        }
        // Superseded protocol and property lists go away (ld64's output
        // keeps only the merged ones).
        let retire = |ctx: &mut Context<E>, r: Option<ObjcRef>| {
            if let Some((isec, 0)) = r.and_then(|r| objc_ref_location(ctx, r)) {
                ctx.isecs[isec as usize].set_alive(false);
            }
        };
        if protocols.is_some() {
            retire(ctx, base_pr);
        }
        if iprops.is_some() {
            retire(ctx, base_ip);
        }
        if cprops.is_some() {
            retire(ctx, base_cp);
        }
        for &ci in cat_ids.iter() {
            let (c, coff) = cats[ci].cat;
            supersede(ctx, objc_pointer_at(ctx, c, coff + 16), None);
            supersede(ctx, objc_pointer_at(ctx, c, coff + 24), None);
            for field in [32, 40, 48] {
                if ctx.isecs[c as usize].size as u64 >= coff + field + 8 {
                    retire(ctx, objc_pointer_at(ctx, c, coff + field));
                }
            }
        }

        // New ro records with the merged lists, replacing the class's
        // (their symbols follow). class_ro_t: flags, instanceStart,
        // instanceSize, reserved, then ivarLayout, name, baseMethods,
        // baseProtocols, ivars, weakIvarLayout, baseProperties - and,
        // when the flags carry RO_HAS_SWIFT_INITIALIZER (1 << 6), a
        // Swift class's metadata initializer pointer at 72, which the
        // runtime calls while realizing the class (dropping it from
        // the rewritten record sent NetNewsWire's AppDelegate into a
        // garbage address in objc_copyClassList).
        let rewrite_ro = |ctx: &mut Context<E>,
                          ro: (u32, u64),
                          methods: Option<u32>,
                          protocols: Option<u32>,
                          props: Option<u32>,
                          new_blob: &mut NewBlob<E>| {
            let data = ctx.isecs[ro.0 as usize].data()[ro.1 as usize..ro.1 as usize + 16].to_vec();
            let flags = u32::from_le_bytes(data[0..4].try_into().unwrap());
            let has_swift_initializer = flags & (1 << 6) != 0;
            let mut fields = vec![DataField::Bytes(data)];
            let mut ptr_fields: Vec<u64> = vec![16, 24, 32, 40, 48, 56, 64];
            if has_swift_initializer {
                ptr_fields.push(72);
            }
            let record_len = ptr_fields.last().unwrap() + 8;
            for (k, field) in ptr_fields.into_iter().enumerate() {
                let sub = match k {
                    2 => methods,
                    3 => protocols,
                    6 => props,
                    _ => None,
                };
                let r = match sub {
                    Some(isec) => ObjcRef::Isec(isec, 0),
                    None => objc_pointer_at(ctx, ro.0, ro.1 + field).unwrap_or(ObjcRef::Null),
                };
                fields.push(DataField::Ptr(r));
            }
            // The new record goes where the old one was: Swift puts a
            // class's ro data in __objc_data (ld64's output keeps
            // __DATA__TtC... there), clang's in __objc_const.
            let sect: &'static str = match ctx.hdr_of(&ctx.isecs[ro.0 as usize]).sectname() {
                "__objc_data" => "__objc_data",
                _ => "__objc_const",
            };
            let blob = new_blob(ctx, sect, fields);
            let isec = ro.0 as usize;
            if ro.1 == 0 && ctx.isecs[isec].size as u64 == record_len {
                // The record was a subsection of its own: replace it, so
                // its symbol names the new record too (ld64 keeps
                // __OBJC_CLASS_RO_$_Foo).
                ctx.isecs[isec].set_alive(false);
                ctx.isecs[isec].replacement = blob;
            }
            blob
        };
        // Point a class's data field at its new ro record (keeping the
        // flag bits a Swift class stores in the pointer's low bits).
        let retarget = |ctx: &mut Context<E>, cls: (u32, u64), blob: u32| {
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
            rel.set_target(RelocTarget::Section(blob));
            rel.addend = flags;
        };
        let ro_blob = rewrite_ro(ctx, ro, imethods, protocols, iprops, &mut new_blob);
        let meta_blob = rewrite_ro(ctx, meta_ro, cmethods, protocols, cprops, &mut new_blob);
        retarget(ctx, cls, ro_blob);
        retarget(ctx, info.meta, meta_blob);
        let mut any_nonlazy = false;
        for &ci in cat_ids.iter() {
            ctx.isecs[cats[ci].cat.0 as usize].set_alive(false);
            cats[ci].merged = true;
            any_nonlazy |= cats[ci].nonlazy;
        }
        if any_nonlazy && !info.nonlazy {
            nonlazy_classes.push(cls);
        }
    }

    // The category lists lose the merged entries: a subsection all of
    // whose entries merged goes away, one with survivors is rebuilt.
    for ls in &list_sects {
        let merged: Vec<bool> =
            ls.entries.iter().map(|e| e.is_some_and(|ci| cats[ci].merged)).collect();
        if !merged.iter().any(|&m| m) {
            continue;
        }
        ctx.isecs[ls.isec as usize].set_alive(false);
        let survivors: Vec<DataField> = ls
            .refs
            .iter()
            .zip(&merged)
            .filter(|&(_, &m)| !m)
            .map(|(&r, _)| DataField::Ptr(r))
            .collect();
        if survivors.is_empty() {
            continue;
        }
        let sect: &'static str = if ls.nonlazy { "__objc_nlcatlist" } else { "__objc_catlist" };
        let (file, shndx) = ctx.add_synthetic_section(MachSection {
            sectname: str_to_name(sect),
            segname: str_to_name("__DATA"),
            p2align: 3,
            flags: S_ATTR_NO_DEAD_STRIP,
            ..Default::default()
        });
        ctx.isecs.push(InputSection {
            file,
            shndx,
            p2align: 3,
            input_addr: 0,
            size: (survivors.len() * 8) as u32,
            contents: 0,
            rel_offset: 0,
            nrels: 0,
            output_section: u32::MAX,
            offset: 0,
            flags: InputSection::flags_placed(),
            replacement: crate::input_sections::NO_REPLACEMENT,
            unwind_offset: 0,
            nunwind: 0,
        });
        let isec = (ctx.isecs.len() - 1) as u32;
        ctx.data_blobs.push(DataBlob { sect, isec, fields: survivors });
    }

    // Classes that absorbed a +load category become non-lazy.
    if !nonlazy_classes.is_empty() {
        let _ = nlclslist_sects;
        for cls in nonlazy_classes {
            let (file, shndx) = ctx.add_synthetic_section(MachSection {
                sectname: str_to_name("__objc_nlclslist"),
                segname: str_to_name("__DATA"),
                p2align: 3,
                flags: S_ATTR_NO_DEAD_STRIP,
                ..Default::default()
            });
            ctx.isecs.push(InputSection {
                file,
                shndx,
                p2align: 3,
                input_addr: 0,
                size: 8,
                contents: 0,
                rel_offset: 0,
                nrels: 0,
                output_section: u32::MAX,
                offset: 0,
                flags: InputSection::flags_placed(),
                replacement: crate::input_sections::NO_REPLACEMENT,
                unwind_offset: 0,
                nunwind: 0,
            });
            let isec = (ctx.isecs.len() - 1) as u32;
            ctx.data_blobs.push(DataBlob {
                sect: "__objc_nlclslist",
                isec,
                fields: vec![DataField::Ptr(ObjcRef::Isec(cls.0, cls.1))],
            });
        }
    }
}
