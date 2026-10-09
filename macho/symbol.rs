//! Symbols and the global symbol table.

use rayon::prelude::*;

use crate::arch::Target;
use crate::chunks::ChunkId;
use crate::context::Context;
use crate::input_files::FileId;

/// A symbol index, u32 as in mold: every per-symbol and
/// per-MachSym vector of ids is half the size of a usize one.
pub type SymbolId = u32;

/// A symbol's owning file in one u32 - none, an object index, or a
/// dylib index with the DYLIB bit - mold's SymbolFile. The
/// dynamic-lookup import (FileId::Dylib(u32::MAX): a dylib index
/// naming no dylib) packs as the one value the bit and NONE leave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SymbolFile(u32);

impl SymbolFile {
    const DYLIB: u32 = 1 << 31;
    const NONE: u32 = u32::MAX;
    const DYNAMIC_LOOKUP: u32 = u32::MAX - 1;

    #[inline]
    fn none() -> Self {
        Self(Self::NONE)
    }

    #[inline]
    fn some(file: FileId) -> Self {
        Self(match file {
            FileId::Obj(i) => {
                debug_assert!(i < Self::DYLIB);
                i
            }
            FileId::Dylib(u32::MAX) => Self::DYNAMIC_LOOKUP,
            FileId::Dylib(i) => {
                debug_assert!(i < Self::DYLIB - 1);
                i | Self::DYLIB
            }
        })
    }

    #[inline]
    fn get(self) -> Option<FileId> {
        match self.0 {
            Self::NONE => None,
            Self::DYNAMIC_LOOKUP => Some(FileId::Dylib(u32::MAX)),
            raw if raw & Self::DYLIB != 0 => Some(FileId::Dylib(raw & !Self::DYLIB)),
            raw => Some(FileId::Obj(raw)),
        }
    }
}

#[derive(Debug)]
pub struct Symbol {
    /// The name, as a pointer and a u32 length rather than a 16-byte
    /// slice - mold's name_ptr/name_len. Read through name().
    name_ptr: usize,
    name_len: u32,
    /// The owning file - the object or dylib that defines the symbol -
    /// or none while it is undefined. Read through file().
    file: SymbolFile,
    /// The defining subsection for an `N_SECT` symbol, or NONE. Read
    /// through isec(); index ctx.isecs with `as usize`.
    isec: u32,
    /// Offset from the start of `isec`, or the absolute value for `N_ABS`
    /// symbols.
    pub value: u64,
    /// Index into the sparse SymbolAux table (SymbolTable's aux), or
    /// NONE: only symbols with a stub/GOT/TLV/objc slot have an entry -
    /// mold's aux_idx - instead of 16 bytes per symbol whether needed or
    /// not. Read through aux().
    aux_idx: u32,
    /// The NEEDS_* flags scan_relocations sets, in parallel, and then
    /// turns into stubs and GOT slots - mold's flags.
    flags: std::sync::atomic::AtomicU8,
    /// The boolean attributes, packed into one atomic word as mold
    /// keeps its Symbol bits: the eight is_* bits, read with plain
    /// loads and written through &mut without an atomic operation, plus
    /// the MARK bit that parallel passes set with a compare-and-swap
    /// (thunk creation dedups its entries that way, in the scan itself).
    bits: std::sync::atomic::AtomicU16,
    pub common_p2align: u8,
}

// Symbol is loaded in every resolution and layout scan, so its width
// is kept minimal. mold's is 48 with more fields (a version index,
// a symbol index); ours packs the same way and lands at 40.
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<Symbol>() == 40);

/// "No index" for `isec` and `aux_idx`.
pub const NONE: u32 = u32::MAX;

/// Symbol flags set while scanning relocations.
pub const NEEDS_GOT: u8 = 1 << 0;
pub const NEEDS_STUB: u8 = 1 << 1;

impl Symbol {
    pub(crate) fn new(name: &'static [u8]) -> Self {
        Self {
            name_ptr: name.as_ptr() as usize,
            name_len: u32::try_from(name.len()).expect("symbol name is larger than 4 GiB"),
            file: SymbolFile::none(),
            isec: NONE,
            value: 0,
            aux_idx: NONE,
            flags: std::sync::atomic::AtomicU8::new(0),
            bits: std::sync::atomic::AtomicU16::new(0),
            common_p2align: 0,
        }
    }

    /// The name, the bytes the string table holds: any but NUL, UTF-8
    /// or not, as a symbol name is to ld-prime (and to mold).
    #[inline]
    pub fn name(&self) -> &'static [u8] {
        // SAFETY: name_ptr/name_len are exactly the bytes of the
        // &'static [u8] the symbol was created with.
        unsafe { std::slice::from_raw_parts(self.name_ptr as *const u8, self.name_len as usize) }
    }

    /// The file that owns the symbol: the object or dylib whose
    /// definition won resolution, or None while it is undefined.
    #[inline]
    pub fn file(&self) -> Option<FileId> {
        self.file.get()
    }

    #[inline]
    pub fn set_file(&mut self, file: FileId) {
        self.file = SymbolFile::some(file);
    }

    #[inline]
    pub fn clear_file(&mut self) {
        self.file = SymbolFile::none();
    }

    pub fn is_defined(&self) -> bool {
        self.file.get().is_some()
    }

    #[inline]
    pub fn input_section(&self) -> Option<u32> {
        (self.isec != NONE).then_some(self.isec)
    }

    #[inline]
    pub fn set_input_section(&mut self, isec: Option<u32>) {
        self.isec = isec.unwrap_or(NONE);
    }
}

const F_EXTERN: u16 = 1 << 0;
const F_WEAK_DEF: u16 = 1 << 1;
const F_IMPORTED: u16 = 1 << 2;
const F_USED: u16 = 1 << 3;
const F_PRIVATE_EXTERN: u16 = 1 << 4;
const F_WEAK_REF: u16 = 1 << 5;
const F_NO_DEAD_STRIP: u16 = 1 << 6;
const F_COMMON: u16 = 1 << 7;
/// Set by mark(), a transient per-pass flag (mold's IS_MARKED).
const F_MARK: u16 = 1 << 8;
/// Referenced non-weakly by some object: with ld64's default
/// -weak_reference_mismatches non-weak, that makes the import strong
/// whatever other references say.
const F_STRONG_REF: u16 = 1 << 9;
/// Defined, non-weak, in a section, with REFERENCED_DYNAMICALLY in its
/// object, which the output's entry keeps.
const F_REFERENCED_DYNAMICALLY: u16 = 1 << 10;
/// Defined as an alternate entry point (N_ALT_ENTRY): a label inside
/// another's subsection, which names none.
const F_ALT_ENTRY: u16 = 1 << 11;

macro_rules! sym_flag {
    ($get:ident, $set:ident, $bit:expr, $doc:expr) => {
        #[doc = $doc]
        #[inline]
        pub fn $get(&self) -> bool {
            self.bits.load(std::sync::atomic::Ordering::Relaxed) & $bit != 0
        }
        #[inline]
        pub fn $set(&mut self, v: bool) {
            let f = self.bits.get_mut();
            if v {
                *f |= $bit;
            } else {
                *f &= !$bit;
            }
        }
    };
}

impl Symbol {
    sym_flag!(is_extern, set_extern, F_EXTERN, "An external (global) symbol.");
    sym_flag!(is_weak_def, set_weak_def, F_WEAK_DEF, "A weak definition.");
    sym_flag!(
        is_imported,
        set_imported,
        F_IMPORTED,
        "The definition is in a dylib, so references need dynamic binding."
    );
    sym_flag!(
        is_used,
        set_used,
        F_USED,
        "Some relocation refers to this symbol, so an unresolved symbol is an error."
    );
    sym_flag!(
        is_private_extern,
        set_private_extern,
        F_PRIVATE_EXTERN,
        "A private external symbol (visibility hidden): resolves globally at link time but is neither exported nor kept as an external symbol."
    );
    sym_flag!(is_strong_ref, set_strong_ref, F_STRONG_REF, "Referenced non-weakly by some object.");
    sym_flag!(
        is_weak_ref,
        set_weak_ref,
        F_WEAK_REF,
        "References may go unresolved at load time (a weak import)."
    );
    sym_flag!(
        no_dead_strip,
        set_no_dead_strip,
        F_NO_DEAD_STRIP,
        "The symbol must survive dead-stripping."
    );
    sym_flag!(
        is_referenced_dynamically,
        set_referenced_dynamically,
        F_REFERENCED_DYNAMICALLY,
        "Defined with REFERENCED_DYNAMICALLY (see F_REFERENCED_DYNAMICALLY)."
    );
    sym_flag!(
        is_alt_entry,
        set_alt_entry,
        F_ALT_ENTRY,
        "Defined as an alternate entry point (see F_ALT_ENTRY)."
    );
    sym_flag!(
        is_common,
        set_common,
        F_COMMON,
        "A tentative definition (common symbol) not yet converted; `value` holds its size."
    );

    #[inline]
    pub fn flags(&self) -> u8 {
        self.flags.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Sets NEEDS_* flags. Many relocations refer to the same symbols,
    /// so a flag already set is not written again, which would contend
    /// for the cache line, as in mold.
    #[inline]
    pub fn add_flags(&self, flags: u8) {
        use std::sync::atomic::Ordering::Relaxed;
        if self.flags.load(Relaxed) & flags != flags {
            self.flags.fetch_or(flags, Relaxed);
        }
    }

    #[inline]
    pub fn clear_flags(&self) {
        self.flags.store(0, std::sync::atomic::Ordering::Relaxed);
    }

    /// Atomically sets the transient mark; true if it was clear (the
    /// caller won the race to claim this symbol).
    #[inline]
    pub fn mark(&self) -> bool {
        use std::sync::atomic::Ordering::Relaxed;
        self.bits.fetch_or(F_MARK, Relaxed) & F_MARK == 0
    }
    #[inline]
    pub fn unmark(&self) {
        self.bits.fetch_and(!F_MARK, std::sync::atomic::Ordering::Relaxed);
    }
    #[inline]
    pub fn is_marked(&self) -> bool {
        self.bits.load(std::sync::atomic::Ordering::Relaxed) & F_MARK != 0
    }

    /// The symbol's synthetic-slot indices, from the side table; None
    /// for the vast majority of symbols, which take no slot (the entry
    /// is made by the first SymbolTable::aux_mut).
    #[inline]
    pub fn aux<'a>(&self, symbols: &'a SymbolTable) -> Option<&'a SymbolAux> {
        (self.aux_idx != NONE).then(|| &symbols.aux[self.aux_idx as usize])
    }

    #[inline]
    pub fn stub_idx(&self, symbols: &SymbolTable) -> Option<u32> {
        let idx = self.aux(symbols)?.stub_idx;
        (idx != NO_IDX).then_some(idx)
    }

    #[inline]
    pub fn got_idx(&self, symbols: &SymbolTable) -> Option<u32> {
        let idx = self.aux(symbols)?.got_idx;
        (idx != NO_IDX).then_some(idx)
    }

    #[inline]
    pub fn objc_stub_idx(&self, symbols: &SymbolTable) -> Option<u32> {
        let idx = self.aux(symbols)?.objc_stub_idx;
        (idx != NO_IDX).then_some(idx)
    }

    #[inline]
    pub fn lazy_stub_idx(&self, symbols: &SymbolTable) -> Option<u32> {
        let idx = self.aux(symbols)?.lazy_stub_idx;
        (idx != NO_IDX).then_some(idx)
    }

    #[inline]
    pub fn lazy_got_idx(&self, symbols: &SymbolTable) -> Option<u32> {
        let idx = self.aux(symbols)?.lazy_got_idx;
        (idx != NO_IDX).then_some(idx)
    }

    #[inline]
    pub fn delay_stub_idx(&self, symbols: &SymbolTable) -> Option<u32> {
        let idx = self.aux(symbols)?.delay_stub_idx;
        (idx != NO_IDX).then_some(idx)
    }

    /// Whether calls to the symbol go through a stub of any kind: its
    /// __stubs entry, or for a lazily loaded or delay-init import, its
    /// call helper or delay-init stub - mold's has_plt.
    #[inline]
    pub fn has_stub(&self, symbols: &SymbolTable) -> bool {
        self.aux(symbols).is_some_and(|a| {
            a.stub_idx != NO_IDX || a.lazy_stub_idx != NO_IDX || a.delay_stub_idx != NO_IDX
        })
    }

    #[inline]
    pub fn has_got(&self, symbols: &SymbolTable) -> bool {
        self.got_idx(symbols).is_some()
    }

    /// True for a symbol of a dylib whose initializers wait for the
    /// image's first use of it (see delay_init::create_delay_init).
    pub fn is_delay_import<E: Target>(&self, ctx: &Context<E>) -> bool {
        match self.file() {
            Some(FileId::Dylib(d)) => d != u32::MAX && ctx.dylibs[d as usize].delay_init.is_some(),
            _ => false,
        }
    }

    /// True for a symbol of a dylib dyld loads lazily (see
    /// lazy_load::create_lazy_loads).
    pub fn is_lazy_import<E: Target>(&self, ctx: &Context<E>) -> bool {
        match self.file() {
            Some(FileId::Dylib(d)) => d != u32::MAX && ctx.dylibs[d as usize].is_lazy,
            _ => false,
        }
    }

    /// True if the symbol resolves to a TLV descriptor: a definition in a
    /// S_THREAD_LOCAL_VARIABLES section, or a dylib export listed as
    /// thread-local - sold's Symbol::is_tlv. Symbols left to runtime
    /// lookup pass as either.
    pub fn is_tlv<E: Target>(&self, ctx: &Context<E>) -> bool {
        match self.file() {
            Some(FileId::Obj(_)) => self.input_section().is_some_and(|isec| {
                let isec = &ctx.isecs[isec as usize];
                isec.hdr(&ctx.objs[isec.file as usize]).section_type()
                    == crate::macho::S_THREAD_LOCAL_VARIABLES
            }),
            Some(FileId::Dylib(d)) => {
                d != u32::MAX && ctx.dylibs[d as usize].tlv_exports.contains(self.name())
            }
            _ => false,
        }
    }

    /// True for a weak definition of this image that dyld may replace
    /// with another image's copy at load time: an exported (neither
    /// private nor auto-hidden) weak definition from an object. ld64
    /// routes every reference to such a symbol through a slot dyld
    /// binds by weak lookup - a GOT entry, a stub, a data pointer -
    /// so that C++'s one-definition rule holds across images (an
    /// inline function's static local is one variable, not one per
    /// dylib). In a relocatable output the references stay relocations.
    /// Nor does another image's copy replace one of dyld's own: dyld
    /// fixes itself up before it loads any image, and by rebases alone.
    /// (ld-prime reaches them directly from code too, but binds a
    /// pointer to one in data by weak lookup, a bind dyld could not
    /// carry out.)
    pub fn is_weak_coalesced<E: Target>(&self, ctx: &Context<E>) -> bool {
        if ctx.args.relocatable || ctx.args.is_dylinker() {
            return false;
        }
        matches!(self.file(), Some(FileId::Obj(_)))
            && self.is_weak_def()
            && self.is_extern()
            && !self.is_private_extern()
    }

    /// True for a live weak definition the image exports, which another
    /// image's copy may replace at load time (and so for which ld-prime
    /// sets MH_WEAK_DEFINES): not an auto-hidden or private extern one,
    /// nor one -dead_strip removed.
    pub fn exports_weak_def<E: Target>(&self, ctx: &Context<E>) -> bool {
        self.is_weak_def()
            && self.is_extern()
            && !self.is_private_extern()
            && self.input_section().is_some_and(|isec| ctx.isecs[isec as usize].is_alive())
    }

    /// True for a definition the image exports that dyld binds the
    /// image's own references to by name, as it binds imports, so that
    /// another image can interpose it: each export of a -flat_namespace
    /// dylib or bundle, by flat lookup (an image loaded before it may),
    /// and one -interposable or -interposable_list names in any image
    /// dyld loads (but dyld), to the image itself. ld64 calls it through
    /// a stub, loads it from a GOT slot and binds the pointers to it in
    /// data (initializer and Objective-C metadata pointers too) instead
    /// of rebasing them. ld-prime binds a weak definition so too,
    /// besides by weak lookup (with chained fixups by weak lookup
    /// alone). A -flat_namespace executable's references to its own
    /// definitions stay direct - it comes first in the flat search
    /// order anyway - as do dyld's (ld-prime crashes linking one) and
    /// those to a sectionless symbol: an absolute one, or one that marks
    /// the image's layout.
    pub fn is_interposable_export<E: Target>(&self, ctx: &Context<E>) -> bool {
        let args = &ctx.args;
        let flat = args.flat_namespace
            && matches!(args.output_type, crate::macho::MH_DYLIB | crate::macho::MH_BUNDLE);
        let listed = !args.relocatable
            && !args.without_dyld()
            && !args.is_dylinker()
            && args.interposable.as_ref().is_some_and(|g| g.find(self.name()) != -1);
        (flat || listed)
            && matches!(self.file(), Some(FileId::Obj(_)))
            && self.input_section().is_some()
            && self.is_extern()
            && !self.is_private_extern()
    }

    /// True if dyld binds the slots referring to this symbol as it
    /// binds an import's, by name from the bind stream: an import, or an
    /// interposable export.
    pub fn binds_as_import<E: Target>(&self, ctx: &Context<E>) -> bool {
        self.is_imported() || self.is_interposable_export(ctx)
    }

    /// True if dyld binds a pointer in data to this symbol rather than
    /// sliding it: one to an import's (see binds_as_import), or, in
    /// legacy LINKEDIT (Args::legacy_linkedit), to one of the image's
    /// coalescable weak definitions, which an external relocation
    /// binds by name. LC_DYLD_INFO slides that one and weak-binds it.
    pub fn binds_pointer<E: Target>(&self, ctx: &Context<E>) -> bool {
        self.binds_as_import(ctx) || (ctx.args.legacy_linkedit && self.is_weak_coalesced(ctx))
    }

    /// The library ordinal a bind of this symbol names: its dylib's
    /// (see DylibFile::bind_ordinal), the flat lookup for an import
    /// left to dynamic lookup, or for an interposable export, the flat
    /// lookup under -flat_namespace, else the image itself.
    pub fn bind_ordinal<E: Target>(&self, ctx: &Context<E>) -> i32 {
        match self.file() {
            Some(FileId::Dylib(d)) if d != u32::MAX => {
                ctx.dylibs[d as usize].bind_ordinal(ctx.args.flat_namespace)
            }
            Some(FileId::Dylib(_)) => crate::macho::BIND_SPECIAL_DYLIB_FLAT_LOOKUP,
            _ if self.is_dtrace_pointer_target() => crate::macho::BIND_SPECIAL_DYLIB_FLAT_LOOKUP,
            _ => self.export_bind_ordinal(ctx),
        }
    }

    /// The library ordinal this symbol binds with as an interposable
    /// export.
    pub fn export_bind_ordinal<E: Target>(&self, ctx: &Context<E>) -> i32 {
        match ctx.args.flat_namespace {
            true => crate::macho::BIND_SPECIAL_DYLIB_FLAT_LOOKUP,
            false => crate::macho::BIND_SPECIAL_DYLIB_SELF,
        }
    }

    /// True for a definition of this image that dyld may replace with
    /// another image's at load time, so that its references go through
    /// slots dyld binds and its calls through its stub: a weak
    /// definition subject to coalescing, or an interposable export.
    pub fn is_interposable<E: Target>(&self, ctx: &Context<E>) -> bool {
        self.is_weak_coalesced(ctx) || self.is_interposable_export(ctx)
    }

    /// True for a DTrace symbol (see dtrace), never defined, which a
    /// pointer in data binds by flat lookup, as ld-prime has it: no
    /// import, it takes no stub, GOT slot or symbol table entry.
    pub fn is_dtrace_pointer_target(&self) -> bool {
        self.file().is_none() && crate::dtrace::is_dtrace_symbol(self.name())
    }

    /// True if dyld fills the references to this symbol: an import, or
    /// a definition it may interpose.
    pub fn binds_at_runtime<E: Target>(&self, ctx: &Context<E>) -> bool {
        self.is_imported() || self.is_interposable(ctx)
    }

    /// An input's N_ABS definition has no section and never slides.
    /// Sectionless symbols in the internal object instead describe the
    /// image (its header and layout boundaries), so their values slide.
    pub fn is_absolute<E: Target>(&self, ctx: &Context<E>) -> bool {
        self.input_section().is_none()
            && matches!(self.file(), Some(FileId::Obj(obj)) if !ctx.is_internal(obj as usize))
    }

    /// A GOT load relaxes to a PC-relative address computation unless
    /// dyld fills the slot - an import or an interposable export, or a
    /// weak definition it binds by weak lookup, which a -static image's
    /// code never is - or the target is an absolute constant: the
    /// instruction slides but the value does not.
    pub fn can_relax_got<E: Target>(&self, ctx: &Context<E>) -> bool {
        !self.binds_as_import(ctx) && !self.binds_weak_lookup(ctx) && !self.is_absolute(ctx)
    }

    /// True for a definition this image exports that some dylib in the
    /// link exports as a weak definition: the program's own operator
    /// new overriding libc++'s. dyld must let it win coalescing, so
    /// the image is marked WEAK_DEFINES and, with classic dyld info,
    /// the symbol is listed in the weak_bind stream as a non-weak
    /// definition (ld64 does both).
    pub fn overrides_weak_export<E: Target>(&self, ctx: &Context<E>) -> bool {
        matches!(self.file(), Some(FileId::Obj(_)))
            && self.is_extern()
            && !self.is_private_extern()
            && !self.is_weak_def()
            && ctx.dylibs.iter().any(|d| d.weak_exports.contains(self.name()))
    }

    /// True if dyld resolves this symbol by weak lookup - searching
    /// every loaded image for the coalesced definition - rather than
    /// in one dylib: a coalescable weak definition of this image, or
    /// an import that its dylib exports as a weak definition (libc++'s
    /// operator new and delete, which a program may override). ld64
    /// binds both with library ordinal -3, never lazily, and lists
    /// them in the classic weak_bind stream.
    pub fn binds_weak_lookup<E: Target>(&self, ctx: &Context<E>) -> bool {
        // A static image has no dyld to perform runtime weak lookup, so a
        // call to a weakly-defined symbol in the image binds directly.
        // ld64 emits neither a stub nor a weak bind for it (the stock
        // XNU kernel has no stubs and an empty weak bind table). Nor
        // does a kext's but in the shared region, where it calls and
        // takes its weak definitions through the GOT as ld-prime links
        // an arm64 kext.
        if ctx.args.static_link || (ctx.args.is_kext() && !ctx.args.shared_region) {
            return false;
        }
        if self.is_weak_coalesced(ctx) {
            return true;
        }
        match self.file() {
            Some(FileId::Dylib(d)) if d != u32::MAX => {
                ctx.dylibs[d as usize].weak_exports.contains(self.name())
            }
            _ => false,
        }
    }

    /// True for an exported Objective-C class (or metaclass) of a dylib
    /// bound for the shared region, whose pointers ld-prime writes as
    /// binds to the image itself rather than rebases, with chained
    /// fixups: the cache builder may redirect them to a class that
    /// replaces this one.
    pub fn binds_to_self<E: Target>(&self, ctx: &Context<E>) -> bool {
        ctx.args.shared_region
            && ctx.args.output_type == crate::macho::MH_DYLIB
            && ctx.use_chained_fixups()
            && matches!(self.file(), Some(FileId::Obj(_)))
            && self.is_extern()
            && !self.is_private_extern()
            && (self.name().starts_with(b"_OBJC_CLASS_$_")
                || self.name().starts_with(b"_OBJC_METACLASS_$_"))
    }

    /// Returns the output address of the symbol.
    pub fn addr<E: Target>(&self, ctx: &Context<E>) -> u64 {
        match self.file() {
            // A DTrace symbol, never defined, is at address 0 for
            // ld-prime: where a branch that is no probe site goes.
            None => {
                if !crate::dtrace::is_dtrace_symbol(self.name()) {
                    crate::error!("undefined symbol: {self}");
                }
                0
            }
            Some(FileId::Obj(_)) => {
                if let Some(isec) = self.input_section().map(|i| i as usize) {
                    ctx.isecs[isec].addr(ctx) + self.value
                } else if let Some((_, addr)) = self.stub_entry(ctx) {
                    addr
                } else {
                    self.value
                }
            }
            // A branch to a dylib symbol goes through its stub, or for
            // a lazily loaded dylib's, its call helper. Other
            // references to dylib symbols are filled in by dyld; the
            // relocation scan has already validated them.
            Some(FileId::Dylib(_)) => self.stub_entry(ctx).map_or(0, |(_, addr)| addr),
        }
    }

    /// The entry of the linker's code that stands in for a symbol with
    /// no section of its own, where Symbol::addr puts it: a dylib
    /// symbol's stub, or for a lazily loaded or delay-init dylib's, its
    /// call helper or delay-init stub; an _objc_msgSend$<selector>
    /// symbol's selector stub. The chunk, and the entry's address.
    /// LC_SEGMENT_SPLIT_INFO places such a symbol by it too.
    #[inline]
    pub fn stub_entry<E: Target>(&self, ctx: &Context<E>) -> Option<(ChunkId, u64)> {
        let symbols = &ctx.symbols;
        match self.file()? {
            FileId::Obj(_) => {
                let idx = self.objc_stub_idx(symbols)?;
                let addr =
                    ctx.objc_stubs.hdr.addr + crate::chunks::objc_stubs::entry_offset(ctx, idx);
                Some((ChunkId::ObjcStubs, addr))
            }
            FileId::Dylib(_) => {
                if self.stub_idx(symbols).is_some() {
                    Some((ChunkId::Stubs, self.stub_addr(ctx)))
                } else if let Some(idx) = self.lazy_stub_idx(symbols) {
                    Some((ChunkId::LazyHelpers, ctx.lazy_helpers.helper_addr(idx as usize)))
                } else {
                    let idx = self.delay_stub_idx(symbols)?;
                    Some((ChunkId::DelayStubs, ctx.delay_init.stub_addr::<E>(idx as usize)))
                }
            }
        }
    }

    /// Returns the address of the symbol's __got slot, or for a lazily
    /// loaded dylib's symbol, its __lazy_load_got slot.
    #[inline]
    pub fn got_addr<E: Target>(&self, ctx: &Context<E>) -> u64 {
        self.got_slot(ctx).1
    }

    /// The symbol's __got slot, or for a lazily loaded dylib's symbol,
    /// its __lazy_load_got slot: the chunk, and the slot's address.
    #[inline]
    pub fn got_slot<E: Target>(&self, ctx: &Context<E>) -> (ChunkId, u64) {
        match self.got_idx(&ctx.symbols) {
            Some(idx) => (ChunkId::Got, ctx.got.slot_addr(idx as usize)),
            None => {
                let idx = self.lazy_got_idx(&ctx.symbols).unwrap();
                (ChunkId::LazyLoadGot, ctx.lazy_load_got.slot_addr(idx))
            }
        }
    }

    /// The address of the pointer slot the symbol's stub, stub `i`,
    /// jumps through: its lazy pointer, or its GOT slot - mold's
    /// gotplt_addr. A weak definition of this image always goes
    /// through its GOT slot (the lazy binder cannot do weak lookup), as
    /// in ld64.
    pub fn stub_ptr_addr<E: Target>(&self, ctx: &Context<E>, i: usize) -> u64 {
        if ctx.args.lazy_binding && !self.binds_weak_lookup(ctx) {
            let slot = ctx.stubs.lazy.binary_search(&(i as u32)).unwrap();
            ctx.lazy_ptrs.slot_addr(slot)
        } else {
            self.got_addr(ctx)
        }
    }

    /// Returns the address of the symbol's __stubs entry - mold's
    /// plt_addr.
    pub fn stub_addr<E: Target>(&self, ctx: &Context<E>) -> u64 {
        let idx = self.stub_idx(&ctx.symbols).unwrap();
        ctx.stubs.hdr.addr + crate::chunks::stubs::entry_offset::<E>(idx)
    }

    /// The address a branch to the symbol targets: its stub when it
    /// has one and dyld may redirect it, else the symbol itself.
    pub fn branch_target_addr<E: Target>(&self, ctx: &Context<E>) -> u64 {
        if self.is_interposable(ctx) && self.stub_idx(&ctx.symbols).is_some() {
            self.stub_addr(ctx)
        } else {
            self.addr(ctx)
        }
    }

    /// The address of a thunk entry for the symbol that a branch at `pc`
    /// can reach, if it has one - mold's thunk_addr.
    #[inline]
    pub fn thunk_addr<E: Target>(&self, ctx: &Context<E>, pc: u64) -> Option<u64> {
        let range = (E::BRANCH_RANGE / 2) as i64;
        let addrs: &[u64] = self.aux(&ctx.symbols).map_or(&[], |a| &a.thunk_addrs);
        addrs.iter().copied().find(|&t| {
            let d = t.wrapping_sub(pc) as i64;
            (-range..range).contains(&d)
        })
    }
}

impl Clone for Symbol {
    fn clone(&self) -> Self {
        Self {
            name_ptr: self.name_ptr,
            name_len: self.name_len,
            file: self.file,
            isec: self.isec,
            value: self.value,
            aux_idx: self.aux_idx,
            flags: std::sync::atomic::AtomicU8::new(
                self.flags.load(std::sync::atomic::Ordering::Relaxed),
            ),
            bits: std::sync::atomic::AtomicU16::new(
                self.bits.load(std::sync::atomic::Ordering::Relaxed),
            ),
            common_p2align: self.common_p2align,
        }
    }
}

impl std::fmt::Display for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        crate::error::display_name(self.name()).fmt(f)
    }
}

/// Sentinel for a synthetic-slot index a symbol does not have.
pub const NO_IDX: u32 = u32::MAX;

/// A symbol's synthetic-slot indices (__stubs, __got, __objc_stubs,
/// and for a lazily loaded import __lazy_helpers and __lazy_load_got),
/// each `NO_IDX` when absent. Only the few symbols that take a slot
/// ever have one, so these live in a side table of SymbolTable -
/// mold's SymbolAux - keeping Symbol itself small, as it is loaded in
/// every symbol scan.
#[derive(Clone, Debug)]
pub struct SymbolAux {
    pub stub_idx: u32,
    pub got_idx: u32,
    pub objc_stub_idx: u32,
    /// A lazily loaded import's call helper (its entry in
    /// ctx.lazy_helpers) and its __lazy_load_got slot.
    pub lazy_stub_idx: u32,
    pub lazy_got_idx: u32,
    /// A delay-init import's __delay_stubs entry.
    pub delay_stub_idx: u32,
    /// The addresses of this symbol's range-extension thunk entries,
    /// sorted, so that applying an out-of-range branch can find the one
    /// within reach - mold's SymbolAux::thunk_addrs.
    pub thunk_addrs: Vec<u64>,
}

impl Default for SymbolAux {
    fn default() -> Self {
        Self {
            stub_idx: NO_IDX,
            got_idx: NO_IDX,
            objc_stub_idx: NO_IDX,
            lazy_stub_idx: NO_IDX,
            lazy_got_idx: NO_IDX,
            delay_stub_idx: NO_IDX,
            thunk_addrs: Vec::new(),
        }
    }
}

/// All symbols in this link. Global symbols are interned by name so that
/// all references to one name share a slot; local symbols get anonymous
/// slots of their own.
///
/// The name map is sharded by a hash of the name, following mold's
/// symbol table: keys carry their xxh3 hash, computed in parallel
/// while files are staged, the maps hash by passing that value
/// through, and gather resolves a whole link's worth of names
/// with the shards processed in parallel.
#[derive(Debug)]
pub struct SymbolTable {
    shards: Vec<ShardMap>,
    pub syms: Vec<Symbol>,
    /// The symbols' synthetic-slot indices, for only those that take a
    /// slot (see Symbol::aux), grown by aux_mut - mold's sparse
    /// SymbolAux side table.
    aux: Vec<SymbolAux>,
}

pub const NUM_SHARDS: usize = 64;

/// The hash of a symbol table key, which the sharded table and its
/// callers share. Computed once per name, at staging time when
/// possible.
pub fn hash_key(key: &[u8]) -> u64 {
    xxhash_rust::xxh3::xxh3_64(key)
}

fn shard_of(hash: u64) -> usize {
    (hash % NUM_SHARDS as u64) as usize
}

/// A map key that hashes by its precomputed hash and compares by the
/// name, as in mold.
#[derive(Clone, Copy, Debug)]
struct Key {
    hash: u64,
    key: &'static [u8],
}

impl PartialEq for Key {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash && self.key == other.key
    }
}
impl Eq for Key {}

impl std::hash::Hash for Key {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.hash);
    }
}

/// A key for lookups, which needn't outlive the lookup (mold's Query).
struct Query<'a> {
    hash: u64,
    key: &'a [u8],
}

impl std::hash::Hash for Query<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.hash);
    }
}

impl hashbrown::Equivalent<Key> for Query<'_> {
    fn equivalent(&self, key: &Key) -> bool {
        self.hash == key.hash && self.key == key.key
    }
}

#[derive(Default)]
struct PassThroughHasher(u64);

impl std::hash::Hasher for PassThroughHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, _: &[u8]) {
        unreachable!("keys hash by their precomputed hash")
    }
    fn write_u64(&mut self, hash: u64) {
        self.0 = hash;
    }
}

type ShardMap = hashbrown::HashMap<Key, SymbolId, std::hash::BuildHasherDefault<PassThroughHasher>>;

impl Default for SymbolTable {
    fn default() -> Self {
        Self {
            shards: (0..NUM_SHARDS).map(|_| ShardMap::default()).collect(),
            syms: Vec::new(),
            aux: Vec::new(),
        }
    }
}

impl SymbolTable {
    /// Returns the symbol for a global name, creating it if needed.
    pub fn intern(&mut self, name: &'static [u8]) -> SymbolId {
        let hash = hash_key(name);
        *self.shards[shard_of(hash)].entry(Key { hash, key: name }).or_insert_with(|| {
            self.syms.push(Symbol::new(name));
            (self.syms.len() - 1) as u32
        })
    }

    /// Returns the symbol for a global name if it exists.
    pub fn lookup(&self, name: &[u8]) -> Option<SymbolId> {
        let hash = hash_key(name);
        self.shards[shard_of(hash)].get(&Query { hash, key: name }).copied()
    }

    /// Creates an anonymous slot for a file-local symbol.
    pub fn add_local(&mut self, name: &'static [u8]) -> SymbolId {
        self.syms.push(Symbol::new(name));
        (self.syms.len() - 1) as u32
    }

    /// Mutable access to a symbol's slot indices, allocating its entry
    /// in the side table on first use. Called only from the serial
    /// slot-assignment passes.
    pub fn aux_mut(&mut self, id: SymbolId) -> &mut SymbolAux {
        let aux_idx = &mut self.syms[id as usize].aux_idx;
        if *aux_idx == NONE {
            *aux_idx = self.aux.len() as u32;
            self.aux.push(SymbolAux::default());
        }
        &mut self.aux[*aux_idx as usize]
    }

    /// Interns every (name, precomputed-hash) pair at once, returning
    /// ids aligned with the batch. Names are binned by shard without
    /// touching their bytes, the shards resolve their bins in
    /// parallel, new symbols take contiguous id ranges per shard, and
    /// one serial scatter hands the ids back. Ids depend only on
    /// input order and the hash, so links stay deterministic.
    pub fn gather(&mut self, batch: &[(&'static [u8], u64)]) -> Vec<SymbolId> {
        let mut bins: Vec<Vec<u32>> = vec![Vec::new(); NUM_SHARDS];
        for (i, &(_, hash)) in batch.iter().enumerate() {
            bins[shard_of(hash)].push(i as u32);
        }

        enum Resolved {
            Old(SymbolId),
            New(u32),
        }
        /// A shard's resolutions, and the names it saw for the first time
        /// with their hashes.
        type ShardResult = (Vec<(u32, Resolved)>, Vec<(&'static [u8], u64)>);
        let results: Vec<ShardResult> = self
            .shards
            .par_iter_mut()
            .zip(bins)
            .map(|(shard, bin)| {
                // Names first seen in this batch, with their hash, so
                // the insert pass below never re-hashes them; final ids
                // are assigned once the shards' ranges are known.
                let mut news: Vec<(&'static [u8], u64)> = Vec::new();
                let mut newmap: ShardMap = ShardMap::default();
                let mut out = Vec::with_capacity(bin.len());
                for i in bin {
                    let (name, hash) = batch[i as usize];
                    let key = Key { hash, key: name };
                    if let Some(&id) = shard.get(&key) {
                        out.push((i, Resolved::Old(id)));
                        continue;
                    }
                    let idx = *newmap.entry(key).or_insert_with(|| {
                        news.push((name, hash));
                        (news.len() - 1) as u32
                    });
                    out.push((i, Resolved::New(idx as u32)));
                }
                (out, news)
            })
            .collect();

        let mut bases = Vec::with_capacity(NUM_SHARDS);
        let mut base = self.syms.len();
        for (_, news) in &results {
            bases.push(base);
            base += news.len();
        }

        // Initialize the new symbols into their prefix-summed ranges in
        // parallel - the same disjoint-range contract integrate uses for
        // local symbols - so a debug link's millions of new globals are
        // constructed on all cores, not pushed one at a time.
        let old_len = self.syms.len();
        let total_new = base - old_len;
        crate::util::reserve_arena(&mut self.syms, total_new);
        {
            struct SlotPtr(*mut Symbol);
            unsafe impl Sync for SlotPtr {}
            let ptr = SlotPtr(self.syms.as_mut_ptr());
            let ptr = &ptr;
            results.par_iter().zip(&bases).for_each(|((_, news), &b)| {
                for (k, &(name, _)) in news.iter().enumerate() {
                    // SAFETY: [b, b+news.len()) ranges are disjoint
                    // across shards and lie within the reserved space.
                    unsafe { ptr.0.add(b + k).write(Symbol::new(name)) };
                }
            });
            // SAFETY: every slot in old_len..old_len+total_new was
            // written exactly once above.
            unsafe { self.syms.set_len(old_len + total_new) };
        }

        // Insert the new names with their final ids, reusing the hash
        // computed during staging (no re-hash here).
        self.shards.par_iter_mut().zip(&results).zip(&bases).for_each(
            |((shard, (_, news)), &b)| {
                for (k, &(name, hash)) in news.iter().enumerate() {
                    shard.insert(Key { hash, key: name }, (b + k) as u32);
                }
            },
        );

        // Scatter each batch entry's resolved id in parallel; every
        // batch index appears in exactly one shard's output list, so
        // the writes are disjoint.
        let mut ids = vec![0u32; batch.len()];
        {
            struct IdPtr(*mut u32);
            unsafe impl Sync for IdPtr {}
            let ptr = IdPtr(ids.as_mut_ptr());
            let ptr = &ptr;
            results.par_iter().zip(&bases).for_each(|((out, _), &b)| {
                for &(i, ref r) in out {
                    let v = match r {
                        Resolved::Old(id) => *id,
                        Resolved::New(k) => (b + *k as usize) as u32,
                    };
                    // SAFETY: each batch index i is produced by exactly
                    // one shard, so these writes never overlap.
                    unsafe { *ptr.0.add(i as usize) = v };
                }
            });
        }
        ids
    }
}

impl std::ops::Index<SymbolId> for SymbolTable {
    type Output = Symbol;
    #[inline]
    fn index(&self, id: SymbolId) -> &Symbol {
        &self.syms[id as usize]
    }
}

impl std::ops::IndexMut<SymbolId> for SymbolTable {
    #[inline]
    fn index_mut(&mut self, id: SymbolId) -> &mut Symbol {
        &mut self.syms[id as usize]
    }
}

impl std::ops::Index<usize> for SymbolTable {
    type Output = Symbol;
    #[inline]
    fn index(&self, id: usize) -> &Symbol {
        &self.syms[id]
    }
}

impl std::ops::IndexMut<usize> for SymbolTable {
    #[inline]
    fn index_mut(&mut self, id: usize) -> &mut Symbol {
        &mut self.syms[id]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// C++ template instantiations produce symbol names tens of
    /// thousands of bytes long (ClickHouse's exceed 65535); the cached
    /// length must hold them whole.
    #[test]
    fn accepts_long_symbol_name() {
        let name: &'static [u8] = Vec::leak(vec![b'x'; 65536]);
        let symbol = Symbol::new(name);
        assert_eq!(symbol.name().len(), 65536);
        assert_eq!(symbol.name(), name);
    }
}
