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
use crate::passes::{
    absorb_got_slots, add_got, objc_refs_are_const, pointer_target,
    redirect_symbols_to_replacements,
};
use crate::target::RelocClass;
use crate::target::Target;
use crate::util::align_to;

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

/// The address a reference in a synthesized record or a rewritten
/// method list resolves to, once the output is laid out.
pub fn objc_ref_addr<E: Target>(ctx: &Context<E>, r: ObjcRef) -> u64 {
    match r {
        ObjcRef::Isec(isec, off) => ctx.isec_addr(isec as usize) + off,
        ObjcRef::Sym(id, addend) => (ctx.sym_addr(id) as i64 + addend) as u64,
        ObjcRef::TailSelref(n) => ctx.objc_selref_addr(n),
        ObjcRef::Null => 0,
    }
}

/// Appends a live synthetic subsection of `sect`, a section of the
/// internal object as add_synthetic_section returns it, and returns
/// it. Its output section and offset are set by hand (IS_PLACED), not
/// by create_output_sections; `offset` is its offset if already known.
fn add_placed_isec<E: Target>(
    ctx: &mut Context<E>,
    sect: (u32, u32),
    p2align: u8,
    size: u64,
    offset: u64,
) -> u32 {
    let (file, shndx) = sect;
    ctx.isecs.push(InputSection {
        file,
        shndx,
        p2align,
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
    (ctx.isecs.len() - 1) as u32
}

/// Appends a synthetic subsection of `sect` standing for an 8-byte slot
/// a chunk writes (a GOT entry, an objc stub's selector reference), for
/// input subsections to be replaced by, and returns it. It is not
/// alive: it gets the slot's output section and offset once the chunk
/// is laid out.
pub(crate) fn add_slot_stand_in<E: Target>(ctx: &mut Context<E>, sect: (u32, u32)) -> u32 {
    let (file, shndx) = sect;
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
        flags: InputSection::flags_dead(),
        replacement: crate::input_sections::NO_REPLACEMENT,
        unwind_offset: 0,
        nunwind: 0,
    });
    (ctx.isecs.len() - 1) as u32
}

/// Appends a synthesized record to the tail of __DATA,`sect` (a section
/// with the given flags) and returns its subsection.
fn add_data_blob<E: Target>(
    ctx: &mut Context<E>,
    sect: &'static str,
    flags: u32,
    fields: Vec<DataField>,
) -> u32 {
    let hdr = ctx.add_synthetic_section(MachSection {
        sectname: str_to_name(sect),
        segname: str_to_name("__DATA"),
        p2align: 3,
        flags,
        ..Default::default()
    });
    let blob = DataBlob { sect, isec: 0, fields };
    let isec = add_placed_isec(ctx, hdr, 3, blob.size(), 0);
    ctx.data_blobs.push(DataBlob { isec, ..blob });
    isec
}

/// A new __TEXT,__objc_methlist section of the internal object, for
/// method lists rewritten in the relative form.
fn add_methlist_section<E: Target>(ctx: &mut Context<E>) -> (u32, u32) {
    ctx.add_synthetic_section(MachSection {
        sectname: str_to_name("__objc_methlist"),
        segname: str_to_name("__TEXT"),
        p2align: 2,
        flags: S_REGULAR,
        ..Default::default()
    })
}

/// Appends a method list in the relative form, at `*offset` in `sect`
/// (an __objc_methlist section of the internal object), and returns
/// its subsection.
fn add_relative_method_list<E: Target>(
    ctx: &mut Context<E>,
    sect: (u32, u32),
    offset: &mut u64,
    methods: Vec<ObjcMethod>,
) -> u32 {
    let size = 8 + 12 * methods.len() as u64;
    *offset = align_to(*offset, 4);
    let isec = add_placed_isec(ctx, sect, 2, size, *offset);
    *offset += size;
    ctx.objc_methlist.lists.push(ObjcMethList { isec, methods });
    isec
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

/// The pointers in the 8-byte slots of a list section such as
/// __objc_classlist (None where a slot has none).
fn list_entries<E: Target>(ctx: &Context<E>, isec: u32) -> impl Iterator<Item = Option<ObjcRef>> {
    let size = ctx.isecs[isec as usize].size as u64;
    (0..size).step_by(8).map(move |off| objc_pointer_at(ctx, isec, off))
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

/// A class's ro data: class_t.data at offset 32, whose low two bits a
/// Swift class uses as flags (FAST_IS_SWIFT_STABLE), so the record
/// itself sits at the pointer with those bits cleared.
fn objc_class_ro<E: Target>(ctx: &Context<E>, cls: (u32, u64)) -> Option<(u32, u64)> {
    let (isec, off) =
        objc_pointer_at(ctx, cls.0, cls.1 + 32).and_then(|r| objc_ref_location(ctx, r))?;
    Some((isec, off & !3))
}

/// The C string a reference points at, if it is in the image.
fn objc_cstring_at<E: Target>(ctx: &Context<E>, r: Option<ObjcRef>) -> Option<String> {
    let (isec, off) = objc_ref_location(ctx, r?)?;
    let data = ctx.isecs[isec as usize].data();
    let bytes = data.get(off as usize..)?;
    let end = bytes.iter().position(|&b| b == 0)?;
    Some(String::from_utf8_lossy(&bytes[..end]).into_owned())
}

/// A C string's bytes, up to its terminating NUL.
pub(crate) fn cstring_of(data: &[u8]) -> &[u8] {
    &data[..data.iter().position(|&b| b == 0).unwrap_or(data.len())]
}

/// Coalesces the Objective-C reference records the compiler emits
/// once per object: __objc_selrefs entries naming the same selector,
/// __objc_classrefs entries naming the same class, and identical
/// __cfstring constants. ld64 keeps one of each, in a -r output as in
/// a final link (NetNewsWire's RSCore prelink had 56 class references
/// where ld-prime's has 30, its debug dylib 592 selector references
/// too many); the first copy wins and the rest redirect to it, like
/// merged literals. A final link from macOS 15 on leaves class
/// references to fold_objc_classrefs, which turns them into GOT slots
/// (and coalesces those nothing refers to). __objc_superrefs and
/// __objc_protorefs entries of one class or protocol coalesce too, but
/// for those a symbol names (see mark_labeled_literals): the compiler
/// labels each, but an x86-64 -r output drops the labels.
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
        Super(Target),
        Proto(Target),
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
            "__objc_superrefs" | "__objc_protorefs" => {
                if isec.is_labeled() || isec.size != 8 || rels.len() != 1 || !plain_ptr(&rels[0]) {
                    continue;
                }
                let target = place(ctx, obj, &rels[0]);
                if h.sectname() == "__objc_superrefs" {
                    Key::Super(target)
                } else {
                    Key::Proto(target)
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
/// slot, placed once the __objc_selrefs tail is, and as aligned as the
/// most aligned of them (as ld-prime keeps a 2^4 input's alignment for
/// the slot). `absorbed` pairs an input selector reference with its
/// stub.
fn absorb_selrefs<E: Target>(ctx: &mut Context<E>, absorbed: Vec<(u32, u32)>) {
    if absorbed.is_empty() {
        return;
    }
    let sect = ctx.add_synthetic_section(MachSection {
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
                let synth = add_slot_stand_in(ctx, sect);
                synth_of.insert(stub, synth);
                ctx.objc_stubs.absorbed.push((synth, stub));
                synth
            }
        };
        let p2align = ctx.isecs[input as usize].p2align;
        ctx.isecs[input as usize].replacement = synth;
        let s = &mut ctx.isecs[synth as usize];
        s.p2align = s.p2align.max(p2align);
    }
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
/// What folds is what is referenced, whatever the slot points at (a
/// class or not): ld-prime rewrites the references to the slots it
/// has coalesced, one per class. The slot of a class nothing refers
/// to through one stays in __objc_classrefs, and its class gets no
/// GOT entry.
///
/// On arm64 ld-prime relaxes a class's loads only if each adrp of its
/// slot is followed, within the function, by one @PAGEOFF use before
/// the next adrp of it (-O0 code can load twice through one adrp). If
/// any reference of the class's, in any object, pairs up otherwise,
/// no reference is rewritten: the slot moves to the GOT and every
/// load reads it there.
///
/// A slot may point at its class section-relatively, as an x86-64
/// object refers to a class only a temporary (L) label names; ld-prime
/// folds it all the same (see name_classref_targets).
pub fn fold_objc_classrefs<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.relocatable || !objc_refs_are_const(ctx) {
        return;
    }
    name_classref_targets(ctx);
    let slots: Vec<_> = (0..ctx.objs.len()).map(|i| classref_slots(ctx, i)).collect();
    let uses: Vec<_> = (0..ctx.objs.len()).map(|i| classref_uses(ctx, i, &slots[i])).collect();
    let mut unpaired = hashbrown::HashSet::new();
    let pairs: Vec<_> =
        (0..ctx.objs.len()).map(|i| pair_classref_uses(ctx, i, &uses[i], &mut unpaired)).collect();
    let referenced: hashbrown::HashSet<_> = uses.iter().flatten().map(|u| u.class).collect();
    let mut unreferenced: hashbrown::HashMap<crate::symbol::SymbolId, u32> =
        hashbrown::HashMap::new();
    let mut coalesced = false;
    let mut absorbed = Vec::new();
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
            if !referenced.contains(&class) {
                // Coalesced, as below macOS 15: the first slot stays.
                let first = *unreferenced.entry(class).or_insert(slot);
                if first != slot {
                    ctx.isecs[slot as usize].replacement = first;
                    coalesced = true;
                }
                continue;
            }
            // (A lazy dylib's class is loaded through a lazy-load
            // helper instead: see passes::create_lazy_loads.)
            let imported = ctx.symbols[class].is_imported() && !ctx.is_lazy_import(class);
            if imported || keep.contains(&slot) {
                add_got(ctx, class);
            }
            if !keep.contains(&slot) {
                ctx.isecs[slot as usize].set_alive(false);
                continue;
            }
            absorbed.push((slot, class));
        }
    }
    absorb_got_slots(ctx, absorbed);
    if coalesced {
        redirect_symbols_to_replacements(ctx);
    }
}

/// Names by a symbol of its object, anonymous and in no symbol table,
/// the class each class-reference slot points at section-relatively,
/// for fold_objc_classrefs to fold the slot as it does one naming a
/// symbol; the slots of an object pointing at one class share it. A
/// slot that points into the middle of a subsection is left alone and
/// keeps its slot: ld-prime folds it to the start of its atom, losing
/// the offset.
fn name_classref_targets<E: Target>(ctx: &mut Context<E>) {
    for obj_idx in 0..ctx.objs.len() {
        let obj = &ctx.objs[obj_idx];
        if !obj.is_alive
            || !obj
                .sect_hdrs
                .iter()
                .any(|h| h.segname() == "__DATA" && h.sectname() == "__objc_classrefs")
        {
            continue;
        }
        let mut named: hashbrown::HashMap<u32, u32> = hashbrown::HashMap::new();
        for k in 0..ctx.objs[obj_idx].subsecs.len() {
            let i = ctx.objs[obj_idx].subsecs[k] as usize;
            let isec = &ctx.isecs[i];
            let h = ctx.hdr_of(isec);
            if !isec.is_alive()
                || isec.replacement != crate::input_sections::NO_REPLACEMENT
                || h.segname() != "__DATA"
                || h.sectname() != "__objc_classrefs"
                || isec.size != 8
            {
                continue;
            }
            let rel_idx = isec.rel_offset as usize;
            let [rel] = ctx.isec_relocs(i) else { continue };
            let RelocTarget::Section(class) = rel.target() else { continue };
            if E::classify_reloc(rel.r_type) != RelocClass::Plain
                || rel.addend != 0
                || rel.size != 8
                || rel.is_pcrel
                || rel.is_subtracted
            {
                continue;
            }
            let idx = *named.entry(class).or_insert_with(|| {
                let mut sym = crate::symbol::Symbol::new("");
                sym.set_file(FileId::Obj(obj_idx as u32));
                sym.set_input_section(Some(class));
                ctx.symbols.syms.push(sym);
                let obj = &mut ctx.objs[obj_idx];
                obj.symbols.push((ctx.symbols.syms.len() - 1) as u32);
                (obj.symbols.len() - 1) as u32
            });
            ctx.objs[obj_idx].relocs[rel_idx].set_target(RelocTarget::Sym(idx));
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
        if !isec.is_alive() || isec.replacement != crate::input_sections::NO_REPLACEMENT {
            continue;
        }
        let h = ctx.hdr_of(isec);
        if h.segname() != "__DATA" || h.sectname() != "__objc_classrefs" {
            continue;
        }
        if let Some(idx) = pointer_target(ctx, i as usize) {
            slots.insert(i, (idx, ctx.objs[obj_idx].symbols[idx as usize]));
        }
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

pub(crate) fn objc_relative_method_lists<E: Target>(ctx: &Context<E>) -> bool {
    // ld-prime converts method lists in every arm64 image, and on
    // x86-64 in dylibs and bundles only: an x86-64 executable keeps
    // the compiler's absolute lists at any deployment target.
    ctx.args.objc_relative_method_lists.unwrap_or_else(|| {
        (E::CPUTYPE == crate::macho::CPU_TYPE_ARM64 || ctx.args.output_type != MH_EXECUTE)
            && ctx.args.platform == crate::macho::PLATFORM_MACOS
            && ctx.args.platform_minos >= crate::macho::encode_version(11, 0, 0)
    })
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
    if ctx.args.relocatable || !objc_relative_method_lists(ctx) {
        return;
    }
    let lists = runtime_method_lists(ctx);
    if lists.is_empty() {
        return;
    }

    let mut selrefs = SelrefFinder::new(ctx);
    let sect = add_methlist_section(ctx);
    let mut offset: u64 = 0;
    let mut repoint: hashbrown::HashMap<u32, u32> = hashbrown::HashMap::new();
    for list in lists {
        let Some(methods) = relative_methods(ctx, list, &mut selrefs) else { continue };
        let synth = add_relative_method_list(ctx, sect, &mut offset, methods);
        ctx.isecs[list as usize].replacement = synth;
        repoint.insert(list, synth);
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

/// Every method list the runtime would visit, each once, in the order
/// the list sections lead to them.
fn runtime_method_lists<E: Target>(ctx: &Context<E>) -> Vec<u32> {
    let mut found = MethodListFinder::default();
    for i in 0..ctx.isecs.len() {
        let isec = &ctx.isecs[i];
        if !isec.is_alive() || ctx.is_internal(isec.file as usize) {
            continue;
        }
        let h = ctx.hdr_of(isec);
        if !h.segname().starts_with("__DATA") {
            continue;
        }
        let records = || list_entries(ctx, i as u32).filter_map(|r| objc_ref_location(ctx, r?));
        match h.sectname() {
            "__objc_classlist" | "__objc_nlclslist" => {
                for cls in records() {
                    found.visit_class(ctx, cls);
                }
            }
            "__objc_catlist" | "__objc_catlist2" | "__objc_nlcatlist" => {
                // category_t: name, cls, instanceMethods, classMethods.
                for cat in records() {
                    found.note(ctx, cat, 16);
                    found.note(ctx, cat, 24);
                }
            }
            "__objc_protolist" => {
                // protocol_t: isa, name, protocols, then the four method
                // lists.
                for proto in records() {
                    for field in [24, 32, 40, 48] {
                        found.note(ctx, proto, field);
                    }
                }
            }
            "__objc_clsrolist" => {
                // class_ro_t: baseMethods at 32.
                for ro in records() {
                    found.note(ctx, ro, 32);
                }
            }
            _ => {}
        }
    }
    found.lists
}

/// The method lists found so far, and the classes visited.
#[derive(Default)]
struct MethodListFinder {
    lists: Vec<u32>,
    seen: hashbrown::HashSet<u32>,
    classes_seen: hashbrown::HashSet<(u32, u64)>,
}

impl MethodListFinder {
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
/// the selector string it names: an input's, else an objc stub's,
/// which serves the same selector (ld64 keeps one slot per selector),
/// else a new one in the __objc_selrefs tail.
struct SelrefFinder {
    /// The inputs' selector references, by the selector string
    /// subsection they point at.
    input: hashbrown::HashMap<u32, u32>,
    /// The objc stubs' slots, by selector.
    stub: hashbrown::HashMap<Vec<u8>, usize>,
    /// The slots added to the tail, by selector string subsection.
    extra: hashbrown::HashMap<u32, usize>,
}

impl SelrefFinder {
    fn new<E: Target>(ctx: &Context<E>) -> Self {
        let mut input = hashbrown::HashMap::new();
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
                input.entry(name).or_insert(ctx.resolve_isec(i) as u32);
            }
        }
        let stub = (ctx.objc_stubs.symbols.iter().enumerate())
            .map(|(i, (_, sel))| (sel.as_bytes().to_vec(), i))
            .collect();
        Self { input, stub, extra: hashbrown::HashMap::new() }
    }

    fn get<E: Target>(&mut self, ctx: &mut Context<E>, sel: u32) -> ObjcRef {
        if let Some(&slot) = self.input.get(&sel) {
            return ObjcRef::Isec(slot, 0);
        }
        if let Some(&i) = self.stub.get(cstring_of(ctx.isecs[sel as usize].data())) {
            return ObjcRef::TailSelref(i);
        }
        let n = *self.extra.entry(sel).or_insert_with(|| {
            ctx.objc_stubs.extra_selrefs.push(sel);
            ctx.objc_stubs.extra_selrefs.len() - 1
        });
        ObjcRef::TailSelref(ctx.objc_stubs.symbols.len() + n)
    }
}

/// The methods of a classic method list as a relative list holds them,
/// naming a selector reference rather than the selector string; None
/// if the list is not the whole of its subsection in the classic
/// 24-byte form, or an entry's selector is not a string in the image.
fn relative_methods<E: Target>(
    ctx: &mut Context<E>,
    list: u32,
    selrefs: &mut SelrefFinder,
) -> Option<Vec<ObjcMethod>> {
    let data = ctx.isecs[list as usize].data();
    if data.len() < 8 {
        return None;
    }
    let entsize_flags = u32::from_le_bytes(data[0..4].try_into().unwrap());
    let count = u32::from_le_bytes(data[4..8].try_into().unwrap()) as u64;
    if entsize_flags & 0x8000_0000 != 0
        || entsize_flags & 0xffff != 24
        || 8 + 24 * count != data.len() as u64
    {
        return None;
    }
    // Every name must be a whole selector string in the image before
    // any gets a selector reference: a list left absolute needs none.
    let mut sels = Vec::with_capacity(count as usize);
    for i in 0..count {
        let name = objc_pointer_at(ctx, list, 8 + 24 * i);
        let Some((sel, 0)) = name.and_then(|r| objc_ref_location(ctx, r)) else { return None };
        sels.push(sel);
    }
    let mut methods = Vec::with_capacity(count as usize);
    for (i, sel) in sels.into_iter().enumerate() {
        let at = 8 + 24 * i as u64;
        let types = objc_pointer_at(ctx, list, at + 8).unwrap_or(ObjcRef::Null);
        let imp = objc_pointer_at(ctx, list, at + 16).unwrap_or(ObjcRef::Null);
        methods.push(ObjcMethod { name: selrefs.get(ctx, sel), types, imp });
    }
    Some(methods)
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
/// class that absorbed a +load category joins __objc_nlclslist. The
/// categories on a class from another image merge into the first of
/// them instead (see merge_into_first_category). A category whose data
/// is not in the expected shape is left alone. Runs after the method
/// lists have been rewritten in relative form, when it merges those;
/// with classic lists the merged list is a classic one.
pub fn merge_objc_categories<E: Target>(ctx: &mut Context<E>) {
    if ctx.args.relocatable || !ctx.args.objc_category_merging {
        return;
    }
    let relative = objc_relative_method_lists(ctx);
    let (mut classes, class_idx) = defined_classes(ctx);
    let (mut cats, catlists, imported) = find_categories(ctx, &mut classes, &class_idx);
    if cats.is_empty() {
        return;
    }

    let mut writer = MergedListWriter::new(ctx, relative);
    let mut nonlazy_classes: Vec<(u32, u64)> = Vec::new();
    for class in &classes {
        if class.cats.is_empty() {
            continue;
        }

        // Gather everything first, and give up on the class if anything
        // is not in shape: nothing changes until everything checks out.
        let own = ListRefs::of_class(ctx, class);
        let cat_lists: Vec<ListRefs> =
            class.cats.iter().map(|&ci| ListRefs::of_category(ctx, cats[ci].isec)).collect();
        let ro_ok = ro_rewritable(ctx, class);
        let lists = match merge_lists(ctx, &own, &cat_lists, relative) {
            Some(lists) if ro_ok => lists,
            lists => {
                if std::env::var_os("MOLD_OBJC_DEBUG").is_some() {
                    eprintln!(
                        "category merging: class at {:?} with {} categories skipped (lists in shape: {}, ro reachable: {})",
                        class.cls,
                        class.cats.len(),
                        lists.is_some(),
                        ro_ok
                    );
                }
                continue;
            }
        };

        // Write the merged lists, named after the class and its
        // categories, and drop the lists they supersede.
        let class_name = objc_cstring_at(ctx, objc_pointer_at(ctx, class.ro.0, class.ro.1 + 24))
            .unwrap_or_default();
        let cat_names: Vec<&str> = class.cats.iter().map(|&ci| cats[ci].name.as_str()).collect();
        let merged = writer.write(ctx, lists, &format!("{class_name}({})", cat_names.join("|")));
        drop_superseded_lists(ctx, &own, &merged, &cat_lists);

        // Point the class and its metaclass at new ro records holding
        // the merged lists.
        let ro = rewrite_ro(ctx, class.ro, merged.imethods, merged.protocols, merged.iprops);
        let meta_ro =
            rewrite_ro(ctx, class.meta_ro, merged.cmethods, merged.protocols, merged.cprops);
        retarget_class_data(ctx, class.cls, ro);
        retarget_class_data(ctx, class.meta, meta_ro);

        for &ci in &class.cats {
            ctx.isecs[cats[ci].isec as usize].set_alive(false);
            cats[ci].merged = true;
        }
        if !class.nonlazy && class.cats.iter().any(|&ci| cats[ci].nonlazy) {
            nonlazy_classes.push(class.cls);
        }
    }
    for class in &imported {
        merge_into_first_category(ctx, &mut writer, &mut cats, class);
    }

    rebuild_category_lists(ctx, &catlists, &cats);

    // Classes that absorbed a +load category become non-lazy.
    for cls in nonlazy_classes {
        add_data_blob(
            ctx,
            "__objc_nlclslist",
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
/// first listed, and their index by class_t location.
fn defined_classes<E: Target>(
    ctx: &Context<E>,
) -> (Vec<DefinedClass>, hashbrown::HashMap<(u32, u64), usize>) {
    let mut classes: Vec<DefinedClass> = Vec::new();
    let mut class_idx = hashbrown::HashMap::new();
    for i in 0..ctx.isecs.len() {
        let isec = &ctx.isecs[i];
        if !isec.is_alive() || ctx.is_internal(isec.file as usize) {
            continue;
        }
        let nonlazy = match ctx.hdr_of(isec).sectname() {
            "__objc_classlist" => false,
            "__objc_nlclslist" => true,
            _ => continue,
        };
        for cls in list_entries(ctx, i as u32).filter_map(|r| objc_ref_location(ctx, r?)) {
            // class_t: isa (the metaclass), superclass, cache, vtable,
            // data (the ro).
            let ro = objc_class_ro(ctx, cls);
            let meta = objc_pointer_at(ctx, cls.0, cls.1).and_then(|r| objc_ref_location(ctx, r));
            let meta_ro = meta.and_then(|m| objc_class_ro(ctx, m));
            let (Some(ro), Some(meta), Some(meta_ro)) = (ro, meta, meta_ro) else { continue };
            let idx = *class_idx.entry(cls).or_insert_with(|| {
                classes.push(DefinedClass { cls, meta, ro, meta_ro, nonlazy: false, cats: vec![] });
                classes.len() - 1
            });
            classes[idx].nonlazy |= nonlazy;
        }
    }
    (classes, class_idx)
}

/// A category on a class defined in the image.
struct Category {
    /// Its category_t, a subsection of its own.
    isec: u32,
    name: String,
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

/// The categories on classes defined in the image and on classes other
/// images define, in __objc_catlist order, each noted on its class (an
/// imported one by its symbol, in the order first extended), and the
/// category-list subsections. A category sits in __objc_catlist and, if
/// it has a +load, in __objc_nlcatlist too; a list subsection may hold
/// several.
fn find_categories<E: Target>(
    ctx: &Context<E>,
    classes: &mut [DefinedClass],
    class_idx: &hashbrown::HashMap<(u32, u64), usize>,
) -> (Vec<Category>, Vec<CategoryList>, Vec<ImportedClass>) {
    let mut cats: Vec<Category> = Vec::new();
    let mut cat_idx: hashbrown::HashMap<u32, usize> = hashbrown::HashMap::new();
    let mut lists = Vec::new();
    let mut imported: Vec<ImportedClass> = Vec::new();
    let mut imported_idx: hashbrown::HashMap<crate::symbol::SymbolId, usize> =
        hashbrown::HashMap::new();
    for i in 0..ctx.isecs.len() {
        let isec = &ctx.isecs[i];
        if !isec.is_alive() || ctx.is_internal(isec.file as usize) {
            continue;
        }
        let nonlazy = match ctx.hdr_of(isec).sectname() {
            "__objc_catlist" => false,
            "__objc_nlcatlist" => true,
            _ => continue,
        };
        let mut list = CategoryList { isec: i as u32, nonlazy, entries: Vec::new() };
        for r in list_entries(ctx, i as u32) {
            let r = r.unwrap_or(ObjcRef::Null);
            let ci = category_and_class(ctx, r, class_idx).and_then(|(cat, class)| {
                if let Some(&ci) = cat_idx.get(&cat) {
                    return Some(ci);
                }
                let name = objc_cstring_at(ctx, objc_pointer_at(ctx, cat, 0))?;
                cats.push(Category { isec: cat, name, nonlazy: false, merged: false });
                cat_idx.insert(cat, cats.len() - 1);
                match class {
                    Extended::Defined(class) => classes[class].cats.push(cats.len() - 1),
                    Extended::Imported(sym) => {
                        let k = *imported_idx.entry(sym).or_insert_with(|| {
                            imported.push(ImportedClass { sym, cats: Vec::new() });
                            imported.len() - 1
                        });
                        imported[k].cats.push(cats.len() - 1);
                    }
                }
                Some(cats.len() - 1)
            });
            if let Some(ci) = ci {
                cats[ci].nonlazy |= nonlazy;
            }
            list.entries.push((r, ci));
        }
        lists.push(list);
    }
    (cats, lists, imported)
}

/// A class another image defines, by its symbol, and the categories on
/// it that can merge, in __objc_catlist order.
struct ImportedClass {
    sym: crate::symbol::SymbolId,
    cats: Vec<usize>,
}

/// The class a category extends: one defined in the image (its index
/// among the defined classes), or one another image defines, by its
/// symbol.
#[derive(Clone, Copy)]
enum Extended {
    Defined(usize),
    Imported(crate::symbol::SymbolId),
}

/// The category a category-list entry points at and the class it
/// extends, if that class is defined in the image or imported, and the
/// category_t is a subsection of its own, long enough to have instance
/// properties.
fn category_and_class<E: Target>(
    ctx: &Context<E>,
    r: ObjcRef,
    class_idx: &hashbrown::HashMap<(u32, u64), usize>,
) -> Option<(u32, Extended)> {
    let (cat, off) = objc_ref_location(ctx, r)?;
    if off != 0 || ctx.isecs[cat as usize].size < 48 {
        return None;
    }
    // category_t: name, cls, ...
    match objc_pointer_at(ctx, cat, 8)? {
        ObjcRef::Sym(id, 0) if ctx.symbols[id].input_section().is_none() => {
            Some((cat, Extended::Imported(id)))
        }
        cls => Some((cat, Extended::Defined(*class_idx.get(&objc_ref_location(ctx, cls)?)?))),
    }
}

/// Merges the categories on a class another image defines, given in
/// __objc_catlist order, into the first of them, as ld-prime does: the
/// runtime then attaches one category. Only the lists of a kind another
/// category has merge - into a new list named after the class and the
/// categories, __OBJC_$_INSTANCE_METHODS_NSView(A|B), in the order
/// merging into a class gives them - and the first category's record,
/// which stays in __objc_catlist, points at them; the other categories
/// go, with the lists merged. ld-prime merges none of a class's
/// categories if one has a +load: the runtime calls each category's.
fn merge_into_first_category<E: Target>(
    ctx: &mut Context<E>,
    writer: &mut MergedListWriter,
    cats: &mut [Category],
    class: &ImportedClass,
) {
    let class_cats = &class.cats;
    if class_cats.len() < 2 || class_cats.iter().any(|&ci| cats[ci].nonlazy) {
        return;
    }
    let cat_lists: Vec<ListRefs> =
        class_cats.iter().map(|&ci| ListRefs::of_category(ctx, cats[ci].isec)).collect();
    let Some(mut lists) = merge_lists(ctx, &ListRefs::default(), &cat_lists, writer.relative)
    else {
        return;
    };
    let others =
        |list: fn(&ListRefs) -> Option<ObjcRef>| cat_lists[1..].iter().any(|c| list(c).is_some());
    if !others(|c| c.imethods) {
        lists.imethods = None;
    }
    if !others(|c| c.cmethods) {
        lists.cmethods = None;
    }
    if !others(|c| c.protocols) {
        lists.protocols = None;
    }
    if !others(|c| c.iprops) {
        lists.iprops = None;
    }
    if !others(|c| c.cprops) {
        lists.cprops = None;
    }
    // A record from an older compiler has no class properties field.
    let first = cats[class_cats[0]].isec;
    if lists.cprops.is_some() && ctx.isecs[first as usize].size < 56 {
        return;
    }

    let name = ctx.symbols[class.sym].name();
    let class_name = name.strip_prefix("_OBJC_CLASS_$_").unwrap_or(name).to_string();
    let cat_names: Vec<&str> = class_cats.iter().map(|&ci| cats[ci].name.as_str()).collect();
    let merged = writer.write(ctx, lists, &format!("{class_name}({})", cat_names.join("|")));
    let superseded: Vec<ListRefs> = cat_lists.iter().map(|c| c.of_kinds(&merged)).collect();
    drop_superseded_lists(ctx, &ListRefs::default(), &merged, &superseded);

    // category_t: name, cls, instanceMethods, classMethods, protocols,
    // instanceProperties, _classProperties.
    let fields = [merged.imethods, merged.cmethods, merged.protocols, merged.iprops, merged.cprops];
    for (off, list) in (16..).step_by(8).zip(fields) {
        if let Some(ObjcRef::Isec(list, 0)) = list {
            set_pointer_field(ctx, first, off, list);
        }
    }
    for &ci in &class_cats[1..] {
        ctx.isecs[cats[ci].isec as usize].set_alive(false);
        cats[ci].merged = true;
    }
}

/// Points the pointer field at `off` of a record at subsection `to`: its
/// relocation is retargeted, or a null field gets one.
fn set_pointer_field<E: Target>(ctx: &mut Context<E>, rec: u32, off: u64, to: u32) {
    if let Some((obj, k)) = objc_pointer_reloc(ctx, rec, off) {
        let rel = &mut ctx.objs[obj].relocs[k];
        rel.set_target(RelocTarget::Section(to));
        rel.addend = 0;
        return;
    }
    // The record's relocations are a run of its object's; the run
    // moves to the end, one longer.
    let isec = &ctx.isecs[rec as usize];
    let obj = &mut ctx.objs[isec.file as usize];
    let run = isec.rel_offset as usize..(isec.rel_offset + isec.nrels) as usize;
    let mut rels = obj.relocs[run].to_vec();
    rels.push(crate::input_sections::Reloc {
        offset: off as u32,
        r_type: E::RELOC_UNSIGNED,
        size: 8,
        is_pcrel: false,
        is_subtracted: false,
        target: RelocTarget::Section(to).pack(),
        addend: 0,
    });
    let start = obj.relocs.len() as u32;
    obj.relocs.extend(rels);
    let isec = &mut ctx.isecs[rec as usize];
    isec.rel_offset = start;
    isec.nrels += 1;
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

    /// These lists, of the kinds `kinds` has only.
    fn of_kinds(&self, kinds: &ListRefs) -> Self {
        Self {
            imethods: kinds.imethods.and(self.imethods),
            cmethods: kinds.cmethods.and(self.cmethods),
            protocols: kinds.protocols.and(self.protocols),
            iprops: kinds.iprops.and(self.iprops),
            cprops: kinds.cprops.and(self.cprops),
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
    if let Some(list) = ctx.objc_methlist.lists.iter().find(|l| l.isec == isec) {
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
}

/// A protocol list's protocols (none for no list): a count (8 bytes),
/// then pointers.
fn read_protocol_list<E: Target>(ctx: &Context<E>, list: Option<ObjcRef>) -> Option<Vec<ObjcRef>> {
    let Some(r) = list else { return Some(Vec::new()) };
    let (isec, off) = objc_ref_location(ctx, r)?;
    let data = ctx.isecs[isec as usize].data();
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
    let data = ctx.isecs[isec as usize].data();
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
        suffix: &str,
    ) -> ListRefs {
        let name = |ctx: &mut Context<E>, prefix: &str, isec: u32| {
            ctx.extra_local_syms.push((String::leak(format!("{prefix}{suffix}")), isec));
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
        add_data_blob(ctx, "__objc_data", 0, fields)
    }
}

/// Writes a protocol list, as read_protocol_list reads it.
fn add_protocol_list<E: Target>(ctx: &mut Context<E>, protocols: &[ObjcRef]) -> u32 {
    let mut fields = vec![DataField::Bytes((protocols.len() as u64).to_le_bytes().to_vec())];
    fields.extend(protocols.iter().map(|&r| DataField::Ptr(r)));
    add_data_blob(ctx, "__objc_const", 0, fields)
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
    add_data_blob(ctx, "__objc_const", 0, fields)
}

/// Drops the lists the merged ones supersede - the class's own of each
/// kind merged, and all of the categories' - as ld64's output keeps
/// only the merged lists (which carry the names).
fn drop_superseded_lists<E: Target>(
    ctx: &mut Context<E>,
    own: &ListRefs,
    merged: &ListRefs,
    cats: &[ListRefs],
) {
    if merged.imethods.is_some() {
        drop_method_list(ctx, own.imethods);
    }
    if merged.cmethods.is_some() {
        drop_method_list(ctx, own.cmethods);
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
        drop_method_list(ctx, c.imethods);
        drop_method_list(ctx, c.cmethods);
        drop_list(ctx, c.protocols);
        drop_list(ctx, c.iprops);
        drop_list(ctx, c.cprops);
    }
}

fn drop_list<E: Target>(ctx: &mut Context<E>, list: Option<ObjcRef>) {
    if let Some((isec, 0)) = list.and_then(|r| objc_ref_location(ctx, r)) {
        ctx.isecs[isec as usize].set_alive(false);
    }
}

/// Drops a method list, which __objc_methlist no longer writes either
/// if it is one convert_objc_method_lists rewrote.
fn drop_method_list<E: Target>(ctx: &mut Context<E>, list: Option<ObjcRef>) {
    if let Some((isec, 0)) = list.and_then(|r| objc_ref_location(ctx, r)) {
        ctx.objc_methlist.lists.retain(|l| l.isec != isec);
        ctx.isecs[isec as usize].set_alive(false);
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
    let data = ctx.isecs[ro.0 as usize].data()[ro.1 as usize..ro.1 as usize + 16].to_vec();
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
    let sect = match ctx.hdr_of(&ctx.isecs[ro.0 as usize]).sectname() {
        "__objc_data" => "__objc_data",
        _ => "__objc_const",
    };
    let blob = add_data_blob(ctx, sect, 0, fields);
    let isec = &mut ctx.isecs[ro.0 as usize];
    if ro.1 == 0 && isec.size as u64 == len {
        // The record was a subsection of its own: replace it, so its
        // symbol names the new record too (ld64 keeps
        // __OBJC_CLASS_RO_$_Foo).
        isec.set_alive(false);
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
/// survivors is rewritten with those.
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
        ctx.isecs[list.isec as usize].set_alive(false);
        let survivors: Vec<DataField> = (list.entries.iter())
            .filter(|&&(_, ci)| !merged(ci))
            .map(|&(r, _)| DataField::Ptr(r))
            .collect();
        if !survivors.is_empty() {
            let sect = if list.nonlazy { "__objc_nlcatlist" } else { "__objc_catlist" };
            add_data_blob(ctx, sect, S_ATTR_NO_DEAD_STRIP, survivors);
        }
    }
}
