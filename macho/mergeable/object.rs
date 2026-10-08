//! The object file a mergeable dylib's record stands for, which a
//! merging link takes in place of the dylib: as an `ld -r` of the
//! dylib's objects would write it, debug notes included. Each entry
//! gets its section (one of ld-prime's standard ones by its content
//! type, or one of the custom table's), its symbol or a private label,
//! and its fixups the relocations the objects had; an import stays
//! undefined, and the dylibs the mergeable one links stand by their
//! install names (see reader::add_merged_dependencies). What ld-prime
//! keeps of the objects is all there is: no data-in-code entries and no
//! optimization hints survive, in its merged images either.

use std::path::Path;

use super::{
    Entry, FLAG_CATEGORY_CLASS_PROPERTIES, FLAG_HAS_OBJC_INFO, FLAG_SIGNED_CLASS_RO, Fixup,
    MergeableRecord, ctype, fk, kind, read32, read64, scope, standard_section,
};
use crate::arch::Target;
use crate::error::RawPath;
use crate::fatal;
use crate::macho::*;
use crate::util::align_to_mod;

/// Whether a section's subsections are fixed-size records or literals
/// the linker splits by itself, all of one alignment. (A C string keeps
/// its alignment and modulus as any subsection.)
fn is_record_section(flags: u32, sectname: &[u8; 16]) -> bool {
    matches!(
        flags & SECTION_TYPE,
        S_4BYTE_LITERALS
            | S_8BYTE_LITERALS
            | S_16BYTE_LITERALS
            | S_LITERAL_POINTERS
            | S_MOD_INIT_FUNC_POINTERS
            | S_MOD_TERM_FUNC_POINTERS
            | S_THREAD_LOCAL_VARIABLES
    ) || [
        "__compact_unwind",
        "__eh_frame",
        "__cfstring",
        "__objc_classrefs",
        "__objc_superrefs",
        "__objc_classlist",
        "__objc_catlist",
        "__objc_catlist2",
        "__objc_nlclslist",
        "__objc_nlcatlist",
        "__objc_protolist",
        "__objc_protorefs",
        "__objc_clsrolist",
    ]
    .iter()
    .any(|n| bytes_to_name(n.as_bytes()) == *sectname)
}

fn is_zerofill(flags: u32) -> bool {
    matches!(flags & SECTION_TYPE, S_ZEROFILL | S_GB_ZEROFILL | S_THREAD_LOCAL_ZEROFILL)
}

/// A section by its segment and section names and flags.
type SectionKey = ([u8; 16], [u8; 16], u32);

/// A section of the object being made.
struct Section {
    segname: [u8; 16],
    sectname: [u8; 16],
    flags: u32,
    p2align: u8,
    data: Vec<u8>,
    size: u64,
    addr: u64,
    relocs: Vec<MachRel>,
}

impl Section {
    fn new(segname: [u8; 16], sectname: [u8; 16], flags: u32, p2align: u8) -> Self {
        let (data, relocs) = (Vec::new(), Vec::new());
        Self { segname, sectname, flags, p2align, data, size: 0, addr: 0, relocs }
    }

    /// Appends an entry's bytes (zeros where it has none, nothing in zero
    /// fill) at the first offset that is its modulus past a multiple of
    /// its alignment, which is where it was in its object - a record at
    /// one of the alignment, as the linker aligns records. Returns the
    /// offset.
    fn append(&mut self, content: Option<&[u8]>, size: u64, p2align: u8, modulus: u64) -> u64 {
        self.p2align = self.p2align.max(p2align);
        let modulus = if is_record_section(self.flags, &self.sectname) { 0 } else { modulus };
        let off = align_to_mod(self.size, 1 << p2align, modulus);
        self.size = off + size;
        if !is_zerofill(self.flags) {
            self.data.resize(off as usize, 0);
            match content {
                Some(bytes) => self.data.extend_from_slice(bytes),
                None => self.data.resize(self.size as usize, 0),
            }
        }
        off
    }
}

/// Where a symbol of the object being made stands.
#[derive(Clone, Copy)]
enum SymPlace {
    Defined { sect: usize, offset: u64 },
    Undefined,
    Common { size: u64, p2align: u8 },
    Absolute(u64),
}

struct Symbol {
    name: Vec<u8>,
    place: SymPlace,
    /// N_EXT, N_PEXT.
    n_type: u8,
    desc: u16,
}

/// The object a mergeable record stands for, under construction.
struct Synth<'a, E: Target> {
    rec: &'a MergeableRecord,
    sections: Vec<Section>,
    /// The section of each section key and alignment.
    open: hashbrown::HashMap<(SectionKey, u8), usize>,
    /// Each entry's section and offset there, if it has a place.
    place: Vec<Option<(usize, u64)>>,
    /// The symbol that stands for each entry, if any.
    sym_of: Vec<Option<usize>>,
    symbols: Vec<Symbol>,
    undefined: hashbrown::HashMap<Vec<u8>, usize>,
    /// The class reference slots made for the GOT references ld-prime
    /// records of them (see add_class_refs).
    class_refs: Vec<ClassRef>,
    _target: std::marker::PhantomData<E>,
}

/// A class reference slot of the object being made: the entry of its
/// class, its section and offset, and its label.
struct ClassRef {
    class: u32,
    sect: usize,
    offset: u64,
    label: usize,
}

/// Makes the object file a mergeable dylib's record stands for.
pub fn synthesize_object<E: Target>(rec: &MergeableRecord, path: &Path) -> Vec<u8> {
    let mut s = Synth::<E> {
        rec,
        sections: Vec::new(),
        open: hashbrown::HashMap::new(),
        place: vec![None; rec.entries.len()],
        sym_of: vec![None; rec.entries.len()],
        symbols: Vec::new(),
        undefined: hashbrown::HashMap::new(),
        class_refs: Vec::new(),
        _target: std::marker::PhantomData,
    };
    for i in 0..rec.entries.len() {
        s.place(i, path);
    }
    s.add_class_refs();
    s.add_image_info();
    s.assign_addresses();
    for i in 0..rec.entries.len() {
        s.name_entry(i);
    }
    for i in 0..rec.entries.len() {
        s.name_alias(i);
    }
    s.name_class_refs();
    for i in 0..rec.entries.len() {
        s.apply_fixups(i, path);
    }
    s.write()
}

impl<E: Target> Synth<'_, E> {
    fn is_arm64() -> bool {
        E::CPUTYPE == CPU_TYPE_ARM64
    }

    fn unsigned() -> u8 {
        if Self::is_arm64() { ARM64_RELOC_UNSIGNED } else { X86_64_RELOC_UNSIGNED }
    }

    /// Lays an entry out in its section, if it has a place in one.
    fn place(&mut self, i: usize, path: &Path) {
        use kind::*;
        let entry = &self.rec.entries[i];
        if !matches!(entry.kind, REGULAR | WEAK_DEF | RESOLVER | ANON | ANON_COAL_BY_CONTENT) {
            if matches!(entry.kind, 13..=17) {
                fatal!("{}: unsupported entry kind {} in LC_ATOM_INFO", path.raw(), entry.kind);
            }
            return;
        }
        // The objects' image info is made afresh (see add_image_info).
        if entry.content_type == ctype::OBJC_IMAGE_INFO {
            return;
        }
        let key = self.section_key(entry, path);
        // An initializer offset the dylib's link made of a pointer is
        // the pointer again, which the linker makes an offset itself
        // where the target is new enough.
        let (size, p2align, modulus, content) = if entry.content_type == ctype::INIT_OFFSET {
            (8, 3, 0, None)
        } else {
            (entry.size as u64, entry.p2align, entry.modulus as u64, entry.content)
        };
        let idx = self.section_for(key, p2align);
        let off = self.sections[idx].append(content, size, p2align, modulus);
        self.place[i] = Some((idx, off));
    }

    /// The section an entry goes in: one of the custom table's, or the
    /// standard one of its content type.
    fn section_key(&self, entry: &Entry, path: &Path) -> SectionKey {
        if let Some(idx) = entry.custom_section {
            let s = &self.rec.sections[idx as usize];
            return (s.segname, s.sectname, s.flags);
        }
        match standard_section(entry.content_type) {
            Some((seg, sect, flags)) => (bytes_to_name(seg), bytes_to_name(sect), flags),
            None => fatal!(
                "{}: unsupported content type {} in LC_ATOM_INFO",
                path.raw(),
                entry.content_type
            ),
        }
    }

    /// The section of the object to put an entry of a key and alignment
    /// in. The objects had a section of a name each, but with their own
    /// alignments, which to the linker are those of all their
    /// subsections: entries of another alignment go in a section of
    /// their own. Records, which the linker aligns itself, go in one.
    fn section_for(&mut self, key: SectionKey, p2align: u8) -> usize {
        let (segname, sectname, flags) = key;
        let p2align = if is_record_section(flags, &sectname) { 0 } else { p2align };
        *self.open.entry((key, p2align)).or_insert_with(|| {
            self.sections.push(Section::new(segname, sectname, flags, p2align));
            self.sections.len() - 1
        })
    }

    /// Makes the class reference slots the objects had where ld-prime
    /// records the address of a class's GOT entry - taken by an adrp
    /// and add (ARM64_ADRP_ADD_GOT) or held by a pointer (PTR64_TO_GOT),
    /// which it records only of a slot whose address the code took
    /// and a link for macOS 15 or later makes the GOT entry (see
    /// objc::fold_objc_classrefs). No relocation of an object says as
    /// much: an add with a GOT relocation is a load relaxed, of the
    /// class itself, and neither linker takes a 64-bit pointer to a GOT
    /// entry. One slot per class.
    fn add_class_refs(&mut self) {
        let rec = self.rec;
        let (seg, sect, flags) = standard_section(ctype::CLASS_REF).unwrap();
        let key = (bytes_to_name(seg), bytes_to_name(sect), flags);
        for (i, entry) in rec.entries.iter().enumerate() {
            if self.place[i].is_none() {
                continue;
            }
            for f in &rec.fixups[entry.fixups.clone()] {
                if !matches!(f.kind, fk::ARM64_ADRP_ADD_GOT | fk::PTR64_TO_GOT)
                    || self.class_refs.iter().any(|c| c.class == f.target)
                {
                    continue;
                }
                let sect = self.section_for(key, 3);
                let offset = self.sections[sect].append(None, 8, 3, 0);
                self.class_refs.push(ClassRef { class: f.target, sect, offset, label: 0 });
            }
        }
    }

    /// Gives the class reference slots (see add_class_refs) their
    /// labels, and points each at its class.
    fn name_class_refs(&mut self) {
        for n in 0..self.class_refs.len() {
            let ClassRef { class, sect, offset, .. } = self.class_refs[n];
            let label = self.add_symbol(Symbol {
                name: format!("LMC{n}").into_bytes(),
                place: SymPlace::Defined { sect, offset },
                n_type: 0,
                desc: 0,
            });
            self.class_refs[n].label = label;
            let class = self.target_sym(class).expect("a fixup's target has a symbol");
            let reloc = Self::reloc(offset as u32, class, Self::unsigned(), 3, false);
            self.sections[sect].relocs.push(reloc);
        }
    }

    /// The label of a class's reference slot (see add_class_refs).
    fn class_ref(&self, class: u32) -> Option<usize> {
        Some(self.class_refs.iter().find(|c| c.class == class)?.label)
    }

    /// Adds the __objc_imageinfo record the objects had, from what the
    /// record's flags say of it.
    fn add_image_info(&mut self) {
        let flags = self.rec.flags;
        if flags & FLAG_HAS_OBJC_INFO == 0 {
            return;
        }
        let mut info = (flags as u32 & 0xff) << 8 | (flags as u32 & 0xffff00) << 8;
        if flags & FLAG_CATEGORY_CLASS_PROPERTIES != 0 {
            info |= 0x40;
        }
        if flags & FLAG_SIGNED_CLASS_RO != 0 {
            info |= 0x10;
        }
        let mut sect =
            Section::new(bytes_to_name(b"__DATA"), bytes_to_name(b"__objc_imageinfo"), 0, 2);
        sect.append(Some(&[[0; 4], info.to_le_bytes()].concat()), 8, 2, 0);
        self.sections.push(sect);
    }

    /// Gives the sections their addresses in the object, one after
    /// another, the zero fill ones last.
    fn assign_addresses(&mut self) {
        let mut addr = 0u64;
        for zerofill in [false, true] {
            for s in self.sections.iter_mut().filter(|s| is_zerofill(s.flags) == zerofill) {
                addr = addr.next_multiple_of(1u64 << s.p2align);
                s.addr = addr;
                addr += s.size;
            }
        }
    }

    fn add_symbol(&mut self, sym: Symbol) -> usize {
        self.symbols.push(sym);
        self.symbols.len() - 1
    }

    /// The undefined symbol of a name, made once.
    fn undefined(&mut self, name: &[u8], weak: bool) -> usize {
        if let Some(&idx) = self.undefined.get(name) {
            if weak {
                self.symbols[idx].desc |= N_WEAK_REF;
            }
            return idx;
        }
        let desc = if weak { N_WEAK_REF } else { 0 };
        let idx = self.add_symbol(Symbol {
            name: name.to_vec(),
            place: SymPlace::Undefined,
            n_type: N_EXT,
            desc,
        });
        self.undefined.insert(name.to_vec(), idx);
        idx
    }

    /// Gives an entry its symbol: its name in its section, or a private
    /// label if it has none (the linker splits the section at it, as
    /// it was a subsection of its own), or an undefined or common symbol.
    fn name_entry(&mut self, i: usize) {
        use kind::*;
        let entry = &self.rec.entries[i];
        let name = entry.name;
        match entry.kind {
            DYLIB_EXPORT
            | DYLIB_EXPORT_WEAK_DEF
            | DYLIB_EXPORT_FORCE_LOAD
            | UNDEFINE
            | UNDEFINE_WEAK_IMPORT => {
                let weak = entry.import == 1 || entry.kind == UNDEFINE_WEAK_IMPORT;
                let Some(name) = name else { return };
                self.sym_of[i] = Some(self.undefined(name, weak));
            }
            TENTATIVE_DEF => {
                let Some(name) = name else { return };
                let (n_type, desc) = scope_bits(entry.scope);
                self.sym_of[i] = Some(self.add_symbol(Symbol {
                    name: name.to_vec(),
                    place: SymPlace::Common { size: entry.size as u64, p2align: entry.p2align },
                    n_type: n_type | N_EXT,
                    desc,
                }));
            }
            // Its value is its content, eight bytes.
            ABSOLUTE => {
                let (Some(name), Some(value)) = (name, entry.content) else { return };
                let value = value.get(..8).map_or(0, |v| read64(v, 0));
                let (n_type, desc) = scope_bits(entry.scope);
                self.sym_of[i] = Some(self.add_symbol(Symbol {
                    name: name.to_vec(),
                    place: SymPlace::Absolute(value),
                    n_type,
                    desc,
                }));
            }
            _ => {
                let Some((sect, offset)) = self.place[i] else { return };
                // eh_frame records are found by their lengths and need
                // no names.
                if entry.content_type == ctype::CFI {
                    return;
                }
                let (name, n_type, mut desc) = match name {
                    Some(name) => {
                        let (n_type, desc) = scope_bits(entry.scope);
                        (name.to_vec(), n_type, desc)
                    }
                    None => (format!("LM{i}").into_bytes(), 0, 0),
                };
                if entry.kind == WEAK_DEF {
                    desc |= N_WEAK_DEF;
                }
                if entry.kind == RESOLVER {
                    desc |= N_SYMBOL_RESOLVER;
                }
                if entry.no_dead_strip {
                    desc |= N_NO_DEAD_STRIP;
                }
                if entry.cold {
                    desc |= N_COLD_FUNC;
                }
                self.sym_of[i] = Some(self.add_symbol(Symbol {
                    name,
                    place: SymPlace::Defined { sect, offset },
                    n_type,
                    desc,
                }));
            }
        }
    }

    /// Gives an alias its symbol: at its target's place plus the
    /// addend, an alternate entry there unless at its start, or
    /// undefined if the target is (__dso_handle, which ld-prime makes
    /// an alias of segment$start$__TEXT, and which the merging link
    /// defines afresh).
    fn name_alias(&mut self, i: usize) {
        use kind::*;
        let entry = &self.rec.entries[i];
        if !matches!(entry.kind, ALIAS | WEAK_DEF_ALIAS) {
            return;
        }
        let Some(name) = entry.name else { return };
        let Some(f) = self.rec.fixups[entry.fixups.clone()].iter().find(|f| f.kind == fk::ALIAS_OF)
        else {
            return;
        };
        let target = f.target as usize;
        let Some((sect, offset)) = self.place.get(target).copied().flatten() else {
            self.sym_of[i] = Some(self.undefined(name, false));
            return;
        };
        let (n_type, mut desc) = scope_bits(entry.scope);
        if f.addend != 0 {
            desc |= N_ALT_ENTRY;
        }
        if entry.kind == WEAK_DEF_ALIAS {
            desc |= N_WEAK_DEF;
        }
        if entry.no_dead_strip {
            desc |= N_NO_DEAD_STRIP;
        }
        let offset = offset.wrapping_add_signed(f.addend);
        self.sym_of[i] = Some(self.add_symbol(Symbol {
            name: name.to_vec(),
            place: SymPlace::Defined { sect, offset },
            n_type,
            desc,
        }));
    }

    /// The object address of an entry plus `addend`.
    fn addr(&self, entry: u32, addend: i64) -> u64 {
        let (sect, off) = self.place[entry as usize].unwrap_or((0, 0));
        (self.sections[sect].addr + off).wrapping_add_signed(addend)
    }

    /// The symbol that stands for an entry, by which a relocation
    /// refers to it.
    fn target_sym(&self, entry: u32) -> Option<usize> {
        self.sym_of.get(entry as usize).copied().flatten()
    }

    fn reloc(offset: u32, idx: usize, ty: u8, p2size: u32, pcrel: bool) -> MachRel {
        MachRel {
            offset,
            bits: idx as u32 & 0xff_ffff
                | (pcrel as u32) << 24
                | p2size << 25
                | 1 << 27
                | (ty as u32) << 28,
        }
    }

    /// Turns an entry's fixups into the relocations its object had, and
    /// puts back in its bytes what the object had where they apply.
    fn apply_fixups(&mut self, i: usize, path: &Path) {
        let entry = &self.rec.entries[i];
        let Some((sect, entry_off)) = self.place[i] else { return };
        if entry.content_type == ctype::CFI {
            return self.apply_cfi_fixups(i);
        }
        for f in &self.rec.fixups[entry.fixups.clone()] {
            if f.kind == fk::ALIAS_OF || f.kind == fk::KEEP_ALIVE {
                continue;
            }
            let target = self.rec.entries.get(f.target as usize);
            if target.is_none_or(|t| t.kind == kind::ANON_PLACEHOLDER) {
                continue;
            }
            let sym = self.target_sym(f.target).expect("a fixup's target has a symbol");
            let off = (entry_off + f.offset as u64) as u32;
            let mut out = Vec::new();
            let ok = if Self::is_arm64() {
                self.arm64_fixup(sect, off, i, f, sym, &mut out)
            } else {
                self.x86_64_fixup(sect, off, i, f, sym, &mut out)
            };
            if !ok {
                fatal!("{}: unsupported fixup kind 0x{:x} in LC_ATOM_INFO", path.raw(), f.kind);
            }
            self.sections[sect].relocs.extend(out);
        }
    }

    /// Writes `val` of `size` bytes at `off` of a section.
    fn put(&mut self, sect: usize, off: u32, size: u32, val: u64) {
        let data = &mut self.sections[sect].data;
        let off = off as usize;
        match size {
            4 => data[off..off + 4].copy_from_slice(&(val as u32).to_le_bytes()),
            8 => data[off..off + 8].copy_from_slice(&val.to_le_bytes()),
            _ => data[off] = val as u8,
        }
    }

    fn insn(&self, sect: usize, off: u32) -> u32 {
        read32(&self.sections[sect].data, off as usize)
    }

    fn set_insn(&mut self, sect: usize, off: u32, insn: u32) {
        self.put(sect, off, 4, insn as u64);
    }

    /// A SUBTRACTOR pair: `from`'s symbol taken from the target's, the
    /// field holding the addend.
    fn diff_pair(
        &mut self,
        sect: usize,
        off: u32,
        size: u32,
        from: usize,
        sym: usize,
        addend: i64,
    ) -> [MachRel; 2] {
        let (sub, unsigned) = if Self::is_arm64() {
            (ARM64_RELOC_SUBTRACTOR, ARM64_RELOC_UNSIGNED)
        } else {
            (X86_64_RELOC_SUBTRACTOR, X86_64_RELOC_UNSIGNED)
        };
        let p2size = if size == 8 { 3 } else { 2 };
        self.put(sect, off, size, addend as u64);
        [Self::reloc(off, from, sub, p2size, false), Self::reloc(off, sym, unsigned, p2size, false)]
    }

    /// The generic fixups, as either target encodes them.
    fn generic_fixup(
        &mut self,
        sect: usize,
        off: u32,
        entry: usize,
        f: &Fixup,
        sym: usize,
        out: &mut Vec<MachRel>,
    ) -> bool {
        let unsigned = Self::unsigned();
        match f.kind {
            fk::PTR64 | fk::TLV_OFFSET | fk::IMAGE_OFFSET32 => {
                self.put(sect, off, 8, f.addend as u64);
                out.push(Self::reloc(off, sym, unsigned, 3, false));
            }
            // A pointer to a class's GOT entry: to its class reference
            // slot again (see add_class_refs).
            fk::PTR64_TO_GOT if f.addend == 0 => {
                let Some(slot) = self.class_ref(f.target) else { return false };
                self.put(sect, off, 8, 0);
                out.push(Self::reloc(off, slot, unsigned, 3, false));
            }
            fk::PTR32 => {
                self.put(sect, off, 4, f.addend as u64);
                out.push(Self::reloc(off, sym, unsigned, 2, false));
            }
            fk::DIFF32 | fk::DIFF64 => {
                let size = if f.kind == fk::DIFF64 { 8 } else { 4 };
                let Some(from) = f.from.and_then(|e| self.target_sym(e)) else { return false };
                out.extend(self.diff_pair(sect, off, size, from, sym, f.addend));
            }
            fk::PCREL_DELTA32 => {
                // Relative to the field: to the entry's start, less the
                // field's place in it.
                let Some(from) = self.target_sym(entry as u32) else { return false };
                let field = off as i64 - self.place[entry].unwrap().1 as i64;
                out.extend(self.diff_pair(sect, off, 4, from, sym, f.addend - field));
            }
            _ => return false,
        }
        true
    }

    fn arm64_fixup(
        &mut self,
        sect: usize,
        off: u32,
        entry: usize,
        f: &Fixup,
        sym: usize,
        out: &mut Vec<MachRel>,
    ) -> bool {
        use fk::*;
        // An ADDEND before the record carries a nonzero addend.
        let addend = |out: &mut Vec<MachRel>, at: u32| {
            if f.addend != 0 {
                let bits =
                    (f.addend as u32 & 0xff_ffff) | 2 << 25 | (ARM64_RELOC_ADDEND as u32) << 28;
                out.push(MachRel { offset: at, bits });
            }
        };
        let second = off + 4 * f.second as u32;
        match f.kind {
            ARM64_B26 | ARM64_B26_ADDEND => {
                self.set_insn(sect, off, self.insn(sect, off) & 0xfc00_0000);
                addend(out, off);
                out.push(Self::reloc(off, sym, ARM64_RELOC_BRANCH26, 2, true));
            }
            ARM64_ADRP | ARM64_ADRP_ADDEND => {
                self.clear_adrp(sect, off);
                addend(out, off);
                out.push(Self::reloc(off, sym, ARM64_RELOC_PAGE21, 2, true));
            }
            ARM64_LO12 | ARM64_LO12_ADDEND => {
                self.clear_imm12(sect, off);
                addend(out, off);
                out.push(Self::reloc(off, sym, ARM64_RELOC_PAGEOFF12, 2, false));
            }
            ARM64_ADRP_LO12 | ARM64_ADRP_LO12_ADDEND => {
                self.clear_adrp(sect, off);
                addend(out, off);
                out.push(Self::reloc(off, sym, ARM64_RELOC_PAGE21, 2, true));
                self.clear_imm12(sect, second);
                addend(out, second);
                out.push(Self::reloc(second, sym, ARM64_RELOC_PAGEOFF12, 2, false));
            }
            ARM64_ADRP_GOT | ARM64_ADRP_GOT_NO_OPT => {
                self.clear_adrp(sect, off);
                out.push(Self::reloc(off, sym, ARM64_RELOC_GOT_LOAD_PAGE21, 2, true));
            }
            ARM64_LD12_GOT | ARM64_LD12_GOT_NO_OPT => {
                self.restore_ldr(sect, off);
                out.push(Self::reloc(off, sym, ARM64_RELOC_GOT_LOAD_PAGEOFF12, 2, false));
            }
            ARM64_ADD_GOT => {
                self.clear_imm12(sect, off);
                out.push(Self::reloc(off, sym, ARM64_RELOC_GOT_LOAD_PAGEOFF12, 2, false));
            }
            ARM64_ADRP_LDR_GOT | ARM64_ADRP_LDR_GOT_NO_OPT => {
                self.clear_adrp(sect, off);
                out.push(Self::reloc(off, sym, ARM64_RELOC_GOT_LOAD_PAGE21, 2, true));
                self.restore_ldr(sect, second);
                out.push(Self::reloc(second, sym, ARM64_RELOC_GOT_LOAD_PAGEOFF12, 2, false));
            }
            // The address of the class's GOT entry: of its class
            // reference slot again.
            ARM64_ADRP_ADD_GOT if f.addend == 0 => {
                let Some(slot) = self.class_ref(f.target) else { return false };
                self.clear_adrp(sect, off);
                out.push(Self::reloc(off, slot, ARM64_RELOC_PAGE21, 2, true));
                self.clear_imm12(sect, second);
                out.push(Self::reloc(second, slot, ARM64_RELOC_PAGEOFF12, 2, false));
            }
            ARM64_ADRP_TLV => {
                self.clear_adrp(sect, off);
                out.push(Self::reloc(off, sym, ARM64_RELOC_TLVP_LOAD_PAGE21, 2, true));
            }
            ARM64_LD12_TLV => {
                self.restore_ldr(sect, off);
                out.push(Self::reloc(off, sym, ARM64_RELOC_TLVP_LOAD_PAGEOFF12, 2, false));
            }
            PCREL32_TO_GOT | SWIFT_REL32_TO_GOT => {
                let swift = (f.kind == SWIFT_REL32_TO_GOT) as i64;
                self.put(sect, off, 4, (f.addend + swift) as u64);
                out.push(Self::reloc(off, sym, ARM64_RELOC_POINTER_TO_GOT, 2, true));
            }
            _ => return self.generic_fixup(sect, off, entry, f, sym, out),
        }
        true
    }

    fn x86_64_fixup(
        &mut self,
        sect: usize,
        off: u32,
        entry: usize,
        f: &Fixup,
        sym: usize,
        out: &mut Vec<MachRel>,
    ) -> bool {
        use fk::*;
        let (ty, bias) = match f.kind {
            X86_64_CALL => (X86_64_RELOC_BRANCH, 0),
            X86_64_RIP => (X86_64_RELOC_SIGNED, 0),
            X86_64_RIP1 => (X86_64_RELOC_SIGNED_1, 1),
            X86_64_RIP2 => (X86_64_RELOC_SIGNED_2, 2),
            X86_64_RIP4 => (X86_64_RELOC_SIGNED_4, 4),
            X86_64_RIP_GOT | PCREL32_TO_GOT | SWIFT_REL32_TO_GOT => (X86_64_RELOC_GOT, 0),
            X86_64_RIP_GOT_LOAD => (X86_64_RELOC_GOT_LOAD, 0),
            X86_64_RIP_TLV_LOAD => (X86_64_RELOC_TLV, 0),
            X86_64_BRANCH8 => {
                self.put(sect, off, 1, f.addend as u64);
                out.push(Self::reloc(off, sym, X86_64_RELOC_BRANCH, 0, true));
                return true;
            }
            _ => return self.generic_fixup(sect, off, entry, f, sym, out),
        };
        // A movq ld-prime relaxed to a leaq loads again.
        if matches!(ty, X86_64_RELOC_GOT_LOAD | X86_64_RELOC_TLV)
            && let Some(at) = off.checked_sub(2)
            && self.sections[sect].data[at as usize] == 0x8d
        {
            self.sections[sect].data[at as usize] = 0x8b;
        }
        let addend = match f.kind {
            // A data reference to a GOT slot is from the field's start,
            // as the instruction's are from its end; Swift's relative
            // reference to one has its low bit set besides (the indirect
            // flag of a relative pointer), which ld-prime's kind implies.
            PCREL32_TO_GOT => f.addend + 4,
            SWIFT_REL32_TO_GOT => f.addend + 5,
            _ => f.addend - bias,
        };
        self.put(sect, off, 4, addend as u64);
        out.push(Self::reloc(off, sym, ty, 2, true));
        true
    }

    /// Clears an ADRP's page.
    fn clear_adrp(&mut self, sect: usize, off: u32) {
        let insn = self.insn(sect, off);
        if insn & 0x9f00_0000 == 0x9000_0000 {
            self.set_insn(sect, off, insn & 0x9f00_001f);
        }
    }

    /// Clears a load's, store's or add's 12-bit immediate.
    fn clear_imm12(&mut self, sect: usize, off: u32) {
        self.set_insn(sect, off, self.insn(sect, off) & 0xffc0_03ff);
    }

    /// Puts back the load of a GOT or TLV slot that ld-prime relaxed to
    /// an add of the target's page offset.
    fn restore_ldr(&mut self, sect: usize, off: u32) {
        let insn = self.insn(sect, off);
        let insn = if insn & 0xff80_0000 == 0x9100_0000 {
            0xf940_0000 | (insn & 0x3ff)
        } else {
            insn & 0xffc0_03ff
        };
        self.set_insn(sect, off, insn);
    }

    /// An eh_frame record's fixups, applied in place: the linker reads
    /// the records' pointers as values in the object (see
    /// input_files::apply_eh_frame_relocs), leaving only a CIE's
    /// personality reference for a GOT relocation.
    fn apply_cfi_fixups(&mut self, i: usize) {
        let entry = &self.rec.entries[i];
        let (sect, entry_off) = self.place[i].unwrap();
        for f in &self.rec.fixups[entry.fixups.clone()] {
            let off = (entry_off + f.offset as u64) as u32;
            match f.kind {
                fk::DIFF32 | fk::DIFF64 => {
                    let val =
                        self.addr(f.target, f.addend).wrapping_sub(self.addr(f.from.unwrap(), 0));
                    let size = if f.kind == fk::DIFF64 { 8 } else { 4 };
                    self.put(sect, off, size, val);
                }
                fk::PCREL_DELTA32 => {
                    let here = self.sections[sect].addr + off as u64;
                    let val = self.addr(f.target, f.addend).wrapping_sub(here);
                    self.put(sect, off, 4, val);
                }
                fk::PCREL32_TO_GOT | fk::SWIFT_REL32_TO_GOT => {
                    let Some(sym) = self.target_sym(f.target) else { continue };
                    let swift = (f.kind == fk::SWIFT_REL32_TO_GOT) as i64;
                    let (ty, addend) = if Self::is_arm64() {
                        (ARM64_RELOC_POINTER_TO_GOT, f.addend + swift)
                    } else {
                        (X86_64_RELOC_GOT, f.addend + 4 + swift)
                    };
                    self.put(sect, off, 4, addend as u64);
                    self.sections[sect].relocs.push(Self::reloc(off, sym, ty, 2, true));
                }
                _ => {}
            }
        }
    }

    /// The debug notes of the units the entries came from, as an `ld -r`
    /// writes them: for each, an empty N_SO, N_SO with the source
    /// directory and name, N_OSO naming the object, then the notes of
    /// its symbols by address; and an empty N_SO closing the last.
    fn stabs(&self, strtab: &mut Strtab) -> Vec<MachSym> {
        let mut units: Vec<u16> = Vec::new();
        for entry in &self.rec.entries {
            if entry.debug != 0 && !units.contains(&entry.debug) {
                units.push(entry.debug);
            }
        }
        let so = |strtab: &mut Strtab, name: &[u8], sect: u8| MachSym {
            stroff: strtab.add(name),
            n_type: N_SO,
            sect,
            ..Default::default()
        };
        let mut out = Vec::new();
        for &unit in &units {
            let Some(info) = self.rec.debug_infos.get(unit as usize - 1) else { continue };
            out.push(so(strtab, b"", 1));
            out.push(so(strtab, &info.source_dir, 0));
            out.push(so(strtab, &info.source_name, 0));
            out.push(MachSym {
                stroff: strtab.add(&info.object_path),
                n_type: N_OSO,
                sect: self.rec.cpusubtype as u8,
                desc: 1,
                value: info.mtime as u64,
            });
            let mut notes: Vec<(u64, Vec<MachSym>)> = (0..self.rec.entries.len())
                .filter(|&i| self.rec.entries[i].debug == unit)
                .filter_map(|i| self.symbol_stabs(i, strtab))
                .collect();
            notes.sort_by_key(|(addr, _)| *addr);
            out.extend(notes.into_iter().flat_map(|(_, n)| n));
        }
        if !units.is_empty() {
            out.push(so(strtab, b"", 1));
        }
        out
    }

    /// The notes of an entry's symbol, as those of an object's with
    /// DWARF (see chunks::symtab's symbol_stabs), and the address to
    /// order them by: a function's N_FUN pair between N_BNSYM and
    /// N_ENSYM, an external variable's N_GSYM (a private external's
    /// too), a local one's N_STSYM, and a tentative definition's N_GSYM,
    /// last.
    fn symbol_stabs(&self, i: usize, strtab: &mut Strtab) -> Option<(u64, Vec<MachSym>)> {
        let sym = &self.symbols[self.sym_of[i]?];
        if crate::input_files::is_private_label(&sym.name) {
            return None;
        }
        let entry = |n_type, stroff, sect, value| MachSym { stroff, n_type, sect, desc: 0, value };
        let name = strtab.add(&sym.name);
        let (sect, offset) = match sym.place {
            SymPlace::Common { .. } => return Some((u64::MAX, vec![entry(N_GSYM, name, 0, 0)])),
            SymPlace::Undefined | SymPlace::Absolute(_) => return None,
            SymPlace::Defined { sect, offset } => (sect, offset),
        };
        let section = &self.sections[sect];
        let addr = section.addr + offset;
        let sect_idx = sect as u8 + 1;
        let notes = if section.flags & S_ATTR_PURE_INSTRUCTIONS != 0 {
            let empty = strtab.add(b"");
            vec![
                entry(N_BNSYM, empty, sect_idx, addr),
                entry(N_FUN, name, sect_idx, addr),
                entry(N_FUN, empty, 0, self.rec.entries[i].size as u64),
                entry(N_ENSYM, empty, sect_idx, addr),
            ]
        } else if sym.n_type & N_EXT != 0 {
            vec![entry(N_GSYM, name, 0, 0)]
        } else {
            vec![entry(N_STSYM, name, sect_idx, addr)]
        };
        Some((addr, notes))
    }

    /// The symbol table: the locals, then the debug notes, then the
    /// defined externals, then the undefined ones and the tentative
    /// definitions, each kind in the order made.
    fn symbol_table(&self) -> SymbolTable {
        let kind = |s: &Symbol| match s.place {
            SymPlace::Undefined | SymPlace::Common { .. } => 2,
            SymPlace::Defined { .. } | SymPlace::Absolute(_) if s.n_type & N_EXT != 0 => 1,
            SymPlace::Defined { .. } | SymPlace::Absolute(_) => 0,
        };
        let mut order: Vec<usize> = (0..self.symbols.len()).collect();
        order.sort_by_key(|&i| kind(&self.symbols[i]));
        let mut table = SymbolTable {
            mach_syms: Vec::new(),
            strtab: Strtab::new(),
            index: vec![0; self.symbols.len()],
            nlocal: 0,
            nextdef: 0,
        };
        let nlocal = order.iter().take_while(|&&i| kind(&self.symbols[i]) == 0).count();
        for &i in &order[..nlocal] {
            self.push_symbol(&mut table, i);
        }
        let stabs = self.stabs(&mut table.strtab);
        table.mach_syms.extend(stabs);
        table.nlocal = table.mach_syms.len();
        for &i in &order[nlocal..] {
            self.push_symbol(&mut table, i);
        }
        table.nextdef = order.iter().filter(|&&i| kind(&self.symbols[i]) == 1).count();
        table
    }

    fn push_symbol(&self, table: &mut SymbolTable, i: usize) {
        let s = &self.symbols[i];
        let mut n = MachSym {
            stroff: table.strtab.add(&s.name),
            n_type: s.n_type,
            sect: 0,
            desc: s.desc,
            value: 0,
        };
        match s.place {
            SymPlace::Defined { sect, offset } => {
                n.n_type |= N_SECT;
                n.sect = sect as u8 + 1;
                n.value = self.sections[sect].addr + offset;
            }
            SymPlace::Undefined => {}
            SymPlace::Common { size, p2align } => {
                n.value = size;
                n.desc |= (p2align as u16 & 0xf) << 8;
            }
            SymPlace::Absolute(value) => {
                n.n_type |= N_ABS;
                n.value = value;
            }
        }
        table.index[i] = table.mach_syms.len() as u32;
        table.mach_syms.push(n);
    }

    /// Writes the object out: the header and load commands (see
    /// write_load_commands), the sections' bytes, their relocations, the
    /// symbols and their names.
    fn write(self) -> Vec<u8> {
        let mut out = vec![0u8; size_of::<MachHeader>() + self.load_commands_size()];

        // The sections' bytes, which make up the segment; none of zero
        // fill.
        let mut sect_offs = Vec::with_capacity(self.sections.len());
        for s in &self.sections {
            if is_zerofill(s.flags) {
                sect_offs.push(0);
                continue;
            }
            let off = out.len().next_multiple_of(1 << s.p2align.min(12));
            out.resize(off, 0);
            out.extend_from_slice(&s.data);
            sect_offs.push(off as u32);
        }
        let seg_fileoff = sect_offs.iter().copied().filter(|&o| o != 0).min().unwrap_or(0) as u64;
        let seg_filesize = out.len() as u64 - seg_fileoff;
        out.resize(out.len().next_multiple_of(8), 0);

        // The relocations, which refer to the symbols by their indices
        // in the table.
        let table = self.symbol_table();
        let mut reloc_offs = Vec::with_capacity(self.sections.len());
        for s in &self.sections {
            reloc_offs.push(out.len() as u32);
            for r in &s.relocs {
                let mut r = *r;
                if r.is_extern() {
                    r.bits = (r.bits & !0xff_ffff) | table.index[r.idx() as usize];
                }
                out.extend_from_slice(r.as_bytes());
            }
        }

        let symoff = out.len().next_multiple_of(8);
        out.resize(symoff, 0);
        for n in &table.mach_syms {
            out.extend_from_slice(n.as_bytes());
        }
        let stroff = out.len();
        out.extend_from_slice(&table.strtab.data);
        out.resize(out.len().next_multiple_of(8), 0);

        let layout =
            FileLayout { sect_offs, reloc_offs, seg_fileoff, seg_filesize, symoff, stroff };
        self.write_load_commands(&mut out, &layout, &table);
        out
    }

    /// The size of the load commands: a segment of all the sections, the
    /// build version and the symbol tables.
    fn load_commands_size(&self) -> usize {
        size_of::<SegmentCommand>()
            + self.sections.len() * size_of::<MachSection>()
            + size_of::<BuildVersionCommand>()
            + size_of::<SymtabCommand>()
            + size_of::<DysymtabCommand>()
    }

    /// Writes the Mach header and the load commands.
    fn write_load_commands(&self, buf: &mut [u8], layout: &FileLayout, table: &SymbolTable) {
        let mut off = 0;
        let mut put = |bytes: &[u8]| {
            buf[off..off + bytes.len()].copy_from_slice(bytes);
            off += bytes.len();
        };
        let header = MachHeader {
            magic: MH_MAGIC_64,
            cputype: self.rec.cputype,
            cpusubtype: self.rec.cpusubtype,
            filetype: MH_OBJECT,
            ncmds: 4,
            sizeofcmds: self.load_commands_size() as u32,
            flags: MH_SUBSECTIONS_VIA_SYMBOLS,
            reserved: 0,
        };
        put(header.as_bytes());
        let nsects = self.sections.len();
        let segment = SegmentCommand {
            cmd: LC_SEGMENT_64,
            cmdsize: (size_of::<SegmentCommand>() + nsects * size_of::<MachSection>()) as u32,
            vmsize: self.sections.iter().map(|s| s.addr + s.size).max().unwrap_or(0),
            fileoff: layout.seg_fileoff,
            filesize: layout.seg_filesize,
            maxprot: VM_PROT_READ | VM_PROT_WRITE | VM_PROT_EXECUTE,
            initprot: VM_PROT_READ | VM_PROT_WRITE | VM_PROT_EXECUTE,
            nsects: nsects as u32,
            ..Default::default()
        };
        put(segment.as_bytes());
        for (i, s) in self.sections.iter().enumerate() {
            let hdr = MachSection {
                sectname: s.sectname,
                segname: s.segname,
                addr: s.addr,
                size: s.size,
                offset: layout.sect_offs[i],
                p2align: s.p2align as u32,
                reloff: if s.relocs.is_empty() { 0 } else { layout.reloc_offs[i] },
                nreloc: s.relocs.len() as u32,
                flags: s.flags,
                ..Default::default()
            };
            put(hdr.as_bytes());
        }
        let build = BuildVersionCommand {
            cmd: LC_BUILD_VERSION,
            cmdsize: size_of::<BuildVersionCommand>() as u32,
            platform: self.rec.platform,
            minos: self.rec.minos,
            sdk: self.rec.sdk,
            ntools: 0,
        };
        put(build.as_bytes());
        let symtab = SymtabCommand {
            cmd: LC_SYMTAB,
            cmdsize: size_of::<SymtabCommand>() as u32,
            symoff: layout.symoff as u32,
            nsyms: table.mach_syms.len() as u32,
            stroff: layout.stroff as u32,
            strsize: table.strtab.data.len() as u32,
        };
        put(symtab.as_bytes());
        let dysymtab = DysymtabCommand {
            cmd: LC_DYSYMTAB,
            cmdsize: size_of::<DysymtabCommand>() as u32,
            ilocalsym: 0,
            nlocalsym: table.nlocal as u32,
            iextdefsym: table.nlocal as u32,
            nextdefsym: table.nextdef as u32,
            iundefsym: (table.nlocal + table.nextdef) as u32,
            nundefsym: (table.mach_syms.len() - table.nlocal - table.nextdef) as u32,
            ..Default::default()
        };
        put(dysymtab.as_bytes());
    }
}

/// Where the parts of the object lie in the file: each section's bytes
/// and relocations, the segment the bytes make up, and the symbol and
/// string tables.
struct FileLayout {
    sect_offs: Vec<u32>,
    reloc_offs: Vec<u32>,
    seg_fileoff: u64,
    seg_filesize: u64,
    symoff: usize,
    stroff: usize,
}

/// The symbol table of the object being made.
struct SymbolTable {
    mach_syms: Vec<MachSym>,
    strtab: Strtab,
    /// Each symbol's index in `mach_syms`.
    index: Vec<u32>,
    /// The locals and the debug notes.
    nlocal: usize,
    nextdef: usize,
}

/// A string table, starting as ld64's do with " \0".
struct Strtab {
    data: Vec<u8>,
    index: hashbrown::HashMap<Vec<u8>, u32>,
}

impl Strtab {
    fn new() -> Self {
        Self { data: b" \0".to_vec(), index: hashbrown::HashMap::new() }
    }

    fn add(&mut self, s: &[u8]) -> u32 {
        if s.is_empty() {
            return 1;
        }
        if let Some(&i) = self.index.get(s) {
            return i;
        }
        let i = self.data.len() as u32;
        self.data.extend_from_slice(s);
        self.data.push(0);
        self.index.insert(s.to_vec(), i);
        i
    }
}

/// An entry's scope as a MachSym's type and description bits.
fn scope_bits(scope: u8) -> (u8, u16) {
    match scope {
        scope::HIDDEN => (N_EXT | N_PEXT, 0),
        scope::AUTO_HIDE => (N_EXT, N_WEAK_DEF | N_WEAK_REF),
        scope::GLOBAL => (N_EXT, 0),
        scope::NEVER_STRIP => (N_EXT, REFERENCED_DYNAMICALLY),
        _ => (0, 0),
    }
}
