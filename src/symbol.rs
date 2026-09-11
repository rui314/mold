//! Symbols and the global symbol table.
//!
//! There is one [`Symbol`] per unique global symbol name plus one per local
//! symbol of each object file. All of them live in a single table, the
//! [`SymbolTable`], and are referred to by [`SymbolId`].

use std::fmt;
use std::hash::{BuildHasherDefault, Hash, Hasher};
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ops::{Index, IndexMut, Range};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicPtr, AtomicU8, AtomicUsize, Ordering};

// Atomic accesses use relaxed ordering unless stronger synchronization is
// required, matching C++ mold's default atomic wrapper.

use bstr::BStr;
use hashbrown::{Equivalent, HashMap};
use rayon::prelude::*;

use crate::arch::Arch;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::error::demangle_enabled;
use crate::input_files::FileId;
use crate::input_sections::{FragmentRef, InputSection, InputSectionId};
use crate::util::demangle::{demangle_cpp, demangle_rust};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SymbolId(pub u32);

impl SymbolId {
    // Local symbols in discarded COMDAT sections all use one zero-valued symbol.
    pub const DISCARDED_COMDAT: SymbolId = SymbolId(0);

    /// No symbol. Used where an optional id must remain four bytes.
    pub const NONE: SymbolId = SymbolId(u32::MAX);

    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Index into the owning context's append-only output-chunk registry.
#[derive(Clone, Copy)]
pub(crate) struct SymbolChunkId(pub(crate) u32);

// Origin stores an input-section id, an output-chunk id, or compact
// ids for section fragments and symbols in one word. The low two bits identify
// which representation it contains.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub(crate) struct Origin(u64);

const ORIGIN_TAG_MASK: u64 = 0b11;
const SECTION_TAG: u64 = 0;
const CHUNK_TAG: u64 = 1;
const FRAGMENT_TAG: u64 = 2;
const SYMBOL_TAG: u64 = 3;

#[derive(Clone, Copy)]
pub(crate) enum OriginValue {
    None,
    InputSection(InputSectionId),
    OutputChunk(SymbolChunkId),
    Fragment(FragmentRef),
    Symbol(SymbolId),
}

impl Origin {
    const NONE: Origin = Origin(0);

    fn new(value: OriginValue) -> Origin {
        match value {
            OriginValue::None => Origin::NONE,
            OriginValue::InputSection(section) => {
                debug_assert_ne!(section, InputSectionId::NONE);
                Origin(section.raw() << 2 | SECTION_TAG)
            }
            OriginValue::OutputChunk(chunk) => Origin(u64::from(chunk.0) << 2 | CHUNK_TAG),
            OriginValue::Fragment(fragment) => {
                // FragmentRef contains two u32 indices. One billion merged sections
                // are enough to leave the low two bits available for the tag.
                assert!(fragment.section.0 < 1 << 30, "too many merged sections");
                Origin(fragment.raw() << 2 | FRAGMENT_TAG)
            }
            OriginValue::Symbol(symbol) => Origin(u64::from(symbol.0) << 2 | SYMBOL_TAG),
        }
    }

    #[inline]
    fn get(self) -> OriginValue {
        if self.0 == 0 {
            return OriginValue::None;
        }

        match self.0 & ORIGIN_TAG_MASK {
            SECTION_TAG => OriginValue::InputSection(InputSectionId::from_raw(self.0 >> 2)),
            CHUNK_TAG => OriginValue::OutputChunk(SymbolChunkId((self.0 >> 2) as u32)),
            FRAGMENT_TAG => OriginValue::Fragment(FragmentRef::from_raw(self.0 >> 2)),
            SYMBOL_TAG => OriginValue::Symbol(SymbolId((self.0 >> 2) as u32)),
            _ => unreachable!(),
        }
    }
}

impl fmt::Debug for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Origin")
    }
}

const _: () = assert!(std::mem::size_of::<Origin>() == 8);

/// Symbol flags set while scanning relocations.
pub const NEEDS_GOT: u8 = 1 << 0;
pub const NEEDS_PLT: u8 = 1 << 1;
/// A canonical PLT entry or a copy relocation.
pub const NEEDS_CANONICAL: u8 = 1 << 2;
pub const NEEDS_GOTTP: u8 = 1 << 3;
pub const NEEDS_TLSGD: u8 = 1 << 4;
pub const NEEDS_TLSDESC: u8 = 1 << 5;
pub const NEEDS_PPC_OPD: u8 = 1 << 6; // for PPCv1

// Flags for Symbol<E>::get_addr()
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AddrFlags {
    // Request an address other than .plt
    pub no_plt: bool,
    // Request an address other than .opd (PPC64V1 only)
    pub no_opd: bool,
}

impl AddrFlags {
    pub const NO_PLT: AddrFlags = AddrFlags {
        no_plt: true,
        no_opd: false,
    };
    pub const NO_OPD: AddrFlags = AddrFlags {
        no_plt: false,
        no_opd: true,
    };
}

// Rarely used fields for dynamic symbols. Because mold allocates tens of
// millions of symbols for large programs, keeping these fields separate from
// Symbol saves memory.
#[derive(Debug, Default)]
pub struct SymbolAux {
    pub got_idx: OptionalIndex,
    pub gottp_idx: OptionalIndex,
    pub tlsgd_idx: OptionalIndex,
    pub tlsdesc_idx: OptionalIndex,
    pub plt_idx: OptionalIndex,
    pub pltgot_idx: OptionalIndex,
    pub dynsym_idx: OptionalIndex,
    pub opd_idx: OptionalIndex,
    pub djb_hash: u32,
    // For range extension thunks
    pub thunk_addrs: Vec<u64>,
}

const _: () = assert!(std::mem::size_of::<SymbolAux>() == 64);

/// An optional table index, with the all-ones value reserved for no entry.
/// Avoids the separate discriminant that Option<u32> would require.
#[derive(Clone, Copy, Debug)]
pub struct OptionalIndex(u32);

impl Default for OptionalIndex {
    fn default() -> Self {
        Self(u32::MAX)
    }
}

impl OptionalIndex {
    #[inline]
    pub fn set(&mut self, index: u32) {
        assert_ne!(index, u32::MAX);
        self.0 = index;
    }

    #[inline]
    fn get(self) -> Option<u32> {
        (self.0 != u32::MAX).then_some(self.0)
    }
}

/// A nullable 32-bit file reference. The high bit distinguishes shared files
/// from object files and the all-ones value represents no file.
#[derive(Clone, Copy, Debug)]
struct SymbolFile(u32);

impl SymbolFile {
    const DSO: u32 = 1 << 31;
    const NONE: u32 = u32::MAX;

    #[inline]
    fn none() -> SymbolFile {
        SymbolFile(Self::NONE)
    }

    #[inline]
    fn some(file: FileId) -> SymbolFile {
        let raw = match file {
            FileId::Obj(id) => {
                debug_assert!(id.0 < Self::DSO);
                id.0
            }
            FileId::Dso(id) => {
                debug_assert!(id.0 < Self::DSO - 1);
                id.0 | Self::DSO
            }
        };
        SymbolFile(raw)
    }

    #[inline]
    fn get(self) -> Option<FileId> {
        match self.0 {
            Self::NONE => None,
            raw if raw & Self::DSO != 0 => {
                Some(FileId::Dso(crate::input_files::DsoId(raw & !Self::DSO)))
            }
            raw => Some(FileId::Obj(crate::input_files::ObjId(raw))),
        }
    }
}

// Symbol represents one local symbol or one unique global symbol name.
//
// A symbol may have several addresses when it has PLT or GOT entries. This type
// provides the operations that compute those addresses.
#[derive(Debug)]
pub struct Symbol {
    // Global symbol names live in the symbol map, and local symbol names live
    // in the owner file. Since the symbol map and symbol array are separate,
    // retain a thin pointer rather than a 16-byte slice.
    name_ptr: usize,
    name_len: u32,

    /// Serializes the parallel updates made while resolving definitions.
    /// The byte also holds the resolution-only `skip_dso` bit; both fit in
    /// the structure's existing padding.
    mu: AtomicU8,

    // A symbol is owned by a file. If two or more files define the
    // same symbol, the one with the strongest definition owns the symbol.
    // If `file` is null, the symbol is not defined by any input file.
    // A symbol usually belongs to an input section, but it can belong to a
    // section fragment, an output section or nothing (i.e. an absolute symbol).
    // A symbol pointer is used temporarily for default symbol versions.
    file: SymbolFile,
    origin: Origin,

    // `value` contains the symbol value. If this is an absolute symbol, it is
    // equivalent to its address. Otherwise, it is relative to `origin`.
    pub value: u64,

    // Index into the symbol table of the owner file.
    pub sym_idx: u32,
    type_and_bind: u8,

    pub ver_idx: u16,
    pub visibility: AtomicU8,

    // `flags` has NEEDS_ flags.
    pub flags: AtomicU8,

    // Index into SymbolTable's side array of auxiliary data, allocated on
    // demand for dynamic symbols.
    aux_idx: u32,

    /// The symbol's boolean attributes, packed; the accessors below name
    /// them.
    bits: u16,
}

const _: () = assert!(std::mem::size_of::<Symbol>() == 48);

const SYMBOL_LOCKED: u8 = 1 << 0;

// For symbol resolution. This flag is used rarely. See a comment in
// resolve_symbols().
const SYMBOL_SKIP_DSO: u8 = 1 << 1;

const NO_AUX: u32 = u32::MAX;
const WRITE_TO_SYMTAB: u8 = 1 << 7; // for --strip-all and the like
const NEEDS_MASK: u8 = !WRITE_TO_SYMTAB;

const VISIBILITY_MASK: u8 = 0b11;
const SYMBOL_STATE_SHIFT: u32 = 2;
const SYMBOL_STATE_MASK: u8 = 0b11 << SYMBOL_STATE_SHIFT;
const SYMBOL_UNDEFINED: u8 = 0;
const SYMBOL_COMMON: u8 = 1;
const SYMBOL_DEFINED: u8 = 2;

const WEAK: u16 = 1 << 0;

// If a symbol can be resolved to a symbol in a different ELF file at
// runtime, `is_imported` is true. If a symbol is a dynamic symbol and
// can be used by other ELF file at runtime, `is_exported` is true.
//
// Note that both can be true at the same time. Such symbol represents
// a function or data exported from this ELF file which can be
// imported by other definition at runtime. That is actually a usual
// exported symbol when creating a DSO. In other words, a dynamic
// symbol exported by a DSO is usually imported by itself.
//
// If is_imported is true and is_exported is false, it is a dynamic
// symbol just imported from other DSO.
//
// If is_imported is false and is_exported is true, there are two
// possible cases. If we are creating an executable, we know that
// exported symbols cannot be intercepted by any DSO (because the
// dynamic loader searches a dynamic symbol from an executable before
// examining any DSOs), so any exported symbol is export-only in an
// executable. If we are creating a DSO, export-only symbols
// represent a protected symbol (i.e. a symbol whose visibility is
// STV_PROTECTED).
const IMPORTED: u16 = 1 << 1;
const EXPORTED: u16 = 1 << 2;

// `is_canonical` is true if this symbol represents a "canonical" PLT.
// Here is the explanation as to what the canonical PLT is.
//
// In C/C++, the process-wide function pointer equality is guaranteed.
// That is, if you take an address of a function `foo`, it's always
// evaluated to the same address wherever you do that.
//
// For the sake of explanation, assume that `libx.so` exports a
// function symbol `foo`, and there's a program that uses `libx.so`.
// Both `libx.so` and the main executable take the address of `foo`,
// which must be evaluated to the same address because of the above
// guarantee.
//
// If the main executable is position-independent code (PIC), `foo` is
// evaluated to the beginning of the function code, as you would have
// expected. The address of `foo` is stored to GOTs, and the machine
// code that takes the address of `foo` reads the GOT entries at
// runtime.
//
// However, if it's not PIC, the main executable's code was compiled
// to not use GOT (note that shared objects are always PIC, only
// executables can be non-PIC). It instead assumes that `foo` (and any
// other global variables/functions) has an address that is fixed at
// link-time. This assumption is correct if `foo` is in the same
// position-dependent executable, but it's not if `foo` is imported
// from some other DSO at runtime.
//
// In this case, we use the address of the `foo`'s PLT entry in the
// main executable (whose address is fixed at link-time) as its
// address. In order to guarantee pointer equality, we also need to
// fill foo's GOT entries in DSOs with the addres of the foo's PLT
// entry instead of `foo`'s real address. We can do that by setting a
// symbol value to `foo`'s dynamic symbol. If a symbol value is set,
// the dynamic loader initialize `foo`'s GOT entries with that value
// instead of the symbol's real address.
//
// We call such PLT entry in the main executable as "canonical".
// If `foo` has a canonical PLT, its address is evaluated to its
// canonical PLT's address. Otherwise, it's evaluated to `foo`'s
// address.
//
// Only non-PIC main executables may have canonical PLTs. PIC
// executables and shared objects never have a canonical PLT.
//
// This bit manages if we need to make this symbol's PLT canonical.
// This bit is meaningful only when the symbol has a PLT entry.
const CANONICAL: u16 = 1 << 3;

// If an input object file is not compiled with -fPIC (or with
// -fno-PIC), the file not position independent. That means the
// machine code included in the object file does not use GOT to access
// global variables. Instead, it assumes that addresses of global
// variables are known at link-time.
//
// Let's say `libx.so` exports a global variable `foo`, and a main
// executable uses the variable. If the executable is not compiled
// with -fPIC, we can't simply apply a relocation that refers `foo`
// because `foo`'s address is not known at link-time.
//
// In this case, we could print out the "recompile with -fPIC" error
// message, but there's a way to workaround.
//
// The loader supports a feature so-called "copy relocations".
// A copy relocation instructs the loader to copy data from a DSO to a
// specified location in the main executable. By using this feature,
// we can copy `foo`'s data to a BSS region at runtime. With that,
// we can apply relocations agianst `foo` as if `foo` existed in the
// main executable's BSS area, whose address is known at link-time.
//
// Copy relocations are used only by position-dependent executables.
// Position-independent executables and DSOs don't need them because
// they use GOT to access global variables.
//
// `has_copyrel` is true if we need to emit a copy relocation for this
// symbol. If the original symbol in a DSO is in a read-only memory
// region, `is_copyrel_readonly` is set to true so that the copied data
// will become read-only at run-time.
const COPYREL: u16 = 1 << 4;
const COPYREL_READONLY: u16 = 1 << 5;

const TRACED: u16 = 1 << 6; // for --trace-symbol
const WRAPPED: u16 = 1 << 7; // for --wrap

// For symbols with default symbol version, e.g. foo@@VERSION.
const VERSIONED_DEFAULT: u16 = 1 << 8;

// For --gc-sections
const GC_ROOT: u16 = 1 << 10;

// For LTO. True if the symbol is referenced by a regular object (as
// opposed to IR object).
const REFERENCED_BY_REGULAR_OBJ: u16 = 1 << 11;

// For LTO. True if the symbol is the signature of a COMDAT group
// claimed by an IR file.
const COMDAT_CLAIMED_BY_IR: u16 = 1 << 12;

// A dummy symbol created for a relocation into a mergeable fragment.
const FRAGMENT_DUMMY: u16 = 1 << 13;

/// Whether the symbol comes from a Rust object, which decides how a
/// legacy-mangled name is demangled.
const RUST: u16 = 1 << 14;

/// Defines a getter and a setter for each of the packed attributes.
macro_rules! symbol_bits {
    ($($get:ident, $set:ident: $bit:ident;)*) => {
        impl Symbol {
            $(
                #[inline]
                pub fn $get(&self) -> bool {
                    self.bits & $bit != 0
                }

                #[inline]
                pub fn $set(&mut self, on: bool) {
                    if on {
                        self.bits |= $bit;
                    } else {
                        self.bits &= !$bit;
                    }
                }
            )*
        }
    };
}

symbol_bits! {
    is_weak, set_weak: WEAK;
    is_imported, set_imported: IMPORTED;
    is_exported, set_exported: EXPORTED;
    is_canonical, set_canonical: CANONICAL;
    has_copyrel, set_copyrel: COPYREL;
    is_copyrel_readonly, set_copyrel_readonly: COPYREL_READONLY;
    is_traced, set_traced: TRACED;
    is_wrapped, set_wrapped: WRAPPED;
    is_versioned_default, set_versioned_default: VERSIONED_DEFAULT;
    gc_root, set_gc_root: GC_ROOT;
    referenced_by_regular_obj, set_referenced_by_regular_obj: REFERENCED_BY_REGULAR_OBJ;
    comdat_claimed_by_ir, set_comdat_claimed_by_ir: COMDAT_CLAIMED_BY_IR;
    is_fragment_dummy, set_fragment_dummy: FRAGMENT_DUMMY;
    is_rust, set_rust: RUST;
}

// Inline objects and functions

impl Symbol {
    #[inline]
    pub fn new(name: &'static BStr) -> Symbol {
        let name_len = u32::try_from(name.len()).expect("symbol name is larger than 4 GiB");
        Symbol {
            name_ptr: name.as_ptr() as usize,
            name_len,
            mu: AtomicU8::new(0),
            file: SymbolFile::none(),
            origin: Origin::NONE,
            value: 0,
            sym_idx: u32::MAX,
            type_and_bind: 0,
            ver_idx: VER_NDX_UNSPECIFIED as u16,
            visibility: AtomicU8::new(STV_DEFAULT as u8),
            flags: AtomicU8::new(0),
            aux_idx: NO_AUX,
            bits: 0,
        }
    }

    /// The name storage outlives the link. Only [`Self::new`] sets the
    /// pointer, and it accepts a static string.
    #[inline]
    pub fn name(&self) -> &'static BStr {
        // SAFETY: `name_ptr` and `name_len` came from the same static slice
        // in `new` and are never modified.
        BStr::new(unsafe {
            std::slice::from_raw_parts(self.name_ptr as *const u8, self.name_len as usize)
        })
    }

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

    #[inline]
    pub fn set_skip_dso(&self, on: bool) {
        if on {
            crate::util::atomic_or(&self.mu, SYMBOL_SKIP_DSO);
        } else {
            self.mu.fetch_and(!SYMBOL_SKIP_DSO, Ordering::Relaxed);
        }
    }

    /// Reads `skip_dso` without forming a reference to the rest of a Symbol
    /// being edited by another resolver worker.
    ///
    /// # Safety
    ///
    /// `ptr` must point to a live Symbol.
    #[inline]
    pub(crate) unsafe fn skip_dso_at(ptr: *const Symbol) -> bool {
        let mu = unsafe { std::ptr::addr_of!((*ptr).mu) };
        unsafe { &*mu }.load(Ordering::Relaxed) & SYMBOL_SKIP_DSO != 0
    }

    /// Runs `f` while holding this symbol's resolution lock.
    ///
    /// # Safety
    ///
    /// `ptr` must come from an exclusively borrowed symbol table that stays
    /// in place for the call. All concurrent access to the pointed-to symbol
    /// must use this function.
    #[inline]
    pub(crate) unsafe fn with_resolution_lock<R>(
        ptr: *mut Symbol,
        f: impl FnOnce(&mut Symbol) -> R,
    ) -> R {
        let mu = unsafe { std::ptr::addr_of!((*ptr).mu) };
        let mut unlocked = 0;
        loop {
            match unsafe { &*mu }.compare_exchange_weak(
                unlocked,
                unlocked | SYMBOL_LOCKED,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(mut actual) => {
                    while actual & SYMBOL_LOCKED != 0 {
                        std::hint::spin_loop();
                        actual = unsafe { &*mu }.load(Ordering::Relaxed);
                    }
                    unlocked = actual;
                }
            }
        }

        struct Guard(*const AtomicU8, u8);
        impl Drop for Guard {
            #[inline]
            fn drop(&mut self) {
                unsafe { &*self.0 }.store(self.1, Ordering::Release);
            }
        }

        let _guard = Guard(mu, unlocked);
        unsafe { f(&mut *ptr) }
    }

    #[inline]
    fn set_visibility_bits(&self, mask: u8, value: u8) {
        let mut cur = self.visibility.load(Ordering::Relaxed);
        loop {
            let new = (cur & !mask) | (value & mask);
            match self.visibility.compare_exchange_weak(
                cur,
                new,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(actual) => cur = actual,
            }
        }
    }

    #[inline]
    fn symbol_state(&self) -> u8 {
        (self.visibility.load(Ordering::Relaxed) & SYMBOL_STATE_MASK) >> SYMBOL_STATE_SHIFT
    }

    #[inline]
    fn set_symbol_state(&mut self, state: u8) {
        let bits = self.visibility.get_mut();
        *bits = (*bits & !SYMBOL_STATE_MASK) | (state << SYMBOL_STATE_SHIFT);
    }

    #[inline]
    pub fn visibility(&self) -> u32 {
        (self.visibility.load(Ordering::Relaxed) & VISIBILITY_MASK) as u32
    }

    #[inline]
    pub fn set_visibility(&self, v: u32) {
        self.set_visibility_bits(VISIBILITY_MASK, v as u8);
    }

    // Symbol's visibility is set to the most restrictive one. For example,
    // if one input file has a defined symbol `foo` with the default
    // visibility and the other input file has an undefined symbol `foo`
    // with the hidden visibility, the resulting symbol is a hidden defined
    // symbol.
    #[inline]
    pub fn merge_visibility(&self, vis: u32) {
        // Canonicalize visibility
        let vis = if vis == STV_INTERNAL { STV_HIDDEN } else { vis } as u8;
        let rank = |v: u8| match v as u32 {
            STV_HIDDEN => 1,
            STV_PROTECTED => 2,
            _ => 3,
        };
        let mut cur = self.visibility.load(Ordering::Relaxed);
        while rank(vis) < rank(cur & VISIBILITY_MASK) {
            let new = (cur & !VISIBILITY_MASK) | vis;
            match self.visibility.compare_exchange_weak(
                cur,
                new,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => cur = actual,
            }
        }
    }

    #[inline]
    pub fn flags(&self) -> u8 {
        self.flags.load(Ordering::Relaxed) & NEEDS_MASK
    }

    #[inline]
    pub fn add_flags(&self, flags: u8) {
        debug_assert_eq!(flags & WRITE_TO_SYMTAB, 0);
        crate::util::atomic_or(&self.flags, flags);
    }

    #[inline]
    pub fn write_to_symtab(&self) -> bool {
        self.flags.load(Ordering::Relaxed) & WRITE_TO_SYMTAB != 0
    }

    #[inline]
    pub fn set_write_to_symtab(&self) {
        crate::util::atomic_or(&self.flags, WRITE_TO_SYMTAB);
    }

    #[inline]
    pub fn clear_flags(&self) {
        self.flags.fetch_and(WRITE_TO_SYMTAB, Ordering::Relaxed);
    }

    /// Marks the symbol, returning true if it wasn't marked yet. Once the
    /// NEEDS_* flags have been turned into GOT and PLT entries, the field
    /// is free to serve as a scratch mark, which thunk creation uses.
    #[inline]
    pub fn mark(&self) -> bool {
        // A relaxed load + branch (assuming miss) takes only around 20 cycles,
        // while an atomic RMW can easily take hundreds on x86. We note that it's
        // common that another thread beat us in marking, so doing an optimistic
        // early test tends to improve performance in the ~20% ballpark.
        if self.flags.load(Ordering::Relaxed) & NEEDS_MASK != 0 {
            return false;
        }
        self.flags.fetch_or(1, Ordering::Relaxed) & NEEDS_MASK == 0
    }

    #[inline]
    pub fn is_marked(&self) -> bool {
        self.flags.load(Ordering::Relaxed) & NEEDS_MASK != 0
    }

    #[inline]
    pub fn unmark(&self) {
        self.clear_flags();
    }

    #[inline]
    pub fn aux<'a>(&self, symbols: &'a SymbolTable) -> Option<&'a SymbolAux> {
        (self.aux_idx != NO_AUX).then(|| &symbols.aux[self.aux_idx as usize])
    }

    pub fn got_idx(&self, symbols: &SymbolTable) -> Option<u32> {
        self.aux(symbols).and_then(|a| a.got_idx.get())
    }

    pub fn gottp_idx(&self, symbols: &SymbolTable) -> Option<u32> {
        self.aux(symbols).and_then(|a| a.gottp_idx.get())
    }

    pub fn tlsgd_idx(&self, symbols: &SymbolTable) -> Option<u32> {
        self.aux(symbols).and_then(|a| a.tlsgd_idx.get())
    }

    pub fn tlsdesc_idx(&self, symbols: &SymbolTable) -> Option<u32> {
        self.aux(symbols).and_then(|a| a.tlsdesc_idx.get())
    }

    #[inline]
    pub fn plt_idx(&self, symbols: &SymbolTable) -> Option<u32> {
        self.aux(symbols).and_then(|a| a.plt_idx.get())
    }

    #[inline]
    pub fn pltgot_idx(&self, symbols: &SymbolTable) -> Option<u32> {
        self.aux(symbols).and_then(|a| a.pltgot_idx.get())
    }

    pub fn dynsym_idx(&self, symbols: &SymbolTable) -> Option<u32> {
        self.aux(symbols).and_then(|a| a.dynsym_idx.get())
    }

    pub fn opd_idx(&self, symbols: &SymbolTable) -> Option<u32> {
        self.aux(symbols).and_then(|a| a.opd_idx.get())
    }

    #[inline]
    pub fn has_plt(&self, symbols: &SymbolTable) -> bool {
        self.aux(symbols)
            .is_some_and(|a| a.plt_idx.get().is_some() || a.pltgot_idx.get().is_some())
    }

    #[inline]
    pub fn has_got(&self, symbols: &SymbolTable) -> bool {
        self.got_idx(symbols).is_some()
    }

    #[inline]
    pub fn has_gottp(&self, symbols: &SymbolTable) -> bool {
        self.gottp_idx(symbols).is_some()
    }

    #[inline]
    pub fn has_tlsgd(&self, symbols: &SymbolTable) -> bool {
        self.tlsgd_idx(symbols).is_some()
    }

    #[inline]
    pub fn has_tlsdesc(&self, symbols: &SymbolTable) -> bool {
        self.tlsdesc_idx(symbols).is_some()
    }

    pub fn has_opd(&self, symbols: &SymbolTable) -> bool {
        self.opd_idx(symbols).is_some()
    }

    #[inline]
    pub fn input_section(&self) -> Option<InputSectionId> {
        match self.origin.get() {
            OriginValue::InputSection(section) => Some(section),
            _ => None,
        }
    }

    /// Returns the origin's ID, resolved through its owning context when needed.
    #[inline]
    pub(crate) fn origin(&self) -> OriginValue {
        self.origin.get()
    }

    /// Resolves the symbol's input-section reference in `ctx`.
    #[inline]
    pub fn input_section_ref<'a, E: Arch>(
        &self,
        ctx: &'a Context<E>,
    ) -> Option<&'a InputSection<E>> {
        self.input_section()
            .map(|section| ctx.input_section(section))
    }

    #[inline]
    pub fn fragment(&self) -> Option<FragmentRef> {
        match self.origin.get() {
            OriginValue::Fragment(fragment) => Some(fragment),
            _ => None,
        }
    }

    /// Resolves the output chunk through its owning context.
    #[doc = include_str!("../test/symbol-output-chunk.md")]
    pub fn output_chunk<'a, E: Arch>(&self, ctx: &'a Context<E>) -> Option<&'a ChunkHeader<E>> {
        match self.origin.get() {
            OriginValue::OutputChunk(chunk) => Some(ctx.symbol_chunk_header(chunk)),
            _ => None,
        }
    }

    pub fn symbol_origin(&self) -> Option<SymbolId> {
        match self.origin.get() {
            OriginValue::Symbol(symbol) => Some(symbol),
            _ => None,
        }
    }

    #[inline]
    pub fn clear_origin(&mut self) {
        self.origin = Origin::NONE;
    }

    #[inline]
    pub fn set_input_section(&mut self, section: InputSectionId) {
        self.origin = Origin::new(OriginValue::InputSection(section));
    }

    #[inline]
    pub fn set_fragment(&mut self, fragment: FragmentRef) {
        self.origin = Origin::new(OriginValue::Fragment(fragment));
    }

    #[inline]
    pub(crate) fn set_output_chunk(&mut self, chunk: SymbolChunkId) {
        self.origin = Origin::new(OriginValue::OutputChunk(chunk));
    }

    #[inline]
    pub fn set_symbol_origin(&mut self, symbol: SymbolId) {
        self.origin = Origin::new(OriginValue::Symbol(symbol));
    }

    #[inline]
    pub(crate) fn origin_state(&self) -> Origin {
        self.origin
    }

    #[inline]
    pub(crate) fn set_origin_state(&mut self, state: Origin) {
        self.origin = state;
    }

    /// The symbol's entry in the owner file's symbol table; a blank one
    /// for a symbol no file defines.
    #[inline]
    pub fn esym<E: Arch>(&self, ctx: &Context<E>) -> ElfSym<E> {
        match self.file() {
            Some(file) => ctx.file(file).elf_syms[self.sym_idx as usize],
            None => ElfSym::<E>::default(),
        }
    }

    /// Records the file's entry the symbol now refers to (see
    /// [`Self::esym`]).
    pub fn set_esym<R: SymbolRecord>(&mut self, esym: &R) {
        self.type_and_bind = (esym.st_bind() << 4 | esym.st_type()) as u8;
        let state = if esym.st_shndx().get() as u32 == SHN_UNDEF {
            SYMBOL_UNDEFINED
        } else if esym.st_shndx().get() as u32 == SHN_COMMON {
            SYMBOL_COMMON
        } else {
            SYMBOL_DEFINED
        };
        self.set_symbol_state(state);
    }

    #[inline]
    pub fn st_type(&self) -> u32 {
        (self.type_and_bind & 0xf) as u32
    }

    #[inline]
    pub fn st_bind(&self) -> u32 {
        (self.type_and_bind >> 4) as u32
    }

    #[inline]
    pub fn is_undef(&self) -> bool {
        self.symbol_state() == SYMBOL_UNDEFINED
    }

    #[inline]
    pub fn is_common(&self) -> bool {
        self.symbol_state() == SYMBOL_COMMON
    }

    #[inline]
    pub fn is_undef_weak(&self) -> bool {
        self.is_undef() && self.st_bind() == STB_WEAK
    }

    /// The symbol type, treating an IFUNC defined in a DSO as a plain
    /// function since the resolver runs inside that DSO.
    #[inline]
    pub fn ty(&self) -> u32 {
        let ty = self.st_type();
        if ty == STT_GNU_IFUNC && matches!(self.file(), Some(FileId::Dso(_))) {
            STT_FUNC
        } else {
            ty
        }
    }

    #[inline]
    pub fn is_ifunc(&self) -> bool {
        self.ty() == STT_GNU_IFUNC
    }

    // A remaining weak undefined symbol is promoted to a dynamic symbol
    // in DSO and resolved to 0 in an executable. This function returns
    // true if it's latter.
    #[inline]
    pub fn is_remaining_undef_weak(&self) -> bool {
        !self.is_imported() && self.is_undef_weak()
    }

    #[inline]
    pub fn is_absolute(&self) -> bool {
        // An unresolved weak symbol acts as if it were an absolute address
        // at address 0
        if self.is_remaining_undef_weak() {
            return true;
        }
        !self.is_imported() && matches!(self.origin.get(), OriginValue::None)
    }

    #[inline]
    pub fn is_relative(&self) -> bool {
        !self.is_absolute()
    }

    // Returns true if the symbol should be emitted as a local symbol in the
    // output symbol table. Note that a symbol that is merely not exported to
    // the dynamic symbol table is still a global symbol; besides symbols that
    // are local in the input file, only ones hidden by symbol visibility or
    // localized by a version script are demoted. Linker-synthesized symbols
    // are the exception; they are local unless we export them.
    #[inline]
    pub fn is_local<E: Arch>(&self, ctx: &Context<E>) -> bool {
        if self.st_bind() == STB_LOCAL {
            return true;
        }
        if ctx.args.relocatable {
            return false;
        }
        if self.file() == ctx.internal_obj.map(FileId::Obj) {
            return !self.is_exported();
        }
        let vis = self.visibility();
        vis == STV_HIDDEN || vis == STV_INTERNAL || self.ver_idx as u32 == VER_NDX_LOCAL
    }

    pub fn is_pde_ifunc<E: Arch>(&self, ctx: &Context<E>) -> bool {
        // Returns true if this is an ifunc tha uses two GOT slots
        self.is_ifunc() && !ctx.args.pic && !E::IS_PPC64
    }

    // Returns true if the symbol's PC-relative address is known at link-time.
    pub fn is_pcrel_linktime_const<E: Arch>(&self, ctx: &Context<E>) -> bool {
        !self.is_imported() && !self.is_ifunc() && (self.is_relative() || !ctx.args.pic)
    }

    // Returns true if the symbol's Thread Pointer-relative address is
    // known at link-time.
    pub fn is_tprel_linktime_const<E: Arch>(&self, ctx: &Context<E>) -> bool {
        debug_assert_eq!(self.ty(), STT_TLS);
        !ctx.args.shared && !self.is_imported()
    }

    // Returns true if the symbol's Thread Pointer-relative address is
    // known at load-time.
    pub fn is_tprel_runtime_const<E: Arch>(&self, ctx: &Context<E>) -> bool {
        // Returns true unless we are creating a dlopen'able DSO.
        debug_assert_eq!(self.ty(), STT_TLS);
        !(ctx.args.shared && ctx.args.z_dlopen)
    }

    /// The symbol's address, taking PLT, copy relocations and section
    /// fragments into account.
    #[inline]
    pub fn addr<E: Arch>(&self, ctx: &Context<E>) -> u64 {
        self.addr_with(ctx, AddrFlags::default())
    }

    #[inline(always)]
    pub fn addr_with<E: Arch>(&self, ctx: &Context<E>, flags: AddrFlags) -> u64 {
        let origin = self.origin();

        if let OriginValue::Fragment(frag_ref) = origin {
            let frag = ctx.fragment(frag_ref);
            if !frag.is_alive() {
                std::hint::cold_path();
                // This condition is met if a non-alloc section refers an
                // alloc section and if the referenced piece of data is
                // garbage-collected. Typically, this condition occurs if a
                // debug info section refers a string constant in .rodata.
                return 0;
            }
            return ctx.fragment_addr(frag_ref).wrapping_add(self.value);
        }

        if self.has_copyrel() {
            std::hint::cold_path();
            let chunk = if self.is_copyrel_readonly() {
                &ctx.copyrel_relro
            } else {
                &ctx.copyrel
            };
            return chunk.hdr.shdr.sh_addr.get() + self.value;
        }

        if E::FAMILY == crate::arch::Family::Ppc64V1 && !flags.no_opd && self.has_opd(&ctx.symbols)
        {
            return self.opd_addr(ctx);
        }

        if !flags.no_plt && self.has_plt(&ctx.symbols) {
            debug_assert!(self.is_imported() || self.is_ifunc());
            return self.plt_addr(ctx);
        }

        match origin {
            OriginValue::InputSection(section) => {
                let isec = ctx.input_section(section);
                if !isec.is_alive() {
                    std::hint::cold_path();
                    if let Some(leader) = isec.icf_leader() {
                        return ctx.section(leader).addr(ctx) + self.value;
                    }

                    return self.dead_section_addr(ctx, isec);
                }
                isec.addr(ctx).wrapping_add(self.value)
            }
            // Synthetic symbols hold their final address in `value`, as do
            // absolute ones.
            _ => self.value,
        }
    }

    // Keep rare discarded-section diagnostics out of the address hot path.
    #[cold]
    #[inline(never)]
    fn dead_section_addr<E: Arch>(&self, ctx: &Context<E>, isec: &InputSection<E>) -> u64 {
        if isec.name(&ctx.objs[isec.file.index()]) == b".eh_frame" {
            // .eh_frame contents are parsed and reconstructed by the linker,
            // so pointing to a specific location in a source .eh_frame
            // section doesn't make much sense. However, CRT files contain
            // symbols pointing to the very beginning and ending of the section.
            //
            // If LTO is enabled, GCC may add `.lto_priv.<whatever>` as a symbol
            // suffix. That's why we use starts_with() instead of `==` here.
            let name = self.name();
            let eh_frame = &ctx.eh_frame.hdr.shdr;
            if name.starts_with(b"__EH_FRAME_BEGIN__")
                || name.starts_with(b"__EH_FRAME_LIST__")
                || name.starts_with(b".eh_frame_seg")
                || self.st_type() == STT_SECTION
            {
                return eh_frame.sh_addr.get();
            }
            if name.starts_with(b"__FRAME_END__") || name.starts_with(b"__EH_FRAME_LIST_END__") {
                return eh_frame.sh_addr.get() + eh_frame.sh_size.get();
            }
            // ARM object files contain "$d" local symbol at the beginning
            // of data sections. Their values are not significant for .eh_frame,
            // so we just treat them as offset 0.
            if name == b"$d" || name.starts_with(b"$d.") {
                return eh_frame.sh_addr.get();
            }
            crate::fatal!(
                "symbol referring to .eh_frame is not supported: {} {}",
                self,
                ctx.file_display(self.file().unwrap())
            );
        }

        // The control can reach here if there's a relocation that refers
        // a local symbol belonging to a comdat group section. This is a
        // violation of the spec, as all relocations should use only global
        // symbols of comdat members. However, .eh_frame tends to have such
        // relocations.
        0
    }

    #[inline]
    pub fn got_addr<E: Arch>(&self, ctx: &Context<E>) -> u64 {
        ctx.got.hdr.shdr.sh_addr.get()
            + self.got_idx(&ctx.symbols).unwrap() as u64 * E::WORD_SIZE as u64
    }

    pub fn gotplt_addr<E: Arch>(&self, ctx: &Context<E>) -> u64 {
        ctx.gotplt.hdr.shdr.sh_addr.get()
            + crate::chunks::gotplt::header_size::<E>()
            + self.plt_idx(&ctx.symbols).unwrap() as u64 * crate::chunks::gotplt::entry_size::<E>()
    }

    pub fn gottp_addr<E: Arch>(&self, ctx: &Context<E>) -> u64 {
        ctx.got.hdr.shdr.sh_addr.get()
            + self.gottp_idx(&ctx.symbols).unwrap() as u64 * E::WORD_SIZE as u64
    }

    pub fn tlsgd_addr<E: Arch>(&self, ctx: &Context<E>) -> u64 {
        ctx.got.hdr.shdr.sh_addr.get()
            + self.tlsgd_idx(&ctx.symbols).unwrap() as u64 * E::WORD_SIZE as u64
    }

    pub fn tlsdesc_addr<E: Arch>(&self, ctx: &Context<E>) -> u64 {
        ctx.got.hdr.shdr.sh_addr.get()
            + self.tlsdesc_idx(&ctx.symbols).unwrap() as u64 * E::WORD_SIZE as u64
    }

    #[inline]
    pub fn plt_addr<E: Arch>(&self, ctx: &Context<E>) -> u64 {
        if let Some(idx) = self.plt_idx(&ctx.symbols) {
            return ctx.plt.hdr.shdr.sh_addr.get() + crate::chunks::plt::entry_offset::<E>(idx);
        }
        ctx.pltgot.hdr.shdr.sh_addr.get()
            + self.pltgot_idx(&ctx.symbols).unwrap() as u64 * E::PLTGOT_SIZE
    }

    pub fn opd_addr<E: Arch>(&self, ctx: &Context<E>) -> u64 {
        ctx.opd_addr(self.opd_idx(&ctx.symbols).unwrap())
    }

    pub fn got_pltgot_addr<E: Arch>(&self, ctx: &Context<E>) -> u64 {
        // An ifunc symbol occupies two consecutive GOT slots in a
        // position-dependent executable (PDE). The first slot contains the
        // symbol's PLT address, and the second slot holds the resolved
        // address. A PDE uses the ifunc symbol's PLT entry as the address
        // for the symbol, akin to a canonical PLT.
        //
        // This function returns the address that the PLT entry should use
        // to jump to the resolved address.
        //
        // Note that we don't use this function for PPC64. In PPC64, symbols
        // are always accessed through the TOC table regardless of the
        // -fno-PIE setting. We don't need canonical PLTs on the psABIs too.
        if self.is_pde_ifunc(ctx) {
            self.got_addr(ctx) + E::WORD_SIZE as u64
        } else {
            self.got_addr(ctx)
        }
    }

    /// Finds a thunk within branch range of `p`.
    pub fn thunk_addr<E: Arch>(&self, ctx: &Context<E>, p: u64) -> u64 {
        let distance = E::branch_distance() as u64;
        let addrs: &[u64] = self.aux(&ctx.symbols).map_or(&[], |a| &a.thunk_addrs);
        let lo = p.saturating_sub(distance);
        let i = addrs.partition_point(|&a| a < lo);
        if let Some(&addr) = addrs.get(i) {
            let disp = addr as i64 - p as i64;
            if -(distance as i64) <= disp && disp < distance as i64 {
                return addr;
            }
        }
        crate::fatal!("range extension thunk out of range: {}", self);
    }

    /// The symbol's index in the output symbol table.
    pub fn output_sym_idx<E: Arch>(&self, ctx: &Context<E>) -> u32 {
        let file = ctx.file(self.file().unwrap());
        let i = file.output_sym_indices[self.sym_idx as usize];
        debug_assert!(i >= 0);
        if self.is_local(ctx) {
            file.local_symtab_idx + i as u32
        } else {
            file.global_symtab_idx + i as u32
        }
    }

    /// The version string of a symbol defined in a DSO.
    pub fn version<E: Arch>(&self, ctx: &Context<E>) -> &'static [u8] {
        if let Some(FileId::Dso(id)) = self.file() {
            let dso = &ctx.dsos[id.index()];
            if let Some(&ver) = dso.version_strings.get(self.ver_idx as usize) {
                return ver;
            }
        }
        b""
    }

    pub fn demangled(&self) -> Option<String> {
        // The legacy Rust mangling scheme is indistinguishtable from C++.
        // We don't want to accidentally demangle C++ symbols as Rust ones.
        // So, the legacy mangling scheme will be demangled only when we
        // know the object file was created by rustc.
        // "_R" is the prefix of the new Rust mangling scheme.
        if self.is_rust() || self.name().starts_with(b"_R") {
            demangle_rust(self.name())
        } else {
            demangle_cpp(self.name())
        }
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if demangle_enabled() {
            if let Some(name) = self.demangled() {
                return f.write_str(&name);
            }
        }
        write!(f, "{}", self.name())
    }
}

/// The length of the symbol name in a key, which may carry a version
/// suffix (`foo@VER`).
pub fn name_len(key: &[u8]) -> usize {
    memchr::memchr(b'@', key).unwrap_or(key.len())
}

/// A map key with its precomputed hash.
///
/// Keeping the key outside [`Symbol`] means symbols not stored in the map do
/// not pay for it. Recording a key already computed its hash, so retaining it
/// avoids scanning the string again during insertion.
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

impl Hash for Key {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.hash);
    }
}

/// A key for lookups, which needn't outlive the lookup.
struct Query<'a> {
    hash: u64,
    key: &'a [u8],
}

impl Hash for Query<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.hash);
    }
}

impl Equivalent<Key> for Query<'_> {
    fn equivalent(&self, key: &Key) -> bool {
        self.hash == key.hash && self.key == key.key
    }
}

/// Reuses a key's precomputed hash for a shard-local table.
#[derive(Default)]
struct ShardHasher(u64);

impl Hasher for ShardHasher {
    fn finish(&self) -> u64 {
        // The low bits already selected the shard and are constant within it.
        // Fold in the remaining bits for bucket placement. A plain right
        // shift would erase entropy from hashbrown's high-bit lookup tags.
        self.0 ^ (self.0 >> NUM_SHARDS.trailing_zeros())
    }

    fn write(&mut self, _: &[u8]) {
        unreachable!("keys hash by their precomputed hash")
    }

    fn write_u64(&mut self, hash: u64) {
        self.0 = hash;
    }
}

type ShardMap = HashMap<Key, SymbolId, BuildHasherDefault<ShardHasher>>;

const NUM_SHARDS: usize = 64;

/// The hash of a symbol table key, which the sharded table and its bins
/// share.
pub fn hash_key(key: &[u8]) -> u64 {
    xxhash_rust::xxh3::xxh3_64(key)
}

fn shard_of(hash: u64) -> usize {
    (hash % NUM_SHARDS as u64) as usize
}

/// A key recorded for interning and the slot that receives its symbol id.
/// The slot's meaning is determined by the recorder.
#[derive(Clone, Copy, Debug)]
struct Pending<S> {
    key: Key,
    name_len: u32,
    slot: S,
}

/// The keys recorded by one task for interning, grouped by shard.
#[derive(Debug)]
pub struct Bins<S = (u32, u32)>(Vec<Vec<Pending<S>>>);

impl<S> Default for Bins<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Bins<S> {
    pub fn new() -> Bins<S> {
        Bins((0..NUM_SHARDS).map(|_| Vec::new()).collect())
    }

    /// Records `key` and the slot that receives its symbol id.
    ///
    /// The symbol is named by the first `name_len` bytes of `key`. Callers
    /// adding many keys retain one task-local [`Bins`] value, avoiding repeated
    /// thread-local lookups that would cost more than recording an entry.
    pub fn record(&mut self, key: &'static [u8], name_len: usize, slot: S) {
        self.record_hashed(key, hash_key(key), name_len, slot);
    }

    /// Records a key whose hash was computed along with the key, as the
    /// files' keys are hashed while the files are read.
    pub fn record_hashed(&mut self, key: &'static [u8], hash: u64, name_len: usize, slot: S) {
        debug_assert_eq!(hash, hash_key(key));
        self.0[shard_of(hash)].push(Pending {
            key: Key { hash, key },
            name_len: name_len as u32,
            slot,
        });
    }
}

/// A stable slot in an input file that receives an interned symbol id.
///
/// Files allocate their complete symbol-id arrays before recording keys and
/// keep them alive until `gather`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SymbolSlot(NonNull<SymbolId>);

// SAFETY: each slot is recorded exactly once and written by the one shard
// that owns its key. The owner file remains alive until gathering finishes.
unsafe impl Send for SymbolSlot {}
unsafe impl Sync for SymbolSlot {}

impl SymbolSlot {
    #[inline]
    pub(crate) fn new(slot: &mut SymbolId) -> SymbolSlot {
        SymbolSlot(NonNull::from(slot))
    }

    #[inline]
    fn assign(self, id: SymbolId) {
        // SAFETY: guaranteed by the construction and synchronization rules
        // documented on SymbolSlot.
        unsafe { self.0.write(id) };
    }
}

/// A thread-safe bump allocator over the unused tail of a symbol table.
/// Each file allocates its local symbols while it is being parsed. This gives
/// every worker a disjoint range in the central vector.
pub struct ParallelSymbolAllocator<'a> {
    slots: AtomicPtr<MaybeUninit<Symbol>>,
    first: usize,
    next: AtomicUsize,
    maximum: usize,
    marker: PhantomData<&'a mut [MaybeUninit<Symbol>]>,
}

impl ParallelSymbolAllocator<'_> {
    /// Allocates `n` consecutive symbols and passes their id and storage to
    /// `init`.
    ///
    /// # Safety
    ///
    /// `init` must initialize every element of the slice it is given.
    pub unsafe fn allocate(
        &self,
        n: usize,
        init: impl FnOnce(SymbolId, &mut [MaybeUninit<Symbol>]),
    ) -> SymbolId {
        let offset = self.next.fetch_add(n, Ordering::Relaxed);
        let end = offset.checked_add(n).expect("too many symbols");
        assert!(end <= self.maximum);
        let base_id = SymbolId((self.first + offset) as u32);
        let ptr = self.slots.load(Ordering::Relaxed);
        // SAFETY: the table reserved `maximum` slots before publishing the
        // pointer, and the atomic bump pointer gives this call an exclusive
        // range within them.
        let slots = unsafe { std::slice::from_raw_parts_mut(ptr.add(offset), n) };
        init(base_id, slots);
        base_id
    }
}

/// Mutable access to stable symbol storage during a parallel pass.
struct SymbolBlockPtr(*mut Symbol);

// SAFETY: the table holds the vector exclusively and gives each task either
// non-overlapping shard blocks or symbol ids owned by one input file.
unsafe impl Sync for SymbolBlockPtr {}

/// A pointer to the auxiliary vector during a parallel scatter to unique symbols.
struct SymbolAuxPtr(*mut SymbolAux);

// SAFETY: the two methods using this wrapper require distinct symbol ids, so
// their parallel tasks access non-overlapping auxiliary records.
unsafe impl Sync for SymbolAuxPtr {}

impl SymbolAuxPtr {
    /// Applies `f` to the record at `index`.
    ///
    /// # Safety
    /// The caller must have exclusive access to this record.
    unsafe fn with_mut<R>(&self, index: usize, f: impl FnOnce(&mut SymbolAux) -> R) -> R {
        // SAFETY: the caller supplies a valid, exclusively owned index.
        f(unsafe { &mut *self.0.add(index) })
    }
}

impl SymbolBlockPtr {
    /// Mutates only a symbol's auxiliary index.
    ///
    /// # Safety
    ///
    /// `id` must be valid, and no other task may access that symbol's index.
    unsafe fn with_aux_index<R>(&self, id: SymbolId, f: impl FnOnce(&mut u32) -> R) -> R {
        // SAFETY: the caller provides an exclusive, valid symbol id.
        unsafe { f(&mut (*self.0.add(id.index())).aux_idx) }
    }

    /// Applies `f` to the symbols in non-overlapping vector ranges.
    ///
    /// # Safety
    /// The ranges must be initialized and exclusively owned by this task.
    unsafe fn for_each(&self, ranges: &[Range<u32>], f: &(impl Fn(&mut Symbol) + Sync)) {
        for range in ranges {
            let len = (range.end - range.start) as usize;
            // SAFETY: guaranteed by the caller for every shard block.
            let symbols =
                unsafe { std::slice::from_raw_parts_mut(self.0.add(range.start as usize), len) };
            symbols.iter_mut().for_each(&f);
        }
    }
}

/// Asks the operating system to back the interior pages of an allocation with
/// transparent huge pages when that advice is available.
fn madvise_hugepage<T>(values: &Vec<T>) {
    let Some(byte_len) = values.capacity().checked_mul(std::mem::size_of::<T>()) else {
        return;
    };
    // SAFETY: the vector's allocation covers its capacity; the helper leaves
    // its possibly shared boundary pages untouched.
    unsafe { crate::util::madvise_hugepage_interior(values.as_ptr().cast(), byte_len) };
}

/// The vector of all symbols, and the index of global ones by name.
///
/// The index is built in two phases. The parallel first phase records keys and
/// stable slots that need their symbols. [`SymbolTable::gather`] then
/// deduplicates the keys, finds or creates one symbol for each key, and hands
/// its id to every recorded slot. The slots must remain stable until gathering
/// finishes, and bulk construction reserves the vector before exposing slots.
///
/// Keys whose hashes fall into different shards never interact. Each shard is
/// processed by exactly one thread during `gather`, so no synchronization is
/// needed and each key is hashed only once, when recorded.
///
/// [`SymbolTable::intern`] handles keys that arrive outside the two-phase
/// pattern, one at a time.
#[derive(Debug)]
pub struct SymbolTable {
    symbols: Vec<Symbol>,
    aux: Vec<SymbolAux>,
    shards: Vec<ShardMap>,

    /// The ranges of named symbols owned by each map shard. The other
    /// symbols are files' local symbols.
    globals: Vec<Vec<Range<u32>>>,
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

impl SymbolTable {
    pub fn new() -> SymbolTable {
        let mut table = SymbolTable {
            symbols: Vec::new(),
            aux: Vec::new(),
            shards: (0..NUM_SHARDS).map(|_| ShardMap::default()).collect(),
            globals: (0..NUM_SHARDS).map(|_| Vec::new()).collect(),
        };
        let placeholder = table.add(Symbol::new(BStr::new(b"")));
        debug_assert_eq!(placeholder, SymbolId::DISCARDED_COMDAT);
        table
    }

    /// Returns the global symbol for `key`, creating it if necessary. The
    /// key may carry a version suffix (`foo@VER`), which is not part of
    /// the symbol's name.
    pub fn intern(&mut self, key: &'static [u8]) -> SymbolId {
        self.intern_with_name(key, &key[..name_len(key)])
    }

    /// Interns `key` for a symbol named `name`, a prefix of the key.
    pub fn intern_with_name(&mut self, key: &'static [u8], name: &'static [u8]) -> SymbolId {
        debug_assert!(key.starts_with(name));
        let hash = hash_key(key);
        let shard_idx = shard_of(hash);
        let shard = &mut self.shards[shard_idx];
        if let Some(&id) = shard.get(&Key { hash, key }) {
            return id;
        }
        let id = SymbolId(self.symbols.len() as u32);
        self.symbols.push(Symbol::new(BStr::new(name)));
        shard.insert(Key { hash, key }, id);
        self.note_globals(shard_idx, id.0..id.0 + 1);
        id
    }

    /// Records that the symbols in `range` are globals owned by `shard`.
    fn note_globals(&mut self, shard: usize, range: Range<u32>) {
        let globals = &mut self.globals[shard];
        match globals.last_mut() {
            Some(last) if last.end == range.start => last.end = range.end,
            _ => globals.push(range),
        }
    }

    /// Returns the auxiliary record for `id`, allocating it in the side
    /// vector if this symbol has none. C++ mold likewise keeps this rarely
    /// used state outside `Symbol` and refers to it with an index.
    pub fn aux_mut(&mut self, id: SymbolId) -> &mut SymbolAux {
        let mut index = self.symbols[id.index()].aux_idx;
        if index == NO_AUX {
            index = self.aux.len() as u32;
            assert_ne!(index, NO_AUX, "too many symbol auxiliary records");
            self.aux.push(SymbolAux::default());
            self.symbols[id.index()].aux_idx = index;
        }
        &mut self.aux[index as usize]
    }

    /// Allocates records in each file's first-use order, without a global
    /// symbol sort. Each group contains symbols owned by that file, so new
    /// symbols cannot occur in two groups. Repeated ids within a group share
    /// one record, and existing auxiliary data is preserved.
    ///
    /// # Safety
    ///
    /// All ids must be valid and no symbol may occur in more than one group.
    pub(crate) unsafe fn allocate_aux(&mut self, groups: &[Vec<SymbolId>]) {
        let first = self.aux.len();
        let count: usize = groups.iter().map(Vec::len).sum();
        assert!(first + count < NO_AUX as usize);
        let symbols = SymbolBlockPtr(self.symbols.as_mut_ptr());
        let fresh: Vec<Vec<SymbolId>> = groups
            .par_iter()
            .map(|ids| {
                let mut fresh = Vec::new();
                for &id in ids {
                    // SAFETY: this group is the only task accessing these ids.
                    unsafe {
                        symbols.with_aux_index(id, |index| {
                            if *index == NO_AUX {
                                // Suppress repeats within the file. Records are
                                // read only after final indices are assigned.
                                *index = 0;
                                fresh.push(id);
                            }
                        });
                    }
                }
                fresh
            })
            .collect();

        let count: usize = fresh.iter().map(Vec::len).sum();
        self.aux
            .par_extend((0..count).into_par_iter().map(|_| SymbolAux::default()));
        let mut next = first;
        let parts: Vec<_> = fresh
            .into_iter()
            .map(|ids| {
                let start = next;
                next += ids.len();
                (start, ids)
            })
            .collect();
        parts.into_par_iter().for_each(|(first, ids)| {
            for (i, id) in ids.into_iter().enumerate() {
                // SAFETY: groups own disjoint ids, and each fresh id was
                // recorded only once within its group.
                unsafe {
                    symbols.with_aux_index(id, |index| *index = (first + i) as u32);
                }
            }
        });
    }

    /// Records a range extension thunk address. Thunks are created in
    /// address order.
    pub fn add_thunk_addr(&mut self, id: SymbolId, addr: u64) {
        let addrs = &mut self.aux_mut(id).thunk_addrs;
        debug_assert!(addrs.last().is_none_or(|&last| last < addr));
        addrs.push(addr);
    }

    pub fn lookup(&self, key: &[u8]) -> Option<SymbolId> {
        let hash = hash_key(key);
        self.shards[shard_of(hash)]
            .get(&Query { hash, key })
            .copied()
    }

    /// Interns the keys recorded in `bins` all at once and hands each slot
    /// its symbol through `assign`.
    pub fn gather<S: Copy + Send + Sync>(
        &mut self,
        bins: Vec<Bins<S>>,
        additional_capacity: usize,
        assign: impl Fn(S, SymbolId) + Sync,
    ) {
        // Reserve the Rust vector once, then hand each shard 256-slot blocks
        // from an atomic bump pointer. At most one partial block per shard is
        // left unused.
        const BLOCK_SIZE: usize = 256;
        let count: usize = bins.iter().flat_map(|bin| &bin.0).map(Vec::len).sum();
        let first = self.symbols.len();
        let capacity = count
            .saturating_add(NUM_SHARDS * (BLOCK_SIZE - 1))
            .saturating_add(additional_capacity);
        let old_capacity = self.symbols.capacity();
        self.symbols.reserve(capacity);
        if self.symbols.capacity() != old_capacity {
            madvise_hugepage(&self.symbols);
        }
        let capacity = self.symbols.capacity();
        let storage = AtomicPtr::new(self.symbols.as_mut_ptr());
        let next = AtomicUsize::new(first);

        let ranges: Vec<Vec<Range<u32>>> = self
            .shards
            .par_iter_mut()
            .enumerate()
            .map(|(i, shard)| {
                let count: usize = bins.iter().map(|bin| bin.0[i].len()).sum();
                shard.reserve(count);
                let symbols = storage.load(Ordering::Relaxed);
                let mut blocks: Vec<(usize, usize)> = Vec::new();
                for p in bins.iter().flat_map(|bin| &bin.0[i]) {
                    let id = match shard.entry(p.key) {
                        hashbrown::hash_map::Entry::Occupied(entry) => *entry.get(),
                        hashbrown::hash_map::Entry::Vacant(entry) => {
                            if blocks.last().is_none_or(|&(_, used)| used == BLOCK_SIZE) {
                                let start = next.fetch_add(BLOCK_SIZE, Ordering::Relaxed);
                                let end = start.checked_add(BLOCK_SIZE).expect("too many symbols");
                                assert!(end <= capacity && end < u32::MAX as usize);
                                blocks.push((start, 0));
                            }
                            let (start, used) = blocks.last_mut().unwrap();
                            let index = *start + *used;
                            *used += 1;
                            let id = SymbolId(index as u32);
                            // SAFETY: reserve keeps `symbols` stable, and the
                            // atomic bump pointer gives this shard exclusive
                            // ownership of the slot.
                            unsafe {
                                symbols.add(index).write(Symbol::new(BStr::new(
                                    &p.key.key[..p.name_len as usize],
                                )));
                            }
                            entry.insert(id);
                            id
                        }
                    };
                    assign(p.slot, id);
                }

                for &(start, used) in &blocks {
                    for index in start + used..start + BLOCK_SIZE {
                        // SAFETY: these are the unused slots in this shard's
                        // exclusive block. Initializing them makes the whole
                        // vector prefix valid while global scans skip them.
                        unsafe {
                            symbols.add(index).write(Symbol::new(BStr::new(b"")));
                        }
                    }
                }
                blocks
                    .into_iter()
                    .map(|(start, used)| start as u32..(start + used) as u32)
                    .collect()
            })
            .collect();

        let len = next.load(Ordering::Relaxed);
        // SAFETY: every allocated block, including its unused tail, was
        // initialized by its owning shard.
        unsafe { self.symbols.set_len(len) };
        for (shard, shard_ranges) in ranges.into_iter().enumerate() {
            for range in shard_ranges {
                self.note_globals(shard, range);
            }
        }
    }

    /// Gathers keys recorded against stable input-file symbol slots.
    pub(crate) fn gather_symbol_slots(
        &mut self,
        bins: Vec<Bins<SymbolSlot>>,
        additional_capacity: usize,
    ) {
        self.gather(bins, additional_capacity, SymbolSlot::assign);
    }

    /// Adds a symbol that is not indexed by name, such as a local symbol.
    pub fn add(&mut self, sym: Symbol) -> SymbolId {
        let id = SymbolId(self.symbols.len() as u32);
        self.symbols.push(sym);
        id
    }

    /// Adds `n` symbols while also exposing the initialized prefix. The two
    /// slices are disjoint, so a file-parallel pass can update old symbols and
    /// construct new ones together.
    ///
    /// # Safety
    ///
    /// `init` must initialize every element of the second slice.
    pub unsafe fn add_many_with_existing(
        &mut self,
        n: usize,
        init: impl FnOnce(&mut [Symbol], &mut [MaybeUninit<Symbol>]),
    ) -> SymbolId {
        let first = SymbolId(self.symbols.len() as u32);
        let old_capacity = self.symbols.capacity();
        self.symbols.reserve(n);
        if self.symbols.capacity() != old_capacity {
            madvise_hugepage(&self.symbols);
        }
        let ptr = self.symbols.as_mut_ptr();
        // SAFETY: `first` is the initialized length and reserve made room for
        // `n` further elements. The ranges do not overlap.
        let existing = unsafe { std::slice::from_raw_parts_mut(ptr, first.index()) };
        let slots = unsafe {
            std::slice::from_raw_parts_mut(ptr.add(first.index()).cast::<MaybeUninit<Symbol>>(), n)
        };
        init(existing, slots);
        // SAFETY: the caller promises that init initialized all n elements.
        unsafe { self.symbols.set_len(first.index() + n) };
        first
    }

    /// Lets parallel workers append up to `maximum` symbols without moving
    /// the table. The final length contains exactly the ranges they allocate.
    pub fn with_parallel_appender(
        &mut self,
        maximum: usize,
        init: impl FnOnce(&ParallelSymbolAllocator<'_>),
    ) {
        let first = self.symbols.len();
        let end = first.checked_add(maximum).expect("too many symbols");
        assert!(end <= u32::MAX as usize);
        let old_capacity = self.symbols.capacity();
        self.symbols.reserve(maximum);
        if self.symbols.capacity() != old_capacity {
            madvise_hugepage(&self.symbols);
        }

        let allocator = ParallelSymbolAllocator {
            // SAFETY: reserve made the entire tail available, even though it
            // is outside the vector's initialized length.
            slots: AtomicPtr::new(unsafe {
                self.symbols
                    .as_mut_ptr()
                    .add(first)
                    .cast::<MaybeUninit<Symbol>>()
            }),
            first,
            next: AtomicUsize::new(0),
            maximum,
            marker: PhantomData,
        };
        init(&allocator);

        let added = allocator.next.load(Ordering::Relaxed);
        debug_assert!(added <= maximum);
        // SAFETY: every allocated range was initialized before `init`
        // returned, and the bump pointer leaves no gaps between ranges.
        unsafe { self.symbols.set_len(first + added) };
    }

    pub(crate) fn len(&self) -> usize {
        self.symbols.len()
    }

    pub fn as_mut_slice(&mut self) -> &mut [Symbol] {
        &mut self.symbols
    }

    /// The ids of all named (global) symbols.
    pub fn global_ids(&self) -> impl Iterator<Item = SymbolId> + '_ {
        self.globals
            .iter()
            .flatten()
            .flat_map(|range| range.clone().map(SymbolId))
    }

    /// Applies `f` to all named symbols in parallel, one task per map shard.
    pub fn par_for_each_global_mut(&mut self, f: impl Fn(&mut Symbol) + Send + Sync) {
        let symbols = SymbolBlockPtr(self.symbols.as_mut_ptr());
        self.globals.par_iter().for_each(|ranges| {
            // SAFETY: The vector is exclusively borrowed, every global range
            // belongs to exactly one shard, and shard blocks never overlap.
            unsafe { symbols.for_each(ranges, &f) };
        });
    }

    /// Applies `f` in parallel to symbols and their auxiliary records.
    ///
    /// # Safety
    ///
    /// `ids` must contain no duplicates, since each invocation receives mutable
    /// access to the corresponding auxiliary record.
    pub(crate) unsafe fn par_for_each_aux_mut(
        &mut self,
        ids: &[SymbolId],
        f: impl Fn(usize, &Symbol, &mut SymbolAux) + Send + Sync,
    ) {
        let symbols = &self.symbols;
        let aux = SymbolAuxPtr(self.aux.as_mut_ptr());
        let aux_len = self.aux.len();
        ids.par_iter().enumerate().for_each(|(i, &id)| {
            let sym = &symbols[id.index()];
            let index = sym.aux_idx;
            debug_assert_ne!(index, NO_AUX);
            debug_assert!((index as usize) < aux_len);
            // SAFETY: the caller guarantees that ids, and hence aux_idx values,
            // are distinct, and the exclusive table borrow keeps the vector fixed.
            unsafe { aux.with_mut(index as usize, |record| f(i, sym, record)) };
        });
    }

    /// Finds named symbols satisfying `predicate` in deterministic shard
    /// order, while examining all of the symbol-map blocks in parallel.
    pub fn par_find_globals(
        &self,
        predicate: impl Fn(&Symbol) -> bool + Send + Sync,
    ) -> Vec<SymbolId> {
        self.globals
            .par_iter()
            .map(|ranges| {
                ranges
                    .iter()
                    .flat_map(|range| {
                        self.symbols[range.start as usize..range.end as usize]
                            .iter()
                            .enumerate()
                            .filter_map(|(i, sym)| {
                                predicate(sym).then_some(SymbolId(range.start + i as u32))
                            })
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
            .into_iter()
            .flatten()
            .collect()
    }
}

impl Index<SymbolId> for SymbolTable {
    type Output = Symbol;

    fn index(&self, id: SymbolId) -> &Symbol {
        &self.symbols[id.index()]
    }
}

impl IndexMut<SymbolId> for SymbolTable {
    fn index_mut(&mut self, id: SymbolId) -> &mut Symbol {
        &mut self.symbols[id.index()]
    }
}

/// Whether a string is a valid C identifier, which decides whether a
/// section gets `__start_`/`__stop_` symbols.
pub fn is_c_identifier(s: &[u8]) -> bool {
    let is_alpha = |c: u8| c == b'_' || c.is_ascii_alphabetic();
    match s.split_first() {
        Some((&first, rest)) => {
            is_alpha(first) && rest.iter().all(|&c| is_alpha(c) || c.is_ascii_digit())
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auxiliary_records_follow_first_use_and_preserve_existing_data() {
        let mut table = SymbolTable::new();
        let a = table.intern(b"a");
        let b = table.intern(b"b");
        let c = table.intern(b"c");
        let old = table.intern(b"old");
        table.aux_mut(old).got_idx.set(17);

        // SAFETY: the groups contain disjoint, valid symbol ids.
        unsafe { table.allocate_aux(&[vec![b, a, b], vec![old, c, c]]) };
        assert_eq!(table[old].aux_idx, 0);
        assert_eq!(table[b].aux_idx, 1);
        assert_eq!(table[a].aux_idx, 2);
        assert_eq!(table[c].aux_idx, 3);
        assert_eq!(table.aux.len(), 4);
        assert_eq!(table[old].got_idx(&table), Some(17));

        // SAFETY: the groups contain disjoint, valid symbol ids.
        unsafe { table.allocate_aux(&[vec![a, b], vec![old, c]]) };
        assert_eq!(table.aux.len(), 4);
        assert_eq!(table[old].got_idx(&table), Some(17));
    }

    #[test]
    fn optional_symbol_indices() {
        assert_eq!(std::mem::size_of::<OptionalIndex>(), 4);
        let mut index = OptionalIndex::default();
        assert_eq!(index.get(), None);
        for value in [0, 1, u32::MAX - 1, 17] {
            index.set(value);
            assert_eq!(index.get(), Some(value));
        }
    }

    #[test]
    fn shard_hash_distributes_buckets_and_lookup_tags() {
        for shard in 0..NUM_SHARDS as u64 {
            let mut buckets = std::collections::HashSet::new();
            for i in 0..64 {
                let mut hasher = ShardHasher::default();
                hasher.write_u64(i * NUM_SHARDS as u64 + shard);
                buckets.insert(hasher.finish() & 63);
            }
            assert_eq!(buckets.len(), 64);

            let mut tags = std::collections::HashSet::new();
            for i in 0..128 {
                let mut hasher = ShardHasher::default();
                hasher.write_u64((i << 57) | shard);
                tags.insert(hasher.finish() >> 57);
            }
            assert_eq!(tags.len(), 128);
        }
    }

    #[test]
    fn input_section_origin_roundtrip() {
        assert_eq!(InputSectionId::NONE.raw(), 0);
        assert_eq!(InputSectionId::from_raw(0), InputSectionId::NONE);
        assert!(matches!(Origin::NONE.get(), OriginValue::None));

        for (file, index, raw) in [
            (0, 0, 1),
            (0, u32::MAX - 1, 0xffff_ffff),
            (1, 0, 0x1_0000_0001),
            ((1 << 30) - 1, u32::MAX - 1, 0x3fff_ffff_ffff_ffff),
        ] {
            let section = InputSectionId::new(crate::input_files::ObjId(file), index);
            assert_eq!(section.raw(), raw);
            let origin = Origin::new(OriginValue::InputSection(section));
            assert_eq!(origin.0, raw << 2);
            let OriginValue::InputSection(decoded) = origin.get() else {
                panic!("input-section origin decoded as another variant");
            };
            assert_eq!(decoded, section);
            assert_eq!(decoded.file(), crate::input_files::ObjId(file));
            assert_eq!(decoded.index(), index as usize);
        }
    }

    #[test]
    fn accepts_long_symbol_name() {
        let name = Box::leak(vec![b'x'; u16::MAX as usize + 1].into_boxed_slice());
        let symbol = Symbol::new(BStr::new(name));
        assert_eq!(symbol.name(), BStr::new(name));
    }
}
