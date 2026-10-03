//! The Objective-C metadata of a mergeable dylib's record: what the
//! link made of the objects' (see crate::objc) as entries, as ld-prime
//! records its own rewrites - the method lists in the relative form,
//! the records category merging wrote, the selector references of the
//! objc stubs and of the lists - and the slots it leaves for lists a
//! merging link may add.

use hashbrown::HashSet;

use super::{Builder, Content, OutEntry, OutFixup, To, record_size};
use crate::mergeable::{ctype, fk, kind, scope, standard_content_type};
use crate::objc::{DataField, ObjcRef};
use crate::target::Target;

impl<E: Target> Builder<'_, E> {
    /// The entries of the metadata the link made: the selector
    /// references (see add_selref_entries), and, after the imports, the
    /// class references the GOT took over, the method lists in the
    /// relative form and category merging's records, which go by no
    /// name (fixups refer to entries by index). Returns the selector
    /// reference slots, in the order of the __objc_selrefs tail.
    pub(super) fn add_objc_entries(&mut self) -> Vec<To> {
        let ctx = self.ctx;
        let slots = self.add_selref_entries();
        for &(stand_in, id) in &ctx.got.stand_ins {
            let isec = &ctx.isecs[stand_in];
            let mut entry = coalesced(ctype::CLASS_REF, 8, 3);
            entry.content = self.isec_content(isec, ctx.hdr_of(isec));
            let (to, addend) = self.sym_target(id);
            entry.fixups.push(OutFixup::new(0, to, fk::PTR64, addend));
            self.add_linker_isec_entry(stand_in, entry, None);
        }
        for list in &ctx.objc_methlist.lists {
            let isec = &ctx.isecs[list.isec];
            let mut entry = OutEntry::new(scope::LOCAL, kind::ANON, ctype::METHOD_LIST);
            entry.size = isec.size;
            entry.p2align = 3;
            entry.content = self.isec_content(isec, ctx.hdr_of(isec));
            self.add_linker_isec_entry(list.isec, entry, None);
        }
        for blob in &ctx.data_blobs {
            if ctx.isecs[blob.isec].is_alive() {
                self.add_data_blob_entry(blob.isec);
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
            let osec = ctx.output_section(stubs.methname.unwrap());
            let fileoff = osec.hdr.fileoff + osec.tail_off + stubs.methname_offs[i];
            let mut entry = coalesced(ctype::METHOD_NAME, sel.len() as u32 + 1, 0);
            entry.content = Content::Image(fileoff);
            let name = Some(To::Entry(self.push_entry(entry, None)));
            slots.push(self.add_selref(name));
        }
        for &sel in &stubs.extra_selrefs {
            let name = self.isec_target(sel);
            slots.push(self.add_selref(name));
        }
        slots
    }

    /// The entry of a record category merging wrote (the entries of its
    /// records, if it is a list).
    fn add_data_blob_entry(&mut self, isec: u32) {
        let ctx = self.ctx;
        let hdr = ctx.hdr_of(&ctx.isecs[isec]);
        let content_type = standard_content_type(hdr).unwrap_or(ctype::DATA);
        let mut entry = OutEntry::new(scope::LOCAL, kind::ANON, content_type);
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
        let mut entry = coalesced(ctype::SELECTOR_REF, 8, 3);
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
            let is_class = ctype::CLASS_LISTS.contains(&entry.content_type);
            let is_cat = ctype::CATEGORY_LISTS.contains(&entry.content_type);
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
            let mut placeholder = OutEntry::new(scope::LOCAL, kind::ANON_PLACEHOLDER, ctype::DATA);
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
