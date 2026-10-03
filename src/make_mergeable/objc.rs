//! The Objective-C metadata of a mergeable dylib's record: what the
//! link made of the objects' (see crate::objc) as entries, as ld-prime
//! records its own rewrites - the method lists in the relative form,
//! the records category merging wrote, the selector references of the
//! objc stubs and of the lists - and the slots it leaves for lists a
//! merging link may add.

use hashbrown::{HashMap, HashSet};

use super::{
    Builder, CT_DATA, Content, OutEntry, OutFixup, To, record_size, standard_content_type,
};
use crate::mergeable::{fk, kind, scope};
use crate::objc::{DataField, ObjcRef};
use crate::target::Target;

const CT_METHOD_NAME: u8 = 12;
const CT_METHOD_LIST: u8 = 15;
const CT_SELECTOR_REF: u8 = 35;
const CT_CLASS_LISTS: [u8; 2] = [40, 44];
const CT_CATEGORY_LISTS: [u8; 3] = [41, 45, 70];

impl<E: Target> Builder<'_, E> {
    /// The entries of the metadata the link made: the selector
    /// references (see add_selref_entries), and, after the imports, the
    /// method lists in the relative form and category merging's
    /// records. Returns the
    /// selector reference slots, in the order of the __objc_selrefs
    /// tail.
    pub(super) fn add_objc_entries(&mut self) -> Vec<To> {
        let ctx = self.ctx;
        let slots = self.add_selref_entries();
        let names = self.synthetic_names();
        for list in &ctx.objc_methlist.lists {
            let isec = &ctx.isecs[list.isec];
            let mut entry = OutEntry::new(0, kind::REGULAR, CT_METHOD_LIST);
            entry.name = names.get(&list.isec).map(|&(s, _)| s);
            entry.size = isec.size;
            entry.p2align = 3;
            entry.content = self.isec_content(isec, ctx.hdr_of(isec));
            self.add_linker_isec_entry(list.isec, entry, None);
        }
        for blob in &ctx.data_blobs {
            if ctx.isecs[blob.isec].is_alive() {
                self.add_data_blob_entry(blob.isec, names.get(&blob.isec).copied());
            }
        }
        slots
    }

    /// The selector references the link made: for each objc stub (with
    /// the selector's name where no object has it), which the input
    /// references to the selector became, then for each relative method
    /// list entry whose selector no input refers to.
    fn add_selref_entries(&mut self) -> Vec<To> {
        let ctx = self.ctx;
        let stubs = &ctx.objc_stubs;
        let mut slots = Vec::new();
        for (i, (_, sel)) in stubs.symbols.iter().enumerate() {
            let name = match stubs.name_isec[i] {
                u32::MAX => {
                    let osec = ctx.output_section(stubs.methname.unwrap());
                    let fileoff = osec.hdr.fileoff + osec.tail_off + stubs.methname_offs[i];
                    let mut entry = coalesced(CT_METHOD_NAME, sel.len() as u32 + 1, 0);
                    entry.content = Content::Image(fileoff);
                    Some(To::Entry(self.push_entry(entry, None)))
                }
                isec => self.isec_target(isec),
            };
            slots.push(self.add_selref(name));
        }
        for &sel in &stubs.extra_selrefs {
            let name = self.isec_target(sel);
            slots.push(self.add_selref(name));
        }
        for &(stand_in, slot) in &stubs.absorbed {
            self.isec_entry.insert(stand_in, slots[slot as usize]);
        }
        slots
    }

    /// The entry of a record category merging wrote (the entries of its
    /// records, if it is a list), named as the record it replaced.
    fn add_data_blob_entry(&mut self, isec: u32, name: Option<(&'static [u8], u16)>) {
        let ctx = self.ctx;
        let hdr = ctx.hdr_of(&ctx.isecs[isec]);
        let content_type = standard_content_type(hdr).unwrap_or(CT_DATA);
        let mut entry = match name {
            Some((name, debug)) => {
                let mut entry = OutEntry::new(0, kind::REGULAR, content_type);
                entry.name = Some(name);
                if crate::chunks::symtab::has_stabs(hdr) {
                    entry.debug = debug;
                }
                entry
            }
            None => OutEntry::new(0, kind::ANON, content_type),
        };
        entry.no_dead_strip = hdr.flags & crate::macho::S_ATTR_NO_DEAD_STRIP != 0;
        entry.size = ctx.isecs[isec].size;
        entry.p2align = ctx.isecs[isec].p2align;
        entry.content = self.isec_content(&ctx.isecs[isec], hdr);
        self.add_linker_isec_entry(isec, entry, record_size(hdr));
    }

    /// A selector reference to a name, which the link makes without
    /// bytes in the record, as ld-prime does: a merging link writes its
    /// pointer.
    fn add_selref(&mut self, name: Option<To>) -> To {
        let mut entry = coalesced(CT_SELECTOR_REF, 8, 3);
        if let Some(name) = name {
            entry.fixups.push(OutFixup::new(0, name, fk::PTR64, 0));
        }
        To::Entry(self.push_entry(entry, None))
    }

    /// Adds the entry of a subsection the link made, after the imports
    /// (the entries of its records, see Builder::split_records).
    fn add_linker_isec_entry(&mut self, isec: u32, entry: OutEntry, record: Option<u32>) {
        self.isec_entry.insert(isec, To::Tail(self.tail.len() as u32));
        let entries = self.split_records(isec, entry, record);
        self.tail.extend(entries);
    }

    /// The names of the records the link made: those the objects'
    /// symbols moved to (a rewritten method list keeps its name), and
    /// those it named itself (a merged list); each with the debug notes
    /// of the object of the symbol.
    fn synthetic_names(&self) -> HashMap<u32, (&'static [u8], u16)> {
        let ctx = self.ctx;
        let made: HashSet<u32> = (ctx.objc_methlist.lists.iter().map(|l| l.isec))
            .chain(ctx.data_blobs.iter().map(|b| b.isec))
            .collect();
        let mut names: HashMap<u32, (&'static [u8], u16)> = HashMap::new();
        if made.is_empty() {
            return names;
        }
        for &(name, isec) in &ctx.extra_local_syms {
            names.entry(isec).or_insert((name, 0));
        }
        // A rewritten record's symbols name its replacement; a label the
        // assembler made does only if nothing else does.
        for sym in &ctx.symbols.syms {
            let Some(isec) = sym.input_section() else { continue };
            let isec = ctx.resolve_isec(isec as usize) as u32;
            if sym.value != 0 || !made.contains(&isec) {
                continue;
            }
            let name = sym.name();
            let label = crate::input_files::is_private_label(name);
            match names.get(&isec) {
                Some(&(old, _)) if label || !crate::input_files::is_private_label(old) => {}
                _ => {
                    let debug = match sym.file() {
                        Some(crate::input_files::FileId::Obj(o)) => self.obj_debug[o as usize],
                        _ => 0,
                    };
                    names.insert(isec, (name, debug));
                }
            }
        }
        names
    }

    /// The fixups of the records the link made, from the references
    /// they hold: a relative method list's entries are each three
    /// distances from the field (to a selector reference, the types
    /// and the implementation), a record's pointers absolute.
    pub(super) fn add_objc_fixups(&mut self, slots: &[To]) {
        let ctx = self.ctx;
        let target = |b: &Self, r: ObjcRef| -> Option<(To, i64)> {
            match r {
                ObjcRef::Isec(isec, off) => b.isec_target(isec).map(|to| (to, off as i64)),
                ObjcRef::Sym(id, addend) => {
                    let (to, off) = b.sym_target(id);
                    Some((to, off + addend))
                }
                ObjcRef::TailSelref(n) => slots.get(n).map(|&to| (to, 0)),
                ObjcRef::Null => None,
            }
        };
        for list in &ctx.objc_methlist.lists {
            let Some(To::Tail(t)) = self.isec_entry.get(&list.isec).copied() else { continue };
            let mut fixups = Vec::new();
            for (i, m) in list.methods.iter().enumerate() {
                for (k, r) in [m.name, m.types, m.imp].into_iter().enumerate() {
                    if let Some((to, addend)) = target(self, r) {
                        let off = (8 + 12 * i + 4 * k) as u32;
                        fixups.push(OutFixup::new(off, to, fk::PCREL_DELTA32, addend));
                    }
                }
            }
            self.tail[t as usize].fixups = fixups;
        }
        self.add_data_blob_fixups(&target);
    }

    /// The pointers of category merging's records, each a fixup of its
    /// record's entry.
    fn add_data_blob_fixups(&mut self, target: &impl Fn(&Self, ObjcRef) -> Option<(To, i64)>) {
        let ctx = self.ctx;
        for blob in &ctx.data_blobs {
            let Some(To::Tail(t)) = self.isec_entry.get(&blob.isec).copied() else { continue };
            let mut fixups = Vec::new();
            let mut off = 0u32;
            for field in &blob.fields {
                match field {
                    DataField::Bytes(b) => off += b.len() as u32,
                    DataField::Ptr(r) => {
                        if let Some((to, addend)) = target(self, *r) {
                            fixups.push(OutFixup::new(off, to, fk::PTR64, addend));
                        }
                        off += 8;
                    }
                }
            }
            for (k, f) in self.place_fixups(blob.isec, fixups) {
                self.tail[t as usize + k].fixups.push(f);
            }
        }
    }

    /// The slots ld-prime leaves for the lists a merging link may add
    /// to a class or category of the dylib: a fixup to an entry of no
    /// bytes (an "anonPlaceholder") at each null list pointer of a
    /// class's and its metaclass's class_ro_t (methods, protocols,
    /// properties), and of a category_t (its method, protocol and
    /// property lists), in the order of the class and category lists.
    /// ld-prime's merging link crashes without them.
    pub(super) fn add_objc_placeholders(&mut self) {
        let lists: Vec<To> = (0..self.entries.len())
            .map(|i| To::Entry(i as u32))
            .chain((0..self.tail.len()).map(|i| To::Tail(i as u32)))
            .collect();
        let mut seen = HashSet::new();
        let mut cats = Vec::new();
        for &list in &lists {
            let entry = self.entry(list);
            let is_class = CT_CLASS_LISTS.contains(&entry.content_type);
            let is_cat = CT_CATEGORY_LISTS.contains(&entry.content_type);
            if !is_class && !is_cat {
                continue;
            }
            let listed: Vec<(To, i64)> = (entry.fixups.iter())
                .filter(|f| f.kind == fk::PTR64)
                .map(|f| self.through_alias(f.target, f.addend))
                .collect();
            for item in listed {
                if is_cat {
                    cats.push(item);
                } else if seen.insert(item) {
                    for cls in [Some(item), self.pointer_at(item, 0)].into_iter().flatten() {
                        // class_t.data, whose low bits a Swift class
                        // uses for flags.
                        if let Some((ro, off)) = self.pointer_at(cls, 32) {
                            self.add_placeholders((ro, off & !7), &[32, 40, 64]);
                        }
                    }
                }
            }
        }
        for cat in cats {
            self.add_placeholders(cat, &[16, 24, 32, 40, 48]);
        }
    }

    /// A placeholder for each null pointer field of a record.
    fn add_placeholders(&mut self, rec: (To, i64), fields: &[i64]) {
        if !matches!(rec.0, To::Entry(_) | To::Tail(_)) {
            return;
        }
        for &field in fields {
            let off = rec.1 + field;
            let entry = self.entry(rec.0);
            if off < 0
                || off + 8 > entry.size as i64
                || entry.fixups.iter().any(|f| f.offset as i64 == off)
            {
                continue;
            }
            let mut placeholder = OutEntry::new(0, kind::ANON_PLACEHOLDER, CT_DATA);
            placeholder.size = 8;
            let to = To::Entry(self.push_entry(placeholder, None));
            let f = OutFixup::new(off as u32, to, fk::PTR64, 0);
            self.entry_mut(rec.0).fixups.push(f);
        }
    }

    fn entry(&self, to: To) -> &OutEntry {
        match to {
            To::Entry(i) => &self.entries[i as usize],
            To::Tail(i) => &self.tail[i as usize],
            _ => unreachable!(),
        }
    }

    fn entry_mut(&mut self, to: To) -> &mut OutEntry {
        match to {
            To::Entry(i) => &mut self.entries[i as usize],
            To::Tail(i) => &mut self.tail[i as usize],
            _ => unreachable!(),
        }
    }

    /// The entry and offset a reference reaches, an alias's target's.
    fn through_alias(&self, to: To, addend: i64) -> (To, i64) {
        if matches!(to, To::Entry(_) | To::Tail(_)) {
            let entry = self.entry(to);
            if entry.kind == kind::ALIAS
                && let Some(f) = entry.fixups.iter().find(|f| f.kind == fk::ALIAS_OF)
            {
                return (f.target, f.addend + addend);
            }
        }
        (to, addend)
    }

    /// The target of the pointer at `off` of a record of the image.
    fn pointer_at(&self, rec: (To, i64), off: i64) -> Option<(To, i64)> {
        if !matches!(rec.0, To::Entry(_) | To::Tail(_)) {
            return None;
        }
        let at = rec.1 + off;
        let f = (self.entry(rec.0).fixups.iter())
            .find(|f| f.offset as i64 == at && f.kind == fk::PTR64)?;
        Some(self.through_alias(f.target, f.addend))
    }
}

/// A hidden entry merged by its content, as a literal.
fn coalesced(content_type: u8, size: u32, p2align: u8) -> OutEntry {
    let mut entry = OutEntry::new(scope::HIDDEN, kind::ANON_COAL_BY_CONTENT, content_type);
    entry.size = size;
    entry.p2align = p2align;
    entry
}
