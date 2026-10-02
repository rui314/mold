//! -make_mergeable: the LC_ATOM_INFO record of a dylib's contents, which
//! lets a later link's -merge_* take the dylib as the objects it was
//! made of (mergeable.rs reads the record; see there for its format).
//!
//! The record has an entry for each piece the linker moves as a whole:
//! each live subsection of the objects, an alias for each other symbol
//! it defines, a tentative definition, an import or a symbol the linker
//! defines, an initializer offset the link made of an initializer
//! pointer, and an unwind record (an __eh_frame CIE or FDE, a
//! __compact_unwind record). Each entry has its name, linkage and
//! section, and fixups that say what the relocations of its object
//! said, by entry; its bytes are the dylib's own, linked (an unwind
//! record's, which the image has none of, are the object's). The dylibs
//! the dylib links are recorded by install name, for the merging link
//! to load in its place, and the objects with debug info by the notes
//! that point a debugger at them.
//!
//! ld-prime orders the entries by object, and in an object by section
//! and address, then the imports and what the linker made; the
//! imports of a dylib come in an order of its own, which isn't
//! reproduced (they go by name here).

use hashbrown::HashMap;

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::fatal;
use crate::input_files::{FileId, ObjectFile};
use crate::input_sections::{InputSection, NO_REPLACEMENT, Reloc, RelocTarget};
use crate::macho::*;
use crate::mergeable::{CustomSection, ctype, fk, kind, scope};
use crate::symbol::SymbolId;
use crate::target::Target;

mod objc;
mod write;

/// The content types the writer gives besides the standard sections'.
const CT_NONE: u8 = 1;
const CT_COMPACT_UNWIND: u8 = 32;
const CT_CUSTOM: u8 = 63;
const CT_DATA: u8 = 27;
const CT_COMMON: u8 = 66;
const CT_THREAD_VARS: u8 = 57;

/// LC_ATOM_INFO's data in __LINKEDIT: the record, but for what depends
/// on where it lands in the file, filled in as it is copied out.
#[derive(Debug)]
pub struct MergeableRecordSection {
    pub hdr: ChunkHeader,
    /// The record, with the content offsets of the entries whose bytes
    /// are the image's left to fill.
    pub contents: Vec<u8>,
    /// Where in the record each such offset goes, and the file offset
    /// of the bytes it points at.
    pub image_contents: Vec<(u32, u64)>,
    /// The content pool's offset in the record.
    pub pool_offset: u32,
}

impl MergeableRecordSection {
    pub fn new() -> Self {
        Self {
            hdr: ChunkHeader::linkedit(),
            contents: Vec::new(),
            image_contents: Vec::new(),
            pool_offset: 0,
        }
    }
}

impl Default for MergeableRecordSection {
    fn default() -> Self {
        Self::new()
    }
}

/// Copies the record out, pointing its entries at their bytes now that
/// the record has its place: by offsets back from the content pool, and
/// the whole file by one back from the record.
pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let sec = &ctx.mergeable_record;
    buf[..sec.contents.len()].copy_from_slice(&sec.contents);
    let pool = sec.hdr.fileoff as i64 + sec.pool_offset as i64;
    for &(at, fileoff) in &sec.image_contents {
        let off = (fileoff as i64 - pool) as i32;
        buf[at as usize..at as usize + 4].copy_from_slice(&off.to_le_bytes());
    }
    buf[0x54..0x58].copy_from_slice(&(-(sec.hdr.fileoff as i32)).to_le_bytes());
    buf[0x58..0x5c].copy_from_slice(&(ctx.output_size as u32).to_le_bytes());
}

/// Builds the record, once the sections have their places in the file.
pub fn build<E: Target>(ctx: &mut Context<E>) {
    let mut b = Builder::new(ctx);
    b.add_object_entries();
    let selrefs = b.add_objc_entries();
    b.add_object_fixups();
    b.add_objc_fixups(&selrefs);
    b.add_objc_placeholders();
    b.add_init_offsets();
    b.add_cfi_entries();
    b.add_compact_unwind_entries();
    let record = b.finish();
    let out = record.serialize::<E>(ctx);
    let sec = &mut ctx.mergeable_record;
    sec.hdr.size = out.bytes.len() as u64;
    sec.contents = out.bytes;
    sec.image_contents = out.image_contents;
    sec.pool_offset = out.pool_offset;
}

/// Where an entry's bytes are.
#[derive(Clone, Copy, Debug)]
enum Content {
    None,
    /// At this offset of the output file.
    Image(u64),
    /// These, which go in the record's content pool.
    Pool(&'static [u8]),
}

/// What a fixup refers to, before the entries have their final places:
/// an entry of the objects, one the linker made, or the entry of a
/// symbol the objects import or the linker defines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum To {
    Entry(u32),
    Tail(u32),
    Sym(SymbolId),
    /// The entry before the one of the fixup: an alias's target, made
    /// with it.
    Prev,
}

#[derive(Clone, Copy, Debug)]
struct OutFixup {
    offset: u32,
    target: To,
    kind: u16,
    addend: i64,
    /// The entry a difference subtracts.
    from: Option<To>,
    /// The second instruction of a fused pair, in instructions from the
    /// first, and the size of the access it makes.
    other: u8,
    scale: u8,
}

impl OutFixup {
    fn new(offset: u32, target: To, kind: u16, addend: i64) -> Self {
        Self { offset, target, kind, addend, from: None, other: 0, scale: 0 }
    }
}

#[derive(Clone, Debug)]
struct OutEntry {
    name: Option<&'static [u8]>,
    scope: u8,
    kind: u8,
    content_type: u8,
    cold: bool,
    dds_if_refs_live: bool,
    no_dead_strip: bool,
    /// An import's strength: 1 weak, 2 strong.
    import: u8,
    custom_section: Option<u8>,
    size: u32,
    content: Content,
    /// An import's library, by its index among the dependencies.
    dylib: Option<u8>,
    p2align: u8,
    modulus: u16,
    debug: u16,
    fixups: Vec<OutFixup>,
}

impl OutEntry {
    fn new(scope: u8, kind: u8, content_type: u8) -> Self {
        Self {
            name: None,
            scope,
            kind,
            content_type,
            cold: false,
            dds_if_refs_live: false,
            no_dead_strip: false,
            import: 0,
            custom_section: None,
            size: 0,
            content: Content::None,
            dylib: None,
            p2align: 0,
            modulus: 0,
            debug: 0,
            fixups: Vec::new(),
        }
    }

    /// The flags word: scope, kind, content type, the dead-strip and
    /// import bits and the custom section.
    fn flags(&self) -> u32 {
        self.scope as u32
            | (self.kind as u32) << 3
            | (self.content_type as u32) << 8
            | (self.cold as u32) << 15
            | (self.dds_if_refs_live as u32) << 16
            | (self.no_dead_strip as u32) << 17
            | (self.import as u32) << 19
            | (self.custom_section.unwrap_or(0xff) as u32) << 21
    }
}

/// A dylib's identity as the record keeps it (DylibFileInfoRO_2).
#[derive(Debug, Default)]
struct DylibRecord {
    install_name: Vec<u8>,
    current_version: u32,
    compatibility_version: u32,
    platforms: Vec<u32>,
    reexports: Vec<Vec<u8>>,
    clients: Vec<Vec<u8>>,
}

/// An object's debug notes (DebugNoteFileInfoRO_2).
#[derive(Debug)]
struct DebugRecord {
    mtime: u32,
    /// The object's CPU subtype, as N_OSO's n_sect has it.
    cpusubtype: u8,
    source_dir: Vec<u8>,
    source_name: Vec<u8>,
    object_path: Vec<u8>,
    install_name: Vec<u8>,
}

/// The entries and tables, in their final order.
struct MergeableRecord {
    entries: Vec<OutEntry>,
    /// Each fixup with its target and subtracted entry numbered.
    fixups: Vec<(OutFixup, u32, u32)>,
    first_fixup: Vec<u32>,
    sections: Vec<CustomSection>,
    own: DylibRecord,
    deps: Vec<DylibRecord>,
    debug: Vec<DebugRecord>,
    flags: u64,
}

struct Builder<'a, E: Target> {
    ctx: &'a Context<E>,
    /// The entries of the objects, in their order.
    entries: Vec<OutEntry>,
    /// The subsection each of them stands for.
    entry_isec: Vec<Option<u32>>,
    /// The entries the linker made, which follow the imports.
    tail: Vec<OutEntry>,
    /// Each live subsection's entry, the objects' and the linker's: the
    /// first, if its records are entries of their own (see record_size).
    isec_entry: HashMap<u32, To>,
    /// The size of the records of the subsections split so.
    isec_split: HashMap<u32, u32>,
    /// The entries that symbols name but for their subsection's
    /// (aliases, tentative definitions).
    sym_entry: HashMap<SymbolId, To>,
    /// The aliases standing for the functions identical code folding
    /// folded, each with its subsection (see add_folded_function).
    folded: Vec<(u32, u32)>,
    sections: Vec<CustomSection>,
    debug: Vec<DebugRecord>,
    /// Each object's debug notes, by its 1-based index, 0 for none.
    obj_debug: Vec<u16>,
}

impl<'a, E: Target> Builder<'a, E> {
    fn new(ctx: &'a Context<E>) -> Self {
        Self {
            ctx,
            entries: Vec::new(),
            entry_isec: Vec::new(),
            tail: Vec::new(),
            isec_entry: HashMap::new(),
            isec_split: HashMap::new(),
            sym_entry: HashMap::new(),
            folded: Vec::new(),
            sections: Vec::new(),
            debug: Vec::new(),
            obj_debug: vec![0; ctx.objs.len()],
        }
    }

    fn push_entry(&mut self, entry: OutEntry, isec: Option<u32>) -> u32 {
        self.entries.push(entry);
        self.entry_isec.push(isec);
        (self.entries.len() - 1) as u32
    }

    /// Adds a subsection's entry, or the entries of its records (see
    /// split_records); returns the first.
    fn push_records(&mut self, entry: OutEntry, id: u32, record: Option<u32>) -> u32 {
        let first = self.entries.len() as u32;
        for rec in self.split_records(id, entry, record) {
            self.push_entry(rec, Some(id));
        }
        first
    }

    /// The entries of the records a subsection holds several of, of
    /// `record` bytes each, as ld-prime takes a list section's (see
    /// record_size); its own entry if it holds one.
    fn split_records(&mut self, id: u32, entry: OutEntry, record: Option<u32>) -> Vec<OutEntry> {
        let Some(size) = record.filter(|&r| entry.size > r && entry.size.is_multiple_of(r)) else {
            return vec![entry];
        };
        self.isec_split.insert(id, size);
        let content = |k: u32| record_content(entry.content, k * size, size);
        (0..entry.size / size)
            .map(|k| OutEntry { size, content: content(k), ..entry.clone() })
            .collect()
    }

    /// Which of a subsection's entries each fixup goes to (see
    /// split_records), with its offset there.
    fn place_fixups(&self, id: u32, fixups: Vec<OutFixup>) -> Vec<(usize, OutFixup)> {
        let size = self.isec_split.get(&id).copied();
        fixups
            .into_iter()
            .map(|f| match size {
                Some(size) => {
                    ((f.offset / size) as usize, OutFixup { offset: f.offset % size, ..f })
                }
                None => (0, f),
            })
            .collect()
    }

    /// The objects' entries: object by object, the live subsections by
    /// section and address, each followed by the aliases of the other
    /// symbols in it, and then the tentative definitions the object
    /// made.
    fn add_object_entries(&mut self) {
        let ctx = self.ctx;
        for (obj_idx, obj) in ctx.objs.iter().enumerate() {
            if !obj.is_alive || ctx.is_internal(obj_idx) {
                continue;
            }
            let debug = self.add_debug_record(obj);
            self.obj_debug[obj_idx] = debug;
            let mut subsecs = obj.subsecs.clone();
            subsecs.sort_by_key(|&id| (ctx.isecs[id].shndx, ctx.isecs[id].input_addr));
            for id in subsecs {
                self.add_isec_entry(obj, id, debug);
            }
            self.add_absolute_entries(obj_idx);
            self.add_tentative_defs(obj_idx, debug);
        }
        // Each folded function's alias is of the function kept, which
        // may come later.
        for k in 0..self.folded.len() {
            let (alias, id) = self.folded[k];
            if let Some(kept) = self.isec_target(id) {
                let fixup = OutFixup::new(0, kept, fk::ALIAS_OF, 0);
                self.entries[alias as usize].fixups.push(fixup);
            }
        }
    }

    /// The debug notes of an object with DWARF, as a final link would
    /// write them; returns their 1-based index, 0 for none.
    fn add_debug_record(&mut self, obj: &ObjectFile) -> u16 {
        if !obj.has_debug_info || self.ctx.args.strip_debug {
            return 0;
        }
        let cwd = std::env::current_dir().unwrap_or_default();
        let stabs = crate::chunks::symtab::object_stabs_opening(self.ctx, obj, &cwd);
        let [dir, name, oso] = &stabs[..] else { return 0 };
        self.debug.push(DebugRecord {
            mtime: oso.ent.n_value as u32,
            cpusubtype: oso.ent.n_sect,
            source_dir: dir.name.to_vec(),
            source_name: name.name.to_vec(),
            object_path: oso.name.to_vec(),
            install_name: self.ctx.args.output_install_name().to_vec(),
        });
        self.debug.len() as u16
    }

    /// A live subsection's entry (its records', if it is a list of
    /// them), and those of its aliases.
    fn add_isec_entry(&mut self, obj: &ObjectFile, id: u32, debug: u16) {
        let ctx = self.ctx;
        let isec = &ctx.isecs[id];
        if !isec.is_alive() {
            return;
        }
        let hdr = ctx.hdr_of(isec);
        // A slot of an input __DATA,__got the link moved into its GOT
        // (see passes::fold_input_got) is an entry in its place all the
        // same, as ld-prime records it, with the bytes of the GOT slot.
        let got_slot = isec.replacement != NO_REPLACEMENT && is_input_got(hdr);
        if isec.replacement != NO_REPLACEMENT && !got_slot {
            return self.add_folded_function(obj, id, debug);
        }
        if !has_entries(hdr) {
            return;
        }
        // A list record's labels name nothing, nor do a literal's but a
        // symbol's of its own (see Context::subsec_label); neither has
        // aliases.
        let literal = merges_by_content(hdr);
        let record = crate::input_files::is_record_list(hdr, obj.subsections_via_symbols);
        let label = if record { None } else { ctx.subsec_label_index(id as usize) };
        let (content_type, custom) = self.content_type(hdr);
        let mut entry = match label {
            Some(i) => named_entry(ctx, obj, i, hdr, content_type, debug),
            None if literal => {
                OutEntry::new(scope::HIDDEN, kind::ANON_COAL_BY_CONTENT, content_type)
            }
            None => OutEntry::new(0, kind::ANON, content_type),
        };
        entry.custom_section = custom;
        // What is live whatever refers to it: an initializer or a
        // terminator, and what its section says (but a class reference,
        // which ld-prime reads as a pointer to the class whatever its
        // section's attributes; not so a reference to a superclass).
        let classref = hdr.sectname() == b"__objc_classrefs";
        entry.no_dead_strip |= hdr.flags & S_ATTR_NO_DEAD_STRIP != 0 && !classref
            || matches!(hdr.section_type(), S_MOD_INIT_FUNC_POINTERS | S_MOD_TERM_FUNC_POINTERS);
        entry.dds_if_refs_live = hdr.flags & S_ATTR_LIVE_SUPPORT != 0;
        entry.size = isec.size;
        entry.p2align = isec.p2align;
        if !isec.is_record() {
            entry.modulus = (isec.input_addr & ((1 << isec.p2align) - 1)) as u16;
        }
        let placed = if got_slot { &ctx.isecs[isec.replacement] } else { isec };
        entry.content = self.isec_content(placed, ctx.hdr_of(placed));
        let entry_idx = self.push_records(entry, id, record_size(hdr));
        self.isec_entry.insert(id, To::Entry(entry_idx));
        if got_slot {
            self.isec_entry.insert(isec.replacement, To::Entry(entry_idx));
        }
        if let Some(i) = label {
            self.sym_entry.insert(obj.symbols[i], To::Entry(entry_idx));
        }
        if !record && !literal {
            self.add_aliases(obj, id, label, entry_idx, debug);
        }
    }

    /// A function identical code folding folded into another, which
    /// has no bytes of its own: an alias of the kept function's entry,
    /// by the name its own subsection had, as ld-prime records the
    /// functions its deduplication folds - so that a merging link finds
    /// an exported Swift function folded so. Its other symbols are
    /// aliases of that alias, in its place; it goes after the imports
    /// (see final_order), its target filled in once every entry is made.
    fn add_folded_function(&mut self, obj: &ObjectFile, id: u32, debug: u16) {
        let ctx = self.ctx;
        let Some(label) = ctx.subsec_label_index(id as usize) else { return };
        let sym_id = obj.symbols[label];
        // (A losing copy of a weak definition, whose symbol is the
        // winner's, which may be the folded one, has no entry at all.)
        if !ctx.folded_subsec_names.contains_key(&sym_id)
            || ctx.symbols[sym_id].input_section() != Some(id)
        {
            return;
        }
        let (scope, kind) = linkage(ctx, &obj.nlists[label], sym_id);
        let kind = if kind == kind::WEAK_DEF { kind::WEAK_DEF_ALIAS } else { kind::ALIAS };
        let mut alias = OutEntry::new(scope, kind, CT_NONE);
        alias.name = Some(ctx.symbols[sym_id].name().as_bytes());
        let idx = self.push_entry(alias, None);
        self.folded.push((idx, id));
        self.sym_entry.insert(sym_id, To::Entry(idx));
        self.add_aliases(obj, id, Some(label), idx, debug);
    }

    /// The bytes of a subsection: the image's where the link put it
    /// in a section of the file, none for zero fill.
    fn isec_content(&self, isec: &InputSection, hdr: &MachSection) -> Content {
        if matches!(hdr.section_type(), S_ZEROFILL | S_GB_ZEROFILL | S_THREAD_LOCAL_ZEROFILL) {
            return Content::None;
        }
        match isec.output_section() {
            Some(chunk) if isec.offset != u32::MAX => {
                let out = self.ctx.chunk_header(chunk);
                if out.is_zerofill() {
                    Content::None
                } else {
                    Content::Image(out.fileoff + isec.offset as u64)
                }
            }
            _ => Content::Pool(isec.data()),
        }
    }

    /// The other symbols a subsection defines, each an alias of the
    /// subsection's entry at its offset there; but the labels the
    /// compiler made for itself.
    fn add_aliases(
        &mut self,
        obj: &ObjectFile,
        id: u32,
        label: Option<usize>,
        entry: u32,
        debug: u16,
    ) {
        let ctx = self.ctx;
        let isec = &ctx.isecs[id];
        let start = isec.input_addr as u64;
        let end = start + isec.size as u64;
        let mut syms: Vec<usize> = (0..obj.nlists.len())
            .filter(|&i| {
                let n = &obj.nlists[i];
                Some(i) != label
                    && !n.is_stab()
                    && n.n_type() == N_SECT
                    && n.n_sect as u32 == isec.shndx + 1
                    && (start..end.max(start + 1)).contains(&n.n_value)
                    && !crate::input_files::is_private_label(ctx.symbols[obj.symbols[i]].name())
            })
            .collect();
        syms.sort_by_key(|&i| (obj.nlists[i].n_value, std::cmp::Reverse(i)));
        for i in syms {
            let sym_id = obj.symbols[i];
            let sym = &ctx.symbols[sym_id];
            if sym.input_section() != Some(id) {
                continue;
            }
            let (scope, kind) = linkage(ctx, &obj.nlists[i], sym_id);
            let kind = if kind == kind::WEAK_DEF { kind::WEAK_DEF_ALIAS } else { kind::ALIAS };
            let mut alias = OutEntry::new(scope, kind, CT_NONE);
            alias.name = Some(sym.name().as_bytes());
            alias.dds_if_refs_live = true;
            alias.no_dead_strip = obj.nlists[i].n_desc & N_NO_DEAD_STRIP != 0;
            alias.debug = debug;
            let offset = (obj.nlists[i].n_value - start) as i64;
            alias.fixups.push(OutFixup::new(0, To::Entry(entry), fk::ALIAS_OF, offset));
            let idx = self.push_entry(alias, None);
            self.sym_entry.insert(sym_id, To::Entry(idx));
        }
    }

    /// The absolute symbols an object defines, each an entry whose
    /// eight bytes are its value.
    fn add_absolute_entries(&mut self, obj_idx: usize) {
        let ctx = self.ctx;
        let obj = &ctx.objs[obj_idx];
        for (n, &sym_id) in obj.nlists.iter().zip(&obj.symbols) {
            let sym = &ctx.symbols[sym_id];
            if n.is_stab()
                || n.n_type() != N_ABS
                || sym.file() != Some(FileId::Obj(obj_idx as u32))
                || self.sym_entry.contains_key(&sym_id)
            {
                continue;
            }
            let (scope, _) = linkage(ctx, n, sym_id);
            let mut entry = OutEntry::new(scope, kind::ABSOLUTE, CT_DATA);
            entry.name = Some(sym.name().as_bytes());
            entry.size = 8;
            entry.p2align = 3;
            entry.content = Content::Pool(Box::leak(Box::new(sym.value.to_le_bytes())));
            let idx = self.push_entry(entry, None);
            self.sym_entry.insert(sym_id, To::Entry(idx));
        }
    }

    /// The tentative definitions an object made that no definition
    /// replaced, where it is the first to make each.
    fn add_tentative_defs(&mut self, obj_idx: usize, debug: u16) {
        let ctx = self.ctx;
        let obj = &ctx.objs[obj_idx];
        for (n, &sym_id) in obj.nlists.iter().zip(&obj.symbols) {
            if n.is_stab() || !n.is_extern() || n.n_type() != N_UNDF || !n.is_common() {
                continue;
            }
            if self.sym_entry.contains_key(&sym_id) {
                continue;
            }
            let sym = &ctx.symbols[sym_id];
            let Some(isec) = sym.input_section() else { continue };
            if !matches!(sym.file(), Some(FileId::Obj(o)) if ctx.is_internal(o as usize)) {
                continue;
            }
            let isec = &ctx.isecs[isec];
            let scope = if sym.is_private_extern() { scope::HIDDEN } else { scope::GLOBAL };
            let mut entry = OutEntry::new(scope, kind::TENTATIVE_DEF, CT_COMMON);
            entry.name = Some(sym.name().as_bytes());
            entry.size = isec.size;
            entry.p2align = isec.p2align;
            entry.debug = debug;
            let idx = self.push_entry(entry, None);
            self.sym_entry.insert(sym_id, To::Entry(idx));
        }
    }

    /// An object section's content type: that of the standard section
    /// of its name and type, or else a custom one, from the custom
    /// section table.
    fn content_type(&mut self, hdr: &MachSection) -> (u8, Option<u8>) {
        if let Some(ct) = standard_content_type(hdr) {
            return (ct, None);
        }
        let segname = hdr.segname;
        let sectname = hdr.sectname;
        let pos = self
            .sections
            .iter()
            .position(|s| s.segname == segname && s.sectname == sectname && s.flags == hdr.flags);
        let idx = pos.unwrap_or_else(|| {
            self.sections.push(CustomSection { segname, sectname, flags: hdr.flags });
            self.sections.len() - 1
        });
        (CT_CUSTOM, Some(idx as u8))
    }

    /// The fixups of the objects' entries, from their relocations.
    fn add_object_fixups(&mut self) {
        let mut i = 0;
        while i < self.entries.len() {
            let Some(id) = self.entry_isec[i] else {
                i += 1;
                continue;
            };
            let fixups = self.isec_fixups(id as usize);
            for (k, f) in self.place_fixups(id, fixups) {
                self.entries[i + k].fixups.push(f);
            }
            i += 1;
            while i < self.entries.len() && self.entry_isec[i] == Some(id) {
                i += 1;
            }
        }
    }

    /// The entry a symbol refers to, and its offset there.
    fn sym_target(&self, id: SymbolId) -> (To, i64) {
        let ctx = self.ctx;
        if let Some(&to) = self.sym_entry.get(&id) {
            return (to, 0);
        }
        let sym = &ctx.symbols[id];
        if let (Some(FileId::Obj(_)), Some(isec)) = (sym.file(), sym.input_section())
            && let Some((to, off)) = self.isec_target_at(isec, sym.value as i64)
        {
            return (to, off);
        }
        (To::Sym(id), 0)
    }

    /// The entry of a subsection, or of the one that replaced it.
    fn isec_target(&self, isec: u32) -> Option<To> {
        self.isec_entry.get(&(self.ctx.resolve_isec(isec as usize) as u32)).copied()
    }

    /// The entry holding offset `off` of a subsection (of the one that
    /// replaced it), and the offset there.
    fn isec_target_at(&self, isec: u32, off: i64) -> Option<(To, i64)> {
        let isec = self.ctx.resolve_isec(isec as usize) as u32;
        let to = *self.isec_entry.get(&isec)?;
        let Some(&size) = self.isec_split.get(&isec) else { return Some((to, off)) };
        let k = off.max(0) / size as i64;
        let to = match to {
            To::Entry(i) => To::Entry(i + k as u32),
            To::Tail(i) => To::Tail(i + k as u32),
            to => to,
        };
        Some((to, off - k * size as i64))
    }

    /// The entry a relocation refers to, and the offset there.
    fn reloc_target(&self, obj: usize, rel: &Reloc) -> (To, i64) {
        match rel.target() {
            RelocTarget::Sym(idx) => {
                let id = self.ctx.objs[obj].symbols[idx as usize];
                self.other_got_slot(id, rel.addend).unwrap_or_else(|| self.sym_target(id))
            }
            // The offset the relocation's addend has in the subsection
            // is the entry's if it is a record of it.
            RelocTarget::Section(isec) => match self.isec_target_at(isec, rel.addend) {
                Some((to, off)) => (to, off - rel.addend),
                None => fatal!(
                    "{}: -make_mergeable: a relocation refers to a section with no entry",
                    self.ctx.objs[obj].mf.name.display()
                ),
            },
        }
    }

    /// Where a reference to a symbol of an input __DATA,__got with
    /// `addend` reaches past the symbol's slot: the entry of the slot it
    /// reads, and the offset to add to the addend to read it there.
    /// Hand-written code may reach a slot as another's label plus an
    /// offset - the section's start (ltmpN), say - and each slot is an
    /// entry of its own, which a merging link places apart, so the
    /// reference must name the slot it reads. (ld-prime records the first
    /// slot and the offset, which reads the wrong one.)
    fn other_got_slot(&self, id: SymbolId, addend: i64) -> Option<(To, i64)> {
        let ctx = self.ctx;
        let sym = &ctx.symbols[id];
        let (Some(FileId::Obj(obj)), Some(isec)) = (sym.file(), sym.input_section()) else {
            return None;
        };
        let slot = &ctx.isecs[isec as usize];
        let off = sym.value as i64 + addend;
        if !is_input_got(ctx.hdr_of(slot)) || (0..slot.size as i64).contains(&off) {
            return None;
        }
        let addr = u64::try_from(slot.input_addr as i64 + off).ok()?;
        let subsecs = &ctx.objs[obj as usize].subsecs;
        let (other, other_off) = crate::input_files::find_subsec(&ctx.isecs, subsecs, addr)?;
        if ctx.isecs[other].shndx != slot.shndx {
            return None;
        }
        Some((self.isec_target(other as u32)?, other_off as i64 - addend))
    }

    /// The fixups a subsection's relocations make.
    fn isec_fixups(&self, id: usize) -> Vec<OutFixup> {
        let ctx = self.ctx;
        let isec = &ctx.isecs[id];
        let obj = isec.file as usize;
        let rels = ctx.isec_relocs(id);
        let hdr = ctx.hdr_of(isec);
        let data = isec.data();
        let mut out = Vec::with_capacity(rels.len());
        let mut i = 0;
        while i < rels.len() {
            let r = &rels[i];
            let (target, off) = self.reloc_target(obj, r);
            let addend = r.addend + off;
            // UNSIGNED, which both targets number 0: a pointer, or a
            // thread-local variable's offset in the template.
            if r.r_type == 0 {
                let kind = if r.size == 4 {
                    fk::PTR32
                } else if ctx.reloc_target_is_tls(obj, r) {
                    fk::TLV_OFFSET
                } else {
                    fk::PTR64
                };
                out.push(OutFixup::new(r.offset, target, kind, addend));
                i += 1;
                continue;
            }
            // A SUBTRACTOR and the UNSIGNED of its size after it.
            if is_subtractor::<E>(r.r_type) && i + 1 < rels.len() {
                let (to, to_off) = self.reloc_target(obj, &rels[i + 1]);
                let kind = if rels[i + 1].size == 8 { fk::DIFF64 } else { fk::DIFF32 };
                let mut f = OutFixup::new(r.offset, to, kind, rels[i + 1].addend + to_off - off);
                f.from = Some(target);
                out.push(f);
                i += 2;
                continue;
            }
            let fixup = if E::CPUTYPE == CPU_TYPE_ARM64 {
                self.arm64_fixup(obj, data, &rels[i..], target, addend)
            } else {
                x86_64_fixup(hdr, r, target, addend).map(|f| (f, 1))
            };
            let Some((fixup, taken)) = fixup else {
                fatal!(
                    "{}: -make_mergeable: unsupported relocation type {} at 0x{:x}",
                    ctx.objs[obj].mf.name.display(),
                    r.r_type,
                    r.offset
                );
            };
            out.push(fixup);
            i += taken;
        }
        out
    }

    /// An arm64 relocation's fixup, or a pair's, and the number of
    /// relocations it takes: an ADRP and the instruction that adds or
    /// loads its page offset fuse into one fixup where that instruction
    /// is the next relocation's, of the same target, works on the
    /// ADRP's register and is within 255 instructions.
    fn arm64_fixup(
        &self,
        obj: usize,
        data: &[u8],
        rels: &[Reloc],
        target: To,
        addend: i64,
    ) -> Option<(OutFixup, usize)> {
        use fk::*;
        let r = &rels[0];
        let insn = |off: u32| read32(data, off as usize);
        let pair = |r_type: u8| {
            rels.get(1).filter(|n| {
                n.r_type == r_type
                    && n.offset > r.offset
                    && n.offset - r.offset <= 0x3fc
                    && n.addend == r.addend
                    && self.reloc_target(obj, n) == self.reloc_target(obj, r)
                    && imm12_base(insn(n.offset)) == Some(insn(r.offset) & 0x1f)
            })
        };
        let mut f = OutFixup::new(r.offset, target, 0, addend);
        let mut taken = 1;
        f.kind = match r.r_type {
            ARM64_RELOC_BRANCH26 if addend != 0 => ARM64_B26_ADDEND,
            ARM64_RELOC_BRANCH26 => ARM64_B26,
            ARM64_RELOC_PAGE21 => match pair(ARM64_RELOC_PAGEOFF12) {
                Some(n) => {
                    (f.other, f.scale, taken) =
                        (((n.offset - r.offset) / 4) as u8, imm12_scale(insn(n.offset)), 2);
                    if addend != 0 { ARM64_ADRP_LO12_ADDEND } else { ARM64_ADRP_LO12 }
                }
                None if addend != 0 => ARM64_ADRP_ADDEND,
                None => ARM64_ADRP,
            },
            ARM64_RELOC_PAGEOFF12 => {
                f.scale = imm12_scale(insn(r.offset));
                if addend != 0 { ARM64_LO12_ADDEND } else { ARM64_LO12 }
            }
            ARM64_RELOC_GOT_LOAD_PAGE21 => {
                match pair(ARM64_RELOC_GOT_LOAD_PAGEOFF12).filter(|n| is_ldr_x(insn(n.offset))) {
                    Some(n) => {
                        (f.other, f.scale, taken) = (((n.offset - r.offset) / 4) as u8, 8, 2);
                        ARM64_ADRP_LDR_GOT
                    }
                    None => ARM64_ADRP_GOT,
                }
            }
            ARM64_RELOC_GOT_LOAD_PAGEOFF12 => {
                f.scale = imm12_scale(insn(r.offset));
                if is_ldr_x(insn(r.offset)) { ARM64_LD12_GOT } else { ARM64_ADD_GOT }
            }
            ARM64_RELOC_TLVP_LOAD_PAGE21 => ARM64_ADRP_TLV,
            ARM64_RELOC_TLVP_LOAD_PAGEOFF12 => {
                f.scale = imm12_scale(insn(r.offset));
                ARM64_LD12_TLV
            }
            // A 32-bit reference to a GOT slot from data, relative to
            // the field (whose contents the assembler leaves to no use).
            ARM64_RELOC_POINTER_TO_GOT if r.is_pcrel => PCREL32_TO_GOT,
            ARM64_RELOC_POINTER_TO_GOT => PTR64_TO_GOT,
            _ => return None,
        };
        Some((f, taken))
    }

    /// The initializer offsets the link made of the objects'
    /// initializer pointers: an entry of no bytes each, an image
    /// offset of the function.
    fn add_init_offsets(&mut self) {
        let ctx = self.ctx;
        if !ctx.has_chunk(crate::chunks::ChunkId::InitOffsets) {
            return;
        }
        for &func in &ctx.init_offsets.init_funcs {
            let (target, addend) = match func {
                crate::chunks::init_offsets::InitFunc::Local(isec, off) => {
                    match self.isec_target(isec as u32) {
                        Some(to) => (to, off as i64),
                        None => continue,
                    }
                }
                _ => continue,
            };
            let mut entry = OutEntry::new(0, kind::ANON, ctype::INIT_OFFSET);
            entry.size = 4;
            entry.p2align = 2;
            entry.no_dead_strip = true;
            entry.fixups.push(OutFixup::new(0, target, fk::IMAGE_OFFSET32, addend));
            self.tail.push(entry);
        }
    }

    /// The __eh_frame records the image has, in its order: a CIE, with
    /// its personality's GOT slot; an FDE, with the differences that
    /// make its CIE pointer, function and LSDA.
    fn add_cfi_entries(&mut self) {
        let ctx = self.ctx;
        if !ctx.has_chunk(crate::chunks::ChunkId::EhFrame) {
            return;
        }
        let mut records: Vec<(u32, Option<usize>, usize)> = Vec::new();
        for (i, cie) in ctx.cies.iter().enumerate() {
            if cie.is_alive {
                records.push((cie.output_offset, None, i));
            }
        }
        for (i, fde) in ctx.fdes.iter().enumerate() {
            records.push((fde.output_offset, Some(i), fde.cie as usize));
        }
        records.sort_by_key(|r| r.0);
        let mut cie_entries: HashMap<usize, u32> = HashMap::new();
        for (output_offset, fde, cie) in records {
            let me = To::Tail(self.tail.len() as u32);
            let mut entry = match fde {
                None => {
                    cie_entries.insert(cie, self.tail.len() as u32);
                    self.cie_entry(cie)
                }
                Some(fde) => {
                    let Some(&cie) = cie_entries.get(&cie) else { continue };
                    let Some(entry) = self.fde_entry(fde, me, To::Tail(cie)) else { continue };
                    entry
                }
            };
            entry.content = Content::Image(ctx.eh_frame.hdr.fileoff + output_offset as u64);
            self.tail.push(entry);
        }
    }

    fn cie_entry(&self, idx: usize) -> OutEntry {
        let cie = &self.ctx.cies[idx];
        let mut entry = OutEntry::new(scope::HIDDEN, kind::ANON, ctype::CFI);
        entry.size = cie.data.len() as u32;
        if let Some(p) = cie.personality {
            let (to, off) = self.sym_target(p);
            entry.fixups.push(OutFixup::new(cie.personality_offset, to, fk::PCREL32_TO_GOT, off));
        }
        entry
    }

    /// An FDE's entry, if its function has one.
    fn fde_entry(&self, idx: usize, me: To, cie: To) -> Option<OutEntry> {
        let ctx = self.ctx;
        let fde = &ctx.fdes[idx];
        let cie_rec = &ctx.cies[fde.cie as usize];
        let func = self.isec_target(fde.isec)?;
        let mut entry = OutEntry::new(scope::HIDDEN, kind::ANON, ctype::CFI);
        entry.size = fde.data.len() as u32;
        entry.dds_if_refs_live = true;
        let diff = |offset: u32, target: To, size: usize, addend: i64| {
            let kind = if size == 4 { fk::DIFF32 } else { fk::DIFF64 };
            OutFixup { from: Some(me), ..OutFixup::new(offset, target, kind, addend) }
        };
        entry.fixups.push(OutFixup { from: Some(cie), ..OutFixup::new(4, me, fk::DIFF32, 4) });
        let pc = fde.func_offset as i64 - 8;
        entry.fixups.push(diff(8, func, cie_rec.pc_size(), pc));
        if let Some((isec, off)) = fde.lsda
            && let Some(lsda) = self.isec_target(isec)
        {
            let pos = crate::chunks::eh_frame::lsda_pos(fde.data, cie_rec.pc_size()) as u32;
            let addend = off as i64 - pos as i64;
            entry.fixups.push(diff(pos, lsda, cie_rec.lsda_size(), addend));
        }
        Some(entry)
    }

    /// The objects' compact unwind records of the functions kept, as
    /// they had them: an entry each, its pointers fixups.
    fn add_compact_unwind_entries(&mut self) {
        let ctx = self.ctx;
        for (obj_idx, obj) in ctx.objs.iter().enumerate() {
            if !obj.is_alive || ctx.is_internal(obj_idx) {
                continue;
            }
            let sect = obj
                .sect_hdrs
                .iter()
                .position(|h| h.segname() == b"__LD" && h.sectname() == b"__compact_unwind");
            if let Some(sect) = sect {
                let entries = self.object_unwind_entries(obj, sect);
                self.tail.extend(entries);
            }
        }
    }

    fn object_unwind_entries(&self, obj: &ObjectFile, sect: usize) -> Vec<OutEntry> {
        let ctx = self.ctx;
        let hdr = &obj.sect_hdrs[sect];
        let data = obj.mf.data();
        let contents = &data[hdr.offset as usize..][..hdr.size as usize];
        let raw: Vec<MachRel> = read_array(data, hdr.reloff as usize, hdr.nreloc as usize);
        let nsyms = obj.nlists.len();
        let Ok(rels) = E::read_relocs(&obj.mf.name, &obj.sect_hdrs, hdr, contents, &raw, nsyms)
        else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (k, bytes) in contents.as_chunks::<32>().0.iter().enumerate() {
            let start = (k * 32) as u32;
            let mut entry = OutEntry::new(scope::HIDDEN, kind::ANON, CT_COMPACT_UNWIND);
            entry.size = 32;
            entry.p2align = 3;
            entry.dds_if_refs_live = true;
            entry.content = Content::Pool(bytes);
            for r in rels.iter().filter(|r| (start..start + 32).contains(&r.offset)) {
                let Some((to, addend)) = self.unwind_target(obj, r) else { continue };
                let kind = if r.size == 4 { fk::PTR32 } else { fk::PTR64 };
                entry.fixups.push(OutFixup::new(r.offset - start, to, kind, addend));
            }
            // The record of a function not kept goes with it.
            if !entry.fixups.iter().any(|f| f.offset == 0) {
                continue;
            }
            let addr = hdr.addr + start as u64;
            let label = obj.nlists.iter().zip(&obj.symbols).find(|(n, _)| {
                !n.is_stab()
                    && n.n_type() == N_SECT
                    && n.n_sect as usize == sect + 1
                    && n.n_value == addr
            });
            if let Some((_, &id)) = label {
                entry.name = Some(ctx.symbols[id].name().as_bytes());
                entry.scope = 0;
                entry.kind = kind::REGULAR;
            }
            out.push(entry);
        }
        out
    }

    /// The entry a pointer of a compact unwind record refers to, and
    /// the addend there; none for a subsection of the object not kept
    /// as an entry of its own (dead, or folded into another, whose own
    /// record stays).
    fn unwind_target(&self, obj: &ObjectFile, r: &Reloc) -> Option<(To, i64)> {
        let ctx = self.ctx;
        let local = |addr: u64| {
            let (isec, off) = crate::input_files::find_subsec(&ctx.isecs, &obj.subsecs, addr)?;
            let entry = self.isec_entry.get(&(isec as u32))?;
            Some((*entry, off as i64))
        };
        match r.target() {
            RelocTarget::Sym(idx) => {
                let n = &obj.nlists[idx as usize];
                if !n.is_stab() && n.n_type() == N_SECT {
                    let (to, off) = local(n.n_value)?;
                    return Some((to, off + r.addend));
                }
                let (to, off) = self.sym_target(obj.symbols[idx as usize]);
                Some((to, r.addend + off))
            }
            RelocTarget::Section(s) => {
                local(obj.sect_hdrs[s as usize].addr.wrapping_add_signed(r.addend))
            }
        }
    }

    /// Puts the entries in their final order - the objects', then those
    /// of the symbols they leave to the linker or import, then the
    /// linker's - and numbers the fixups' targets.
    fn finish(self) -> MergeableRecord {
        let ctx = self.ctx;
        let deps = dependencies(ctx);
        let mut sym_entries: Vec<OutEntry> = Vec::new();
        let mut sym_index: HashMap<SymbolId, usize> = HashMap::new();
        for (id, dep) in self.referenced_syms(&deps) {
            match dep {
                Some(dylib) => sym_entries.push(import_entry(ctx, id, dylib)),
                None => sym_entries.extend(undefine_entries(ctx, id)),
            }
            sym_index.insert(id, sym_entries.len() - 1);
        }
        let order = self.final_order(sym_entries.len());
        let mut index =
            [vec![0u32; self.entries.len()], vec![0; self.tail.len()], vec![0; sym_entries.len()]];
        for (i, &(list, j)) in order.iter().enumerate() {
            index[list][j] = i as u32;
        }
        let number = |to: To, me: u32| match to {
            To::Entry(a) => index[0][a as usize],
            To::Tail(t) => index[1][t as usize],
            To::Sym(id) => index[2][sym_index[&id]],
            To::Prev => me - 1,
        };
        let lists = [&self.entries, &self.tail, &sym_entries];
        let mut entries: Vec<OutEntry> =
            order.iter().map(|&(list, j)| lists[list][j].clone()).collect();
        let mut fixups = Vec::new();
        let mut first_fixup = Vec::with_capacity(entries.len());
        for (i, entry) in entries.iter_mut().enumerate() {
            first_fixup.push(fixups.len() as u32);
            entry.fixups.sort_by_key(|f| f.offset);
            for f in &entry.fixups {
                let from = f.from.map_or(0, |to| number(to, i as u32));
                fixups.push((*f, number(f.target, i as u32), from));
            }
        }
        MergeableRecord {
            entries,
            fixups,
            first_fixup,
            sections: self.sections,
            own: own_record(ctx),
            deps: deps.into_iter().map(|(_, d)| d).collect(),
            debug: self.debug,
            flags: record_flags(ctx),
        }
    }

    /// The symbols the fixups refer to by name, each with its library's
    /// index among the dependencies if it is an import: those the
    /// linker defines (or leaves to dynamic lookup) as first referred
    /// to, then the imports by library and name.
    fn referenced_syms(&self, deps: &[(i32, DylibRecord)]) -> Vec<(SymbolId, Option<u8>)> {
        let ctx = self.ctx;
        let mut syms: Vec<SymbolId> = Vec::new();
        let mut seen = hashbrown::HashSet::new();
        // The stub helper's and the objc stubs', which ld-prime records
        // too.
        for id in [ctx.stub_helper.dyld_stub_binder, ctx.objc_stubs.msgsend_sym] {
            if let Some(id) = id
                && ctx.symbols[id].is_imported()
            {
                seen.insert(id);
                syms.push(id);
            }
        }
        for f in self.entries.iter().chain(&self.tail).flat_map(|e| &e.fixups) {
            for to in [Some(f.target), f.from].into_iter().flatten() {
                if let To::Sym(id) = to
                    && seen.insert(id)
                {
                    syms.push(id);
                }
            }
        }
        let dep_index = |id: SymbolId| match ctx.symbols[id].file() {
            Some(FileId::Dylib(d)) if d != u32::MAX => {
                let ordinal = ctx.dylibs[d as usize].dylib_idx;
                deps.iter().position(|&(o, _)| o == ordinal).map(|p| p as u8)
            }
            _ => None,
        };
        let mut syms: Vec<(SymbolId, Option<u8>)> =
            syms.into_iter().map(|id| (id, dep_index(id))).collect();
        syms.sort_by_key(|&(id, dep)| match dep {
            None => (0, 0, ""),
            Some(dep) => (1, dep, ctx.symbols[id].name()),
        });
        syms
    }

    /// The entries' final order, as (list, index) pairs of the objects'
    /// entries (0), the linker's (1) and the symbols' (2): the objects'
    /// but the folded functions and the thread-local variables, the
    /// symbols', the folded functions, the thread-local variables, and
    /// the linker's.
    fn final_order(&self, nsyms: usize) -> Vec<(usize, usize)> {
        let is_tlv = |i: &usize| self.entries[*i].content_type == CT_THREAD_VARS;
        let folded: hashbrown::HashSet<usize> =
            self.folded.iter().map(|&(alias, _)| alias as usize).collect();
        let objs = 0..self.entries.len();
        let mut order: Vec<(usize, usize)> =
            (objs.clone()).filter(|i| !is_tlv(i) && !folded.contains(i)).map(|i| (0, i)).collect();
        order.extend((0..nsyms).map(|i| (2, i)));
        order.extend(self.folded.iter().map(|&(alias, _)| (0, alias as usize)));
        order.extend(objs.filter(is_tlv).map(|i| (0, i)));
        order.extend((0..self.tail.len()).map(|i| (1, i)));
        order
    }
}

/// Whether a section's subsections each get an entry in the record: not
/// the debug info, the unwind sections (whose records get entries of
/// their own) and the image info the link makes afresh.
fn has_entries(hdr: &MachSection) -> bool {
    hdr.flags & S_ATTR_DEBUG == 0
        && hdr.segname() != b"__LLVM"
        && !(hdr.segname() == b"__LD" && hdr.sectname() == b"__compact_unwind")
        && hdr.sectname() != b"__eh_frame"
        && !crate::input_files::is_objc_image_info(hdr)
}

/// Whether a section is an object's __DATA,__got, whose slots the link
/// moves into its GOT (see passes::fold_input_got).
fn is_input_got(hdr: &MachSection) -> bool {
    hdr.segname() == b"__DATA" && hdr.sectname() == b"__got"
}

/// Whether ld-prime merges a section's subsections by their contents:
/// the literals (not an object's GOT slots, which it takes one by one
/// all the same), and the UTF-16 strings.
fn merges_by_content(hdr: &MachSection) -> bool {
    crate::input_files::is_literal_section(hdr) && !is_input_got(hdr)
        || (hdr.segname() == b"__TEXT" && hdr.sectname() == b"__ustring")
}

/// The size of the records of a section that ld-prime takes one by
/// one, each an entry (as mold takes literals): the Objective-C pointer
/// lists and references, and the CFStrings.
fn record_size(hdr: &MachSection) -> Option<u32> {
    if !hdr.segname().starts_with(b"__DATA") {
        return None;
    }
    match hdr.sectname() {
        b"__objc_classlist" | b"__objc_nlclslist" | b"__objc_catlist" | b"__objc_catlist2"
        | b"__objc_nlcatlist" | b"__objc_protolist" | b"__objc_classrefs" | b"__objc_superrefs"
        | b"__objc_protorefs" | b"__objc_selrefs" => Some(8),
        b"__cfstring" => Some(32),
        _ => None,
    }
}

/// The bytes of the record at `off` of a subsection's.
fn record_content(content: Content, off: u32, size: u32) -> Content {
    match content {
        Content::None => Content::None,
        Content::Image(fileoff) => Content::Image(fileoff + off as u64),
        Content::Pool(bytes) => Content::Pool(&bytes[off as usize..(off + size) as usize]),
    }
}

/// The content type of the standard section a section is, by name and
/// type (see mergeable::standard_section).
fn standard_content_type(hdr: &MachSection) -> Option<u8> {
    (0..82).find(|&ct| {
        ct != ctype::INIT_OFFSET
            && crate::mergeable::standard_section(ct).is_some_and(|(seg, sect, flags)| {
                hdr.segname() == seg
                    && hdr.sectname() == sect
                    && hdr.section_type() == flags & SECTION_TYPE
            })
    })
}

/// The scope and kind of the entry a symbol of an object names: local
/// to its object, hidden, hidden unless something takes its address
/// (a weak definition that can be hidden), or global; a weak definition
/// only where it is not hidden.
fn linkage<E: Target>(ctx: &Context<E>, nlist: &NList, id: SymbolId) -> (u8, u8) {
    if !nlist.is_extern() {
        return (0, kind::REGULAR);
    }
    let sym = &ctx.symbols[id];
    let weak = nlist.n_desc & N_WEAK_DEF != 0;
    let scope = if nlist.n_type & N_PEXT != 0 {
        scope::HIDDEN
    } else if weak && nlist.n_desc & N_WEAK_REF != 0 {
        scope::AUTO_HIDE
    } else if sym.is_private_extern() {
        scope::HIDDEN
    } else if nlist.n_desc & REFERENCED_DYNAMICALLY != 0 {
        scope::NEVER_STRIP
    } else {
        scope::GLOBAL
    };
    let kind = if nlist.n_desc & N_SYMBOL_RESOLVER != 0 {
        kind::RESOLVER
    } else if weak && scope != scope::HIDDEN {
        kind::WEAK_DEF
    } else {
        kind::REGULAR
    };
    (scope, kind)
}

/// The entry a symbol of an object names: its linkage, its name, and
/// its debug notes if a final link would note it.
fn named_entry<E: Target>(
    ctx: &Context<E>,
    obj: &ObjectFile,
    i: usize,
    hdr: &MachSection,
    content_type: u8,
    debug: u16,
) -> OutEntry {
    let (scope, kind) = linkage(ctx, &obj.nlists[i], obj.symbols[i]);
    let name = ctx.symbols[obj.symbols[i]].name();
    let mut entry = OutEntry::new(scope, kind, content_type);
    entry.name = Some(name.as_bytes());
    entry.cold = obj.nlists[i].n_desc & N_COLD_FUNC != 0;
    entry.no_dead_strip = obj.nlists[i].n_desc & N_NO_DEAD_STRIP != 0;
    if !crate::input_files::is_private_label(name) && crate::chunks::symtab::has_stabs(hdr) {
        entry.debug = debug;
    }
    entry
}

/// The entries of a symbol the objects refer to that no object defines:
/// one the linker defines, or one left to dynamic lookup.
/// ___dso_handle is ld-prime's alias of the image's start.
fn undefine_entries<E: Target>(ctx: &Context<E>, id: SymbolId) -> Vec<OutEntry> {
    let sym = &ctx.symbols[id];
    let kind = if sym.is_weak_ref() && !sym.is_defined() {
        kind::UNDEFINE_WEAK_IMPORT
    } else {
        kind::UNDEFINE
    };
    let mut entry = OutEntry::new(scope::GLOBAL, kind, CT_NONE);
    if sym.name() == "___dso_handle" {
        entry.name = Some(b"segment$start$__TEXT");
        let mut alias = OutEntry::new(scope::HIDDEN, kind::ALIAS, CT_NONE);
        alias.name = Some(b"___dso_handle");
        alias.fixups.push(OutFixup::new(0, To::Prev, fk::ALIAS_OF, 0));
        return vec![entry, alias];
    }
    entry.name = Some(sym.name().as_bytes());
    vec![entry]
}

/// An import's entry: the library's export, weak if it defines it so,
/// and how strongly the image refers to it.
fn import_entry<E: Target>(ctx: &Context<E>, id: SymbolId, dylib: u8) -> OutEntry {
    let sym = &ctx.symbols[id];
    let Some(FileId::Dylib(d)) = sym.file() else { unreachable!() };
    let weak_def = ctx.dylibs[d as usize].weak_exports.contains(sym.name());
    let kind = if weak_def { kind::DYLIB_EXPORT_WEAK_DEF } else { kind::DYLIB_EXPORT };
    let mut entry = OutEntry::new(scope::GLOBAL, kind, CT_NONE);
    entry.name = Some(sym.name().as_bytes());
    entry.import = if sym.is_weak_ref() { 1 } else { 2 };
    entry.dylib = Some(dylib);
    entry
}

/// The dylibs the image loads, in the order of their load commands,
/// each by its ordinal and as the record keeps it.
fn dependencies<E: Target>(ctx: &Context<E>) -> Vec<(i32, DylibRecord)> {
    let mut dylibs: Vec<&crate::input_files::DylibFile> =
        ctx.dylibs.iter().filter(|d| !d.is_bundle_loader && !d.is_lazy).collect();
    dylibs.sort_by_key(|d| d.dylib_idx);
    dylibs
        .into_iter()
        .map(|d| {
            let record = DylibRecord {
                install_name: d.install_name.clone(),
                current_version: d.current_version,
                compatibility_version: d.compatibility_version,
                ..Default::default()
            };
            (d.dylib_idx, record)
        })
        .collect()
}

/// The dylib's own identity: its install name and versions, its
/// platform, the dylibs it re-exports and the clients it allows.
fn own_record<E: Target>(ctx: &Context<E>) -> DylibRecord {
    let mut reexports: Vec<&crate::input_files::DylibFile> =
        ctx.dylibs.iter().filter(|d| d.is_reexported).collect();
    reexports.sort_by_key(|d| d.dylib_idx);
    DylibRecord {
        install_name: ctx.args.output_install_name().to_vec(),
        current_version: ctx.args.current_version,
        compatibility_version: ctx.args.compatibility_version,
        platforms: vec![ctx.args.platform],
        reexports: reexports.iter().map(|d| d.install_name.clone()).collect(),
        clients: ctx.args.allowable_clients.clone(),
    }
}

/// The record's flags word: whether every input was split into
/// subsections by its symbols (bit 24: every object has
/// MH_SUBSECTIONS_VIA_SYMBOLS, and no dylib was read from a Mach-O
/// file, which never does); whether an object has Swift metadata (a
/// __TEXT,__swift* section) or Objective-C classes or categories (bit
/// 30, image info or not); and what the Objective-C image info says:
/// that there was one (26), with the Swift versions (bits 0-23) and
/// category class properties (29), and classes (31).
fn record_flags<E: Target>(ctx: &Context<E>) -> u64 {
    use crate::mergeable::{
        FLAG_CATEGORY_CLASS_PROPERTIES, FLAG_HAS_CLASSES, FLAG_HAS_OBJC_INFO,
        FLAG_HAS_SWIFT_OR_OBJC,
    };
    let live_objs = || {
        ctx.objs
            .iter()
            .enumerate()
            .filter(|(i, o)| o.is_alive && !ctx.is_internal(*i))
            .map(|(_, o)| o)
    };
    let has_section = |names: &[&[u8]]| {
        live_objs().any(|o| o.sect_hdrs.iter().any(|h| h.size > 0 && names.contains(&h.sectname())))
    };
    let mut flags = 0u64;
    if live_objs().all(|o| o.subsections_via_symbols) && !ctx.dylibs.iter().any(|d| d.from_binary) {
        flags |= 1 << 24;
    }
    let swift = live_objs().any(|o| {
        o.sect_hdrs.iter().any(|h| h.segname() == b"__TEXT" && h.sectname().starts_with(b"__swift"))
    });
    let has_classes = has_section(&[b"__objc_classlist", b"__objc_nlclslist"]);
    if swift || has_classes || has_section(&[b"__objc_catlist", b"__objc_nlcatlist"]) {
        flags |= FLAG_HAS_SWIFT_OR_OBJC;
    }
    if !ctx.has_chunk(crate::chunks::ChunkId::ObjcImageInfo) {
        return flags;
    }
    let info = ctx.objc_imageinfo.flags as u64;
    flags |= FLAG_HAS_OBJC_INFO;
    flags |= (info >> 8) & 0xff | ((info >> 16) & 0xffff) << 8;
    if info & 0x40 != 0 {
        flags |= FLAG_CATEGORY_CLASS_PROPERTIES;
    }
    if has_classes {
        flags |= FLAG_HAS_CLASSES;
    }
    flags
}

fn is_subtractor<E: Target>(r_type: u8) -> bool {
    if E::CPUTYPE == CPU_TYPE_ARM64 {
        r_type == ARM64_RELOC_SUBTRACTOR
    } else {
        r_type == X86_64_RELOC_SUBTRACTOR
    }
}

/// An x86-64 relocation's fixup. ld-prime's addend is to the
/// target, the instruction's distance to the field's end aside (see
/// target::x86_64's reloc_bias).
fn x86_64_fixup(hdr: &MachSection, r: &Reloc, target: To, addend: i64) -> Option<OutFixup> {
    use fk::*;
    let mut f = OutFixup::new(r.offset, target, 0, addend);
    f.kind = match r.r_type {
        X86_64_RELOC_BRANCH if r.size == 1 => X86_64_BRANCH8,
        X86_64_RELOC_BRANCH => X86_64_CALL,
        X86_64_RELOC_SIGNED => X86_64_RIP,
        X86_64_RELOC_SIGNED_1 => X86_64_RIP1,
        X86_64_RELOC_SIGNED_2 => X86_64_RIP2,
        X86_64_RELOC_SIGNED_4 => X86_64_RIP4,
        X86_64_RELOC_GOT_LOAD => X86_64_RIP_GOT_LOAD,
        X86_64_RELOC_TLV => X86_64_RIP_TLV_LOAD,
        X86_64_RELOC_GOT if hdr.flags & S_ATTR_SOME_INSTRUCTIONS != 0 => X86_64_RIP_GOT,
        // From data, relative to the field's start: Swift's has the
        // low bit set besides.
        X86_64_RELOC_GOT => {
            f.addend -= 4;
            if f.addend & 1 != 0 {
                f.addend -= 1;
                SWIFT_REL32_TO_GOT
            } else {
                PCREL32_TO_GOT
            }
        }
        _ => return None,
    };
    Some(f)
}

fn read32(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(data[off..off + 4].try_into().unwrap())
}

/// The base register of an instruction that adds a page offset (an
/// add, a load or a store of an unsigned 12-bit immediate).
fn imm12_base(insn: u32) -> Option<u32> {
    let is_add = insn & 0x7f80_0000 == 0x1100_0000;
    let is_ldst = insn & 0x3b00_0000 == 0x3900_0000;
    (is_add || is_ldst).then_some((insn >> 5) & 0x1f)
}

/// How many bytes such an instruction moves, by which it scales its
/// offset: 1 for an add.
fn imm12_scale(insn: u32) -> u8 {
    if insn & 0x3b00_0000 != 0x3900_0000 {
        return 1;
    }
    let size = insn >> 30;
    let simd = insn >> 26 & 1 != 0;
    if simd && size == 0 && insn >> 23 & 1 != 0 { 16 } else { 1 << size }
}

/// A 64-bit load of an unsigned 12-bit offset: what loads a GOT slot.
fn is_ldr_x(insn: u32) -> bool {
    insn & 0xffc0_0000 == 0xf940_0000
}
