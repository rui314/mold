//! -make_mergeable: the LC_ATOM_INFO record of a dylib's contents, which
//! lets a later link's -merge_* take the dylib as the objects it was
//! made of (the mergeable module reads the record; see there for its
//! format).
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
//! The entries come object by object, then those of the names the
//! objects import or leave to the linker, then what the linker made.
//! Fixups refer to entries by index; their order means nothing else.

use hashbrown::HashMap;

use crate::arch::Target;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::error::RawPath;
use crate::fatal;
use crate::input_files::{FileId, ObjectFile};
use crate::input_sections::{InputSection, NO_REPLACEMENT, Reloc, RelocTarget};
use crate::macho::*;
use crate::mergeable::{
    CustomSection, Entry, Fixup, ctype, fk, header, kind, scope, standard_content_type,
};
use crate::symbol::SymbolId;

mod objc;
mod write;

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
        let mut hdr = ChunkHeader::linkedit();
        hdr.p2align = 3;
        Self { hdr, contents: Vec::new(), image_contents: Vec::new(), pool_offset: 0 }
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
    let image = header::IMAGE;
    buf[image..image + 4].copy_from_slice(&(-(sec.hdr.fileoff as i32)).to_le_bytes());
    buf[image + 4..image + 8].copy_from_slice(&(ctx.output_size as u32).to_le_bytes());
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
}

/// A fixup as the writer makes it, of entries not yet numbered.
type OutFixup = Fixup<To>;

impl OutFixup {
    fn new(offset: u32, target: To, kind: u16, addend: i64) -> Self {
        Self { offset, target, kind, addend, from: None, scale: 0, second: 0 }
    }
}

/// An entry as the writer makes it: where its bytes are, and its fixups.
type OutEntry = Entry<Content, Vec<OutFixup>>;

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
    /// The object's CPU subtype, as N_OSO's sect has it.
    cpusubtype: u8,
    source_dir: Vec<u8>,
    source_name: Vec<u8>,
    object_path: Vec<u8>,
    install_name: Vec<u8>,
}

/// The entries and tables, in their final order.
struct MergeableRecord {
    entries: Vec<OutEntry>,
    /// The fixups, of the entries numbered, each entry's in a run.
    fixups: Vec<Fixup>,
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
    /// The class of each stand-in for a class reference slot the GOT
    /// took over (see objc::add_classref_stand_ins).
    stand_ins: HashMap<u32, SymbolId>,
    sections: Vec<CustomSection>,
    debug: Vec<DebugRecord>,
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
            stand_ins: ctx.got.stand_ins.iter().copied().collect(),
            sections: Vec::new(),
            debug: Vec::new(),
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

    /// The objects' entries: object by object, the live subsections,
    /// each followed by the aliases of the other symbols in it, and then
    /// the absolute symbols and tentative definitions the object made.
    fn add_object_entries(&mut self) {
        let ctx = self.ctx;
        for (obj_idx, obj) in ctx.objs.iter().enumerate() {
            if !obj.is_alive || ctx.is_internal(obj_idx) {
                continue;
            }
            let debug = self.add_debug_record(obj);
            for &id in &obj.subsecs {
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
            mtime: oso.ent.value as u32,
            cpusubtype: oso.ent.sect,
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
        let hdr = isec.hdr(obj);
        if isec.replacement != NO_REPLACEMENT {
            return self.add_folded_function(obj, id, debug);
        }
        if !has_entries(hdr) {
            return;
        }
        // A list record's labels name nothing, nor do a literal's but a
        // symbol's of its own (see InputSection::label); neither has
        // aliases.
        let literal = merges_by_content(hdr);
        let record = crate::input_files::is_record_list(hdr, obj.subsections_via_symbols);
        let label = if record { None } else { isec.label_index(ctx) };
        let (content_type, custom) = self.content_type(hdr);
        let mut entry = match label {
            Some(i) => named_entry(ctx, obj, i, content_type, debug),
            None if literal => {
                OutEntry::new(scope::HIDDEN, kind::ANON_COAL_BY_CONTENT, content_type)
            }
            None => OutEntry::new(scope::LOCAL, kind::ANON, content_type),
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
        entry.content = self.isec_content(isec, hdr);
        let entry_idx = self.push_records(entry, id, record_size(hdr));
        self.isec_entry.insert(id, To::Entry(entry_idx));
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
    /// aliases of that alias; its target is filled in once every entry
    /// is made.
    fn add_folded_function(&mut self, obj: &ObjectFile, id: u32, debug: u16) {
        let ctx = self.ctx;
        let Some(label) = ctx.isecs[id as usize].label_index(ctx) else { return };
        let sym_id = obj.symbols[label];
        // (A losing copy of a weak definition, whose symbol is the
        // winner's, which may be the folded one, has no entry at all.)
        if !ctx.folded_subsec_names.contains(&sym_id)
            || ctx.symbols[sym_id].input_section() != Some(id)
        {
            return;
        }
        let (scope, kind) = linkage(ctx, &obj.mach_syms[label], sym_id);
        let kind = if kind == kind::WEAK_DEF { kind::WEAK_DEF_ALIAS } else { kind::ALIAS };
        let mut alias = OutEntry::new(scope, kind, ctype::NONE);
        alias.name = Some(ctx.symbols[sym_id].name());
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
        let syms = (0..obj.mach_syms.len()).filter(|&i| {
            let n = &obj.mach_syms[i];
            Some(i) != label
                && !n.is_stab()
                && n.ty() == N_SECT
                && n.sect as u32 == isec.shndx + 1
                && (start..end.max(start + 1)).contains(&n.value)
                && !crate::input_files::is_private_label(ctx.symbols[obj.symbols[i]].name())
        });
        for i in syms {
            let sym_id = obj.symbols[i];
            let sym = &ctx.symbols[sym_id];
            if sym.input_section() != Some(id) {
                continue;
            }
            let (scope, kind) = linkage(ctx, &obj.mach_syms[i], sym_id);
            let kind = if kind == kind::WEAK_DEF { kind::WEAK_DEF_ALIAS } else { kind::ALIAS };
            let mut alias = OutEntry::new(scope, kind, ctype::NONE);
            alias.name = Some(sym.name());
            alias.dds_if_refs_live = true;
            alias.no_dead_strip = obj.mach_syms[i].desc & N_NO_DEAD_STRIP != 0;
            alias.debug = debug;
            let offset = (obj.mach_syms[i].value - start) as i64;
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
        for (n, &sym_id) in obj.mach_syms.iter().zip(&obj.symbols) {
            let sym = &ctx.symbols[sym_id];
            if n.is_stab()
                || n.ty() != N_ABS
                || sym.file() != Some(FileId::Obj(obj_idx as u32))
                || self.sym_entry.contains_key(&sym_id)
            {
                continue;
            }
            let (scope, _) = linkage(ctx, n, sym_id);
            let mut entry = OutEntry::new(scope, kind::ABSOLUTE, ctype::DATA);
            entry.name = Some(sym.name());
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
        for (n, &sym_id) in obj.mach_syms.iter().zip(&obj.symbols) {
            if n.is_stab() || !n.is_extern() || n.ty() != N_UNDF || !n.is_common() {
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
            let mut entry = OutEntry::new(scope, kind::TENTATIVE_DEF, ctype::COMMON);
            entry.name = Some(sym.name());
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
        (ctype::CUSTOM, Some(idx as u8))
    }

    /// The fixups of the objects' entries, from their relocations: each
    /// subsection's, from the first of its entries (see place_fixups).
    fn add_object_fixups(&mut self) {
        for i in 0..self.entries.len() {
            let Some(id) = self.entry_isec[i] else { continue };
            if i > 0 && self.entry_isec[i - 1] == Some(id) {
                continue;
            }
            let fixups = self.isec_fixups(id as usize);
            for (k, f) in self.place_fixups(id, fixups) {
                self.entries[i + k].fixups.push(f);
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
        self.isec_entry.get(&(self.ctx.isecs.resolve(isec as usize) as u32)).copied()
    }

    /// The entry holding offset `off` of a subsection (of the one that
    /// replaced it), and the offset there.
    fn isec_target_at(&self, isec: u32, off: i64) -> Option<(To, i64)> {
        let isec = self.ctx.isecs.resolve(isec as usize) as u32;
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

    /// The stand-in for a class reference slot the GOT took over (see
    /// objc::add_classref_stand_ins) that a relocation refers to, and
    /// the offset there.
    fn class_ref(&self, obj: usize, r: &Reloc) -> Option<(u32, i64)> {
        if self.stand_ins.is_empty() {
            return None;
        }
        let ctx = self.ctx;
        let (slot, off) = match r.target() {
            RelocTarget::Sym(idx) => {
                let sym = &ctx.symbols[ctx.objs[obj].symbols[idx as usize]];
                (sym.input_section()?, sym.value as i64 + r.addend)
            }
            RelocTarget::Section(slot) => (slot, r.addend),
        };
        let stand_in = ctx.isecs.resolve(slot as usize) as u32;
        self.stand_ins.contains_key(&stand_in).then_some((stand_in, off))
    }

    /// The fixup of `rels[i]`, a reference to a class reference slot the
    /// GOT took over, as ld-prime records it: a GOT reference to the
    /// class, with no entry for the slot. An adrp and the add after it
    /// (see objc::pair_classref_uses) take the address of the class's
    /// entry together, in one fixup (the add's own would be a load of
    /// the class relaxed); a load reads the class from it. ld-prime
    /// refuses a reference into the slot or by any other relocation,
    /// and so does this.
    fn class_ref_fixup(
        &self,
        id: usize,
        rels: &[Reloc],
        i: usize,
        (stand_in, off): (u32, i64),
    ) -> Option<OutFixup> {
        let ctx = self.ctx;
        let isec = &ctx.isecs[id];
        let obj = isec.file as usize;
        let r = &rels[i];
        let arm64 = E::CPUTYPE == CPU_TYPE_ARM64;
        let code = isec.hdr(&ctx.objs[obj]).flags & S_ATTR_SOME_INSTRUCTIONS != 0;
        let insn = |r: &Reloc| {
            u32::from_le_bytes(isec.data()[r.offset as usize..][..4].try_into().unwrap())
        };
        // The other references to the slot in the subsection, after or
        // before this one.
        let same = |r: &&Reloc| self.class_ref(obj, r).is_some_and(|(s, _)| s == stand_in);
        let next = rels[i + 1..].iter().find(same);
        let prev = rels[..i].iter().rev().find(same);
        let (kind, scale, second) = match r.ty {
            _ if off != 0 || r.is_subtracted => (0, 0, 0),
            0 if r.size == 8 && !r.is_pcrel => (fk::PTR64_TO_GOT, 0, 0),
            ARM64_RELOC_PAGE21 if arm64 => match next {
                Some(n) if n.ty == ARM64_RELOC_PAGEOFF12 && is_add_x(insn(n)) => {
                    match n.offset.checked_sub(r.offset).and_then(|d| u8::try_from(d / 4).ok()) {
                        Some(second) => (fk::ARM64_ADRP_ADD_GOT, 1, second),
                        None => (0, 0, 0),
                    }
                }
                _ => (fk::ARM64_ADRP_GOT, 0, 0),
            },
            ARM64_RELOC_PAGEOFF12 if arm64 && is_ldr_x(insn(r)) => (fk::ARM64_LD12_GOT, 8, 0),
            ARM64_RELOC_PAGEOFF12 if arm64 && is_add_x(insn(r)) => {
                if prev.is_some_and(|p| p.ty == ARM64_RELOC_PAGE21) {
                    return None;
                }
                (0, 0, 0)
            }
            X86_64_RELOC_SIGNED if !arm64 && code => (fk::X86_64_RIP_GOT, 0, 0),
            _ => (0, 0, 0),
        };
        if kind == 0 {
            fatal!(
                "{}: -make_mergeable: unsupported reference to a class reference at 0x{:x}",
                ctx.objs[obj].mf.name.raw(),
                r.offset
            );
        }
        let (to, addend) = self.sym_target(self.stand_ins[&stand_in]);
        let mut fixup = OutFixup::new(r.offset, to, kind, addend);
        fixup.scale = scale;
        fixup.second = second;
        Some(fixup)
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
                    self.ctx.objs[obj].mf.name.raw()
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
        if !is_input_got(slot.hdr(&ctx.objs[slot.file as usize]))
            || (0..slot.size as i64).contains(&off)
        {
            return None;
        }
        let addr = u64::try_from(slot.input_addr as i64 + off).ok()?;
        let (other, other_off) = ctx.objs[obj as usize].find_subsec(&ctx.isecs, addr)?;
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
        let file = &ctx.objs[obj];
        let rels = isec.rels(file);
        let hdr = isec.hdr(file);
        let mut out = Vec::with_capacity(rels.len());
        let mut i = 0;
        while i < rels.len() {
            let r = &rels[i];
            if let Some(class_ref) = self.class_ref(obj, r) {
                out.extend(self.class_ref_fixup(id, rels, i, class_ref));
                i += 1;
                continue;
            }
            let (target, off) = self.reloc_target(obj, r);
            let addend = r.addend + off;
            // UNSIGNED, which both targets number 0: a pointer, or a
            // thread-local variable's offset in the template.
            if r.ty == 0 {
                let kind = if r.size == 4 {
                    fk::PTR32
                } else if r.refers_to_tls(ctx, file) {
                    fk::TLV_OFFSET
                } else {
                    fk::PTR64
                };
                out.push(OutFixup::new(r.offset, target, kind, addend));
                i += 1;
                continue;
            }
            // A SUBTRACTOR and the UNSIGNED of its size after it.
            if is_subtractor::<E>(r.ty) && i + 1 < rels.len() {
                let (to, to_off) = self.reloc_target(obj, &rels[i + 1]);
                let kind = if rels[i + 1].size == 8 { fk::DIFF64 } else { fk::DIFF32 };
                let mut f = OutFixup::new(r.offset, to, kind, rels[i + 1].addend + to_off - off);
                f.from = Some(target);
                out.push(f);
                i += 2;
                continue;
            }
            let fixup = if E::CPUTYPE == CPU_TYPE_ARM64 {
                arm64_fixup(isec.data(), r, target, addend)
            } else {
                x86_64_fixup(hdr, r, target, addend)
            };
            let Some(fixup) = fixup else {
                fatal!(
                    "{}: -make_mergeable: unsupported relocation type {} at 0x{:x}",
                    ctx.objs[obj].mf.name.raw(),
                    r.ty,
                    r.offset
                );
            };
            out.push(fixup);
            i += 1;
        }
        out
    }

    /// The initializer offsets the link made of the objects'
    /// initializer pointers: an entry of no bytes each, an image
    /// offset of the function.
    fn add_init_offsets(&mut self) {
        let ctx = self.ctx;
        if !ctx.chunks.contains(&crate::chunks::ChunkId::InitOffsets) {
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
            let mut entry = OutEntry::new(scope::LOCAL, kind::ANON, ctype::INIT_OFFSET);
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
        if !ctx.chunks.contains(&crate::chunks::ChunkId::EhFrame) {
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
            let pos = fde.lsda_pos(cie_rec.pc_size()) as u32;
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

    /// The entries of an object's compact unwind records, hidden and
    /// anonymous.
    fn object_unwind_entries(&self, obj: &ObjectFile, sect: usize) -> Vec<OutEntry> {
        let hdr = &obj.sect_hdrs[sect];
        let data = obj.mf.data();
        let contents = &data[hdr.offset as usize..][..hdr.size as usize];
        let raw: Vec<MachRel> = read_array(data, hdr.reloff as usize, hdr.nreloc as usize);
        let rels = E::read_relocs(&obj.mf.name, &obj.sect_hdrs, hdr, contents, &raw);
        let mut out = Vec::new();
        for (k, bytes) in contents.as_chunks::<32>().0.iter().enumerate() {
            let start = (k * 32) as u32;
            let mut entry = OutEntry::new(scope::HIDDEN, kind::ANON, ctype::COMPACT_UNWIND);
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
            if entry.fixups.iter().any(|f| f.offset == 0) {
                out.push(entry);
            }
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
            let (isec, off) = obj.find_subsec(&ctx.isecs, addr)?;
            let entry = self.isec_entry.get(&(isec as u32))?;
            Some((*entry, off as i64))
        };
        match r.target() {
            RelocTarget::Sym(idx) => {
                let n = &obj.mach_syms[idx as usize];
                if !n.is_stab() && n.ty() == N_SECT {
                    let (to, off) = local(n.value)?;
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

    /// Numbers the entries - the objects', then those of the symbols
    /// they import or leave to the linker, then the linker's - and the
    /// fixups' targets.
    fn finish(self) -> MergeableRecord {
        let ctx = self.ctx;
        let deps = dependencies(ctx);
        let mut sym_entries: Vec<OutEntry> = Vec::new();
        let mut sym_index: HashMap<SymbolId, u32> = HashMap::new();
        for (id, dep) in self.referenced_syms(&deps) {
            sym_index.insert(id, sym_entries.len() as u32);
            sym_entries.push(match dep {
                Some(dylib) => import_entry(ctx, id, dylib),
                None => undefine_entry(ctx, id),
            });
        }
        let nobjs = self.entries.len() as u32;
        let nsyms = sym_entries.len() as u32;
        let number = |to: To| match to {
            To::Entry(i) => i,
            To::Sym(id) => nobjs + sym_index[&id],
            To::Tail(i) => nobjs + nsyms + i,
        };
        let mut entries: Vec<OutEntry> =
            self.entries.into_iter().chain(sym_entries).chain(self.tail).collect();
        let mut fixups = Vec::new();
        let mut first_fixup = Vec::with_capacity(entries.len());
        for entry in &mut entries {
            first_fixup.push(fixups.len() as u32);
            entry.fixups.sort_by_key(|f| f.offset);
            for f in &entry.fixups {
                let Fixup { offset, kind, addend, scale, second, .. } = *f;
                let (target, from) = (number(f.target), f.from.map(number));
                fixups.push(Fixup { offset, target, kind, addend, from, scale, second });
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

    /// The symbols the fixups refer to by name, as first referred to,
    /// each with its library's index among the dependencies if it is an
    /// import.
    fn referenced_syms(&self, deps: &[(i32, DylibRecord)]) -> Vec<(SymbolId, Option<u8>)> {
        let ctx = self.ctx;
        let mut syms: Vec<SymbolId> = Vec::new();
        let mut seen = hashbrown::HashSet::new();
        // The imports the stub helper and the objc stubs call, which no
        // fixup names: ld-prime's merging link makes objc stubs of its
        // own, which call _objc_msgSend.
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
        syms.into_iter().map(|id| (id, dep_index(id))).collect()
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

/// Whether a section is an object's __DATA,__got.
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

/// The scope and kind of the entry a symbol of an object names: local
/// to its object, hidden, hidden unless something takes its address
/// (a weak definition that can be hidden), or global; a weak definition
/// only where it is not hidden.
fn linkage<E: Target>(ctx: &Context<E>, msym: &MachSym, id: SymbolId) -> (u8, u8) {
    if !msym.is_extern() {
        return (scope::LOCAL, kind::REGULAR);
    }
    let sym = &ctx.symbols[id];
    let weak = msym.desc & N_WEAK_DEF != 0;
    let scope = if msym.n_type & N_PEXT != 0 {
        scope::HIDDEN
    } else if weak && msym.desc & N_WEAK_REF != 0 {
        scope::AUTO_HIDE
    } else if sym.is_private_extern() {
        scope::HIDDEN
    } else if msym.desc & REFERENCED_DYNAMICALLY != 0 {
        scope::NEVER_STRIP
    } else {
        scope::GLOBAL
    };
    let kind = if msym.desc & N_SYMBOL_RESOLVER != 0 {
        kind::RESOLVER
    } else if weak && scope != scope::HIDDEN {
        kind::WEAK_DEF
    } else {
        kind::REGULAR
    };
    (scope, kind)
}

/// The entry a symbol of an object names: its linkage, its name, and
/// its object's debug notes, unless the assembler made it.
fn named_entry<E: Target>(
    ctx: &Context<E>,
    obj: &ObjectFile,
    i: usize,
    content_type: u8,
    debug: u16,
) -> OutEntry {
    let (scope, kind) = linkage(ctx, &obj.mach_syms[i], obj.symbols[i]);
    let name = ctx.symbols[obj.symbols[i]].name();
    let mut entry = OutEntry::new(scope, kind, content_type);
    entry.name = Some(name);
    entry.cold = obj.mach_syms[i].desc & N_COLD_FUNC != 0;
    entry.no_dead_strip = obj.mach_syms[i].desc & N_NO_DEAD_STRIP != 0;
    if !crate::input_files::is_private_label(name) {
        entry.debug = debug;
    }
    entry
}

/// The entry of a symbol the objects refer to that no object defines:
/// one the linker defines, or one left to dynamic lookup.
fn undefine_entry<E: Target>(ctx: &Context<E>, id: SymbolId) -> OutEntry {
    let sym = &ctx.symbols[id];
    let kind = if sym.is_weak_ref() && !sym.is_defined() {
        kind::UNDEFINE_WEAK_IMPORT
    } else {
        kind::UNDEFINE
    };
    let mut entry = OutEntry::new(scope::GLOBAL, kind, ctype::NONE);
    entry.name = Some(sym.name());
    entry
}

/// An import's entry: the library's export, weak if it defines it so,
/// and how strongly the image refers to it.
fn import_entry<E: Target>(ctx: &Context<E>, id: SymbolId, dylib: u8) -> OutEntry {
    let sym = &ctx.symbols[id];
    let Some(FileId::Dylib(d)) = sym.file() else { unreachable!() };
    let weak_def = ctx.dylibs[d as usize].weak_exports.contains(sym.name());
    let kind = if weak_def { kind::DYLIB_EXPORT_WEAK_DEF } else { kind::DYLIB_EXPORT };
    let mut entry = OutEntry::new(scope::GLOBAL, kind, ctype::NONE);
    entry.name = Some(sym.name());
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

/// The record's flags word: whether every object has
/// MH_SUBSECTIONS_VIA_SYMBOLS (bit 24), and what the Objective-C image
/// info says: that there was one (26), with the Swift versions (bits
/// 0-23) and category class properties (29), and classes (31).
fn record_flags<E: Target>(ctx: &Context<E>) -> u64 {
    use crate::mergeable::{FLAG_CATEGORY_CLASS_PROPERTIES, FLAG_HAS_CLASSES, FLAG_HAS_OBJC_INFO};
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
    if live_objs().all(|o| o.subsections_via_symbols) {
        flags |= 1 << 24;
    }
    if !ctx.chunks.contains(&crate::chunks::ChunkId::ObjcImageInfo) {
        return flags;
    }
    let info = ctx.objc_imageinfo.flags as u64;
    flags |= FLAG_HAS_OBJC_INFO;
    flags |= (info >> 8) & 0xff | ((info >> 16) & 0xffff) << 8;
    if info & 0x40 != 0 {
        flags |= FLAG_CATEGORY_CLASS_PROPERTIES;
    }
    if has_section(&[b"__objc_classlist", b"__objc_nlclslist"]) {
        flags |= FLAG_HAS_CLASSES;
    }
    flags
}

fn is_subtractor<E: Target>(ty: u8) -> bool {
    if E::CPUTYPE == CPU_TYPE_ARM64 {
        ty == ARM64_RELOC_SUBTRACTOR
    } else {
        ty == X86_64_RELOC_SUBTRACTOR
    }
}

/// An x86-64 relocation's fixup. ld-prime's addend is to the
/// target, the instruction's distance to the field's end aside (see
/// arch::x86_64's reloc_bias).
fn x86_64_fixup(hdr: &MachSection, r: &Reloc, target: To, addend: i64) -> Option<OutFixup> {
    use fk::*;
    let mut f = OutFixup::new(r.offset, target, 0, addend);
    f.kind = match r.ty {
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

/// An arm64 relocation's fixup, each instruction's of its own: ld-prime
/// also has kinds for an ADRP and the instruction that adds or loads its
/// page offset together, which it writes for the pairs of its objects.
fn arm64_fixup(data: &[u8], r: &Reloc, target: To, addend: i64) -> Option<OutFixup> {
    use fk::*;
    let insn = u32::from_le_bytes(data[r.offset as usize..][..4].try_into().unwrap());
    let mut f = OutFixup::new(r.offset, target, 0, addend);
    f.kind = match r.ty {
        ARM64_RELOC_BRANCH26 if addend != 0 => ARM64_B26_ADDEND,
        ARM64_RELOC_BRANCH26 => ARM64_B26,
        ARM64_RELOC_PAGE21 if addend != 0 => ARM64_ADRP_ADDEND,
        ARM64_RELOC_PAGE21 => ARM64_ADRP,
        ARM64_RELOC_PAGEOFF12 => {
            f.scale = imm12_scale(insn);
            if addend != 0 { ARM64_LO12_ADDEND } else { ARM64_LO12 }
        }
        ARM64_RELOC_GOT_LOAD_PAGE21 => ARM64_ADRP_GOT,
        ARM64_RELOC_GOT_LOAD_PAGEOFF12 => {
            f.scale = imm12_scale(insn);
            if is_ldr_x(insn) { ARM64_LD12_GOT } else { ARM64_ADD_GOT }
        }
        ARM64_RELOC_TLVP_LOAD_PAGE21 => ARM64_ADRP_TLV,
        ARM64_RELOC_TLVP_LOAD_PAGEOFF12 => {
            f.scale = imm12_scale(insn);
            ARM64_LD12_TLV
        }
        // A 32-bit reference to a GOT slot from data, relative to the
        // field (whose contents the assembler leaves to no use).
        ARM64_RELOC_POINTER_TO_GOT if r.is_pcrel => PCREL32_TO_GOT,
        ARM64_RELOC_POINTER_TO_GOT => PTR64_TO_GOT,
        _ => return None,
    };
    Some(f)
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

/// A 64-bit add of an unsigned 12-bit immediate: what takes a slot's
/// address.
fn is_add_x(insn: u32) -> bool {
    insn & 0xffc0_0000 == 0x9100_0000
}
