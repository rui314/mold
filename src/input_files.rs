//! Input object files and shared libraries.

// DWARF constants keep the spelling of the specification.
#![allow(non_upper_case_globals)]

use std::borrow::Cow;
use std::cell::UnsafeCell;
use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ops::{Index, IndexMut};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, RwLock};

use rayon::iter::plumbing::{bridge, Consumer, Producer, ProducerCallback, UnindexedConsumer};
use rayon::prelude::*;

use crate::arch::{Arch, Family};
use crate::chunks::merged::{MergedSection, MergedSectionCache};
use crate::cmdline::Args;
use crate::context::Context;
use crate::elf::*;
use crate::input_sections::{
    CieRecord, FdeRecord, FragmentRef, InputSection, InputSectionId, MergeInfo, RelocationSpan,
    SFrameFde, SectionList,
};
use crate::mapped_file::MappedFile;
use crate::symbol::{
    hash_key, Bins, OriginValue, ParallelSymbolAllocator, Symbol, SymbolId, SymbolSlot,
    SymbolTable, NEEDS_PLT,
};
use crate::util::endian::Endian;
use crate::util::perf::Counter;
use crate::util::{self, align_to, bits, cstr_at, leak_bytes, path_clean, read_uleb};
use crate::{error, fatal, out, warn};
use bstr::BStr;

// Store short name lengths exactly. A long name stores a logarithmic lower
// bound so that finding its exact length requires scanning only its suffix.
#[derive(Clone, Copy, Debug, Default)]
struct NameLen(u8);

impl NameLen {
    const LONG_NAME: usize = 240;

    #[inline]
    fn new(len: usize) -> NameLen {
        if len < Self::LONG_NAME {
            return NameLen(len as u8);
        }
        let log2 = (len - Self::LONG_NAME + 1).ilog2() as usize;
        NameLen((Self::LONG_NAME + log2.min(u8::MAX as usize - Self::LONG_NAME)) as u8)
    }

    #[inline]
    fn is_long(self) -> bool {
        self.0 as usize >= Self::LONG_NAME
    }

    #[inline]
    fn lower_bound(self) -> usize {
        if !self.is_long() {
            return self.0 as usize;
        }
        Self::LONG_NAME - 1 + (1 << (self.0 as usize - Self::LONG_NAME))
    }

    #[inline]
    fn get(self, strtab: &'static [u8], offset: usize) -> &'static [u8] {
        let rest = strtab.get(offset..).unwrap_or(&[]);
        let lower = self.lower_bound().min(rest.len());
        let len = if self.is_long() {
            lower + cstr_at(rest, lower).len()
        } else {
            lower
        };
        &rest[..len]
    }
}

/// Index of an object file in `Context::objs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjId(pub u32);

/// Index of a shared library in `Context::dsos`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DsoId(pub u32);

/// A fragment symbol awaiting its slot in the central symbol vector.
/// Keep only the varying fields here so that growing file-local vectors does
/// not repeatedly copy full Symbols.
pub(crate) struct FragmentSymbol {
    fragment: FragmentRef,
    value: u64,
    sym_idx: u32,
}

impl FragmentSymbol {
    #[inline]
    pub(crate) fn into_symbol<E: Arch>(self, file: &ObjectFile<E>) -> Symbol {
        let mut sym = Symbol::new(BStr::new(b"<fragment>"));
        sym.set_file(FileId::Obj(file.id()));
        sym.set_fragment_dummy(true);
        sym.set_sym_idx(self.sym_idx);
        sym.set_esym(&file.base.elf_syms[self.sym_idx as usize]);
        sym.set_visibility(STV_HIDDEN);
        sym.set_fragment(self.fragment);
        sym.value = self.value;
        sym
    }
}

impl ObjId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl DsoId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Owns all files in stable storage and keeps the live files' pool indices
/// separately. This is the Rust equivalent of C++ mold's `obj_pool` /
/// `dso_pool` and `objs` / `dsos` vectors.
pub struct FileList<T: FileInPool> {
    pool: Vec<Box<T>>,
    // Indices are unique and increasing: push appends, and retain preserves
    // order. Mutable iterators can therefore split the pool into disjoint
    // slices as they visit live files.
    live: Vec<u32>,
}

/// Supplies the stable pool index stored in each input file.
pub trait FileInPool {
    fn set_file_index(&mut self, index: u32);
}

impl<T: FileInPool> Default for FileList<T> {
    fn default() -> FileList<T> {
        FileList {
            pool: Vec::new(),
            live: Vec::new(),
        }
    }
}

impl<T: FileInPool> FileList<T> {
    /// Adds a file to the stable pool and the live index vector, returning its
    /// stable pool index.
    pub fn push(&mut self, mut file: Box<T>) -> u32 {
        let index = u32::try_from(self.pool.len()).expect("too many input files");
        // Packed input-section IDs reserve two of the file-index bits.
        assert!(index < 1 << 30, "too many input files");
        file.set_file_index(index);
        self.pool.push(file);
        self.live.push(index);
        index
    }

    pub fn len(&self) -> usize {
        self.live.len()
    }

    pub fn pool_len(&self) -> usize {
        self.pool.len()
    }

    /// Returns a file by its stable pool index without a bounds check.
    ///
    /// # Safety
    ///
    /// `index` must have been returned by this list's `push` method.
    #[inline]
    pub(crate) unsafe fn get_unchecked(&self, index: usize) -> &T {
        debug_assert!(index < self.pool.len());
        // SAFETY: guaranteed by the caller.
        unsafe { self.pool.get_unchecked(index) }
    }

    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    pub fn first(&self) -> Option<&T> {
        self.live
            .first()
            .map(|&index| self.pool[index as usize].as_ref())
    }

    pub fn iter(&self) -> FileIter<'_, T> {
        FileIter {
            pool: &self.pool,
            live: self.live.iter(),
        }
    }

    pub fn iter_mut(&mut self) -> FileIterMut<'_, T> {
        FileIterMut {
            pool: &mut self.pool,
            live: &self.live,
            offset: 0,
        }
    }

    /// Iterates over the pool, including files erased from the live vector.
    pub fn pool_iter(&self) -> impl ExactSizeIterator<Item = &T> + DoubleEndedIterator {
        self.pool.iter().map(Box::as_ref)
    }

    pub fn par_iter(&self) -> impl IndexedParallelIterator<Item = &T>
    where
        T: Sync,
    {
        self.live
            .par_iter()
            .map(|&index| self.pool[index as usize].as_ref())
    }

    pub fn par_iter_mut(&mut self) -> impl IndexedParallelIterator<Item = &mut T>
    where
        T: Send,
    {
        FileParIterMut(self.iter_mut())
    }

    /// Erases indices from the live vector without destroying their
    /// pool-owned files.
    pub fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        self.live.retain(|&index| keep(&self.pool[index as usize]));
    }
}

impl<T: FileInPool> Index<usize> for FileList<T> {
    type Output = T;

    fn index(&self, index: usize) -> &T {
        &self.pool[index]
    }
}

impl<T: FileInPool> IndexMut<usize> for FileList<T> {
    fn index_mut(&mut self, index: usize) -> &mut T {
        &mut self.pool[index]
    }
}

impl<'a, T: FileInPool> IntoIterator for &'a FileList<T> {
    type Item = &'a T;
    type IntoIter = FileIter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, T: FileInPool> IntoIterator for &'a mut FileList<T> {
    type Item = &'a mut T;
    type IntoIter = FileIterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<T: FileInPool> IntoIterator for FileList<T> {
    type Item = Box<T>;
    type IntoIter = std::vec::IntoIter<Box<T>>;

    fn into_iter(self) -> Self::IntoIter {
        debug_assert_eq!(self.live.len(), self.pool.len());
        self.pool.into_iter()
    }
}

/// Iterates over live files by their stable pool indices.
pub struct FileIter<'a, T> {
    pool: &'a [Box<T>],
    live: std::slice::Iter<'a, u32>,
}

impl<'a, T> Iterator for FileIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<&'a T> {
        self.live
            .next()
            .map(|&index| self.pool[index as usize].as_ref())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.live.size_hint()
    }
}

impl<'a, T> DoubleEndedIterator for FileIter<'a, T> {
    fn next_back(&mut self) -> Option<&'a T> {
        self.live
            .next_back()
            .map(|&index| self.pool[index as usize].as_ref())
    }
}

impl<T> ExactSizeIterator for FileIter<'_, T> {}
impl<T> std::iter::FusedIterator for FileIter<'_, T> {}

/// Iterates mutably over live files, removing each visited slot from the
/// remaining pool slice so that returned references cannot overlap.
pub struct FileIterMut<'a, T> {
    pool: &'a mut [Box<T>],
    live: &'a [u32],
    // The stable index corresponding to pool[0].
    offset: usize,
}

impl<'a, T> Iterator for FileIterMut<'a, T> {
    type Item = &'a mut T;

    fn next(&mut self) -> Option<&'a mut T> {
        let (&index, live) = self.live.split_first()?;
        self.live = live;
        let (_, pool) = std::mem::take(&mut self.pool).split_at_mut(index as usize - self.offset);
        let (file, pool) = pool.split_first_mut().unwrap();
        self.pool = pool;
        self.offset = index as usize + 1;
        Some(file.as_mut())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.live.len(), Some(self.live.len()))
    }
}

impl<'a, T> DoubleEndedIterator for FileIterMut<'a, T> {
    fn next_back(&mut self) -> Option<&'a mut T> {
        let (&index, live) = self.live.split_last()?;
        self.live = live;
        let (pool, tail) =
            std::mem::take(&mut self.pool).split_at_mut(index as usize - self.offset);
        self.pool = pool;
        Some(tail[0].as_mut())
    }
}

impl<T> ExactSizeIterator for FileIterMut<'_, T> {}
impl<T> std::iter::FusedIterator for FileIterMut<'_, T> {}

// Rayon's producer splits at a live-file boundary, giving each worker a
// disjoint pool slice and its corresponding live indices.
impl<'a, T: Send> Producer for FileIterMut<'a, T> {
    type Item = &'a mut T;
    type IntoIter = Self;

    fn into_iter(self) -> Self {
        self
    }

    fn split_at(self, index: usize) -> (Self, Self) {
        let (left, right) = self.live.split_at(index);
        let split = right
            .first()
            .map_or(self.pool.len(), |&i| i as usize - self.offset);
        let (pool_left, pool_right) = self.pool.split_at_mut(split);
        (
            FileIterMut {
                pool: pool_left,
                live: left,
                offset: self.offset,
            },
            FileIterMut {
                pool: pool_right,
                live: right,
                offset: self.offset + split,
            },
        )
    }
}

struct FileParIterMut<'a, T>(FileIterMut<'a, T>);

impl<'a, T: Send> ParallelIterator for FileParIterMut<'a, T> {
    type Item = &'a mut T;

    fn drive_unindexed<C: UnindexedConsumer<Self::Item>>(self, consumer: C) -> C::Result {
        bridge(self, consumer)
    }

    fn opt_len(&self) -> Option<usize> {
        Some(self.0.live.len())
    }
}

impl<T: Send> IndexedParallelIterator for FileParIterMut<'_, T> {
    fn drive<C: Consumer<Self::Item>>(self, consumer: C) -> C::Result {
        bridge(self, consumer)
    }

    fn len(&self) -> usize {
        self.0.live.len()
    }

    fn with_producer<CB: ProducerCallback<Self::Item>>(self, callback: CB) -> CB::Output {
        callback.callback(self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FileId {
    Obj(ObjId),
    Dso(DsoId),
}

impl FileId {
    pub fn is_dso(self) -> bool {
        matches!(self, FileId::Dso(_))
    }
}

impl From<ObjId> for FileId {
    fn from(id: ObjId) -> FileId {
        FileId::Obj(id)
    }
}

impl From<DsoId> for FileId {
    fn from(id: DsoId) -> FileId {
        FileId::Dso(id)
    }
}

// InputFile contains the fields shared by ObjectFile and SharedFile.
#[derive(Debug)]
pub struct InputFile<E: Layout> {
    pub mf: Option<&'static MappedFile>,
    pub filename: Cow<'static, str>,

    /// Position in the command line; lower is earlier. Symbol resolution
    /// breaks ties in favor of earlier files.
    pub priority: u32,
    file_index: u32,
    /// Files explicitly included in the output are reachable from creation.
    /// Archive members and --as-needed DSOs become reachable when referenced.
    /// Resolution ranks unreachable definitions lower, so this must be set
    /// before the first symbol resolution pass.
    pub is_reachable: AtomicBool,
    pub is_little_endian: bool,
    pub e_flags: u32,

    pub shdrs: &'static [ElfShdr<E>],
    pub shstrtab: &'static [u8],
    pub elf_syms: Cow<'static, [ElfSym<E>]>,
    pub symbol_strtab: &'static [u8],

    // Parallel to elf_syms; avoids rescanning complete symbol names.
    symbol_name_lengths: Vec<NameLen>,

    /// The symbol for each entry of `elf_syms`, plus any fragment dummies
    /// appended after them.
    pub symbols: Vec<SymbolId>,
    pub first_global: usize,

    pub as_needed: bool,

    // To create an output .symtab
    pub local_symtab_idx: u32,
    pub global_symtab_idx: u32,
    pub num_local_symtab: u32,
    pub num_global_symtab: u32,
    pub strtab_offset: u64,
    pub strtab_size: u64,

    // For --emit-relocs
    // Output symbol table index (relative to the file's block) of each
    // symbol, or -1 if not written.
    pub output_sym_indices: Vec<i32>,
}

impl<E: Layout> InputFile<E> {
    fn empty(filename: Cow<'static, str>) -> InputFile<E> {
        InputFile {
            mf: None,
            filename,
            priority: 0,
            file_index: u32::MAX,
            is_reachable: AtomicBool::new(false),
            is_little_endian: true,
            e_flags: 0,
            shdrs: &[],
            shstrtab: &[],
            elf_syms: Cow::Borrowed(&[]),
            symbol_strtab: &[],
            symbol_name_lengths: Vec::new(),
            symbols: Vec::new(),
            first_global: 0,
            as_needed: false,
            local_symtab_idx: 0,
            global_symtab_idx: 0,
            num_local_symtab: 0,
            num_global_symtab: 0,
            strtab_offset: 0,
            strtab_size: 0,
            output_sym_indices: Vec::new(),
        }
    }

    /// Reads the ELF and section headers.
    fn parse(mf: &'static MappedFile, display: &dyn fmt::Display) -> InputFile<E> {
        let data = mf.data();
        if data.len() < std::mem::size_of::<ElfEhdr<E>>() {
            fatal!("{display}: file too small");
        }
        if !data.starts_with(b"\x7fELF") {
            fatal!("{display}: not an ELF file");
        }

        let ehdr = record_from_bytes::<ElfEhdr<E>>(data);
        let shoff = ehdr.e_shoff.get() as usize;
        let shdr_size = std::mem::size_of::<ElfShdr<E>>();

        // e_shnum contains the total number of sections in an object file.
        // Since it is a 16-bit integer field, it's not large enough to
        // represent >65535 sections. If an object file contains more than 65535
        // sections, the actual number is stored to sh_size field.
        let first = data
            .get(shoff..shoff + shdr_size)
            .map(record_from_bytes::<ElfShdr<E>>);
        let num_sections = match (ehdr.e_shnum.get(), first) {
            (0, Some(first)) => first.sh_size.get() as usize,
            (n, _) => n as usize,
        };

        let Some(shdr_bytes) = data.get(shoff..shoff + num_sections * shdr_size) else {
            fatal!(
                "{}: e_shoff or e_shnum corrupted: {} {num_sections}",
                mf.name.display(),
                data.len()
            );
        };
        let shdrs = records_from_bytes::<ElfShdr<E>>(shdr_bytes);

        let mut file = InputFile {
            mf: Some(mf),
            is_little_endian: E::Endian::IS_LITTLE,
            e_flags: ehdr.e_flags.get(),
            shdrs,
            ..InputFile::empty(mf.name.to_string_lossy())
        };

        // e_shstrndx is a 16-bit field. If .shstrtab's section index is
        // too large, the actual number is stored to sh_link field.
        let shstrtab_idx = if u32::from(ehdr.e_shstrndx.get()) == SHN_XINDEX {
            file.shdrs.first().map_or(0, |s| s.sh_link.get() as usize)
        } else {
            ehdr.e_shstrndx.get() as usize
        };
        file.shstrtab = file.section_contents_checked(shstrtab_idx, display);
        file
    }

    #[inline]
    pub fn is_reachable(&self) -> bool {
        self.is_reachable.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn set_reachable(&self, value: bool) {
        self.is_reachable.store(value, Ordering::Relaxed);
    }

    /// Marks the file reachable, returning true if it wasn't already.
    #[inline]
    pub fn mark_reachable(&self) -> bool {
        // A relaxed load + branch (assuming miss) takes only around 20 cycles,
        // while an atomic RMW can easily take hundreds on x86. It is common
        // that another thread beat us in marking, so test optimistically first.
        if self.is_reachable.load(Ordering::Relaxed) {
            return false;
        }
        !self.is_reachable.swap(true, Ordering::Relaxed)
    }

    #[inline]
    pub fn data(&self) -> &'static [u8] {
        self.mf.map_or(&[], MappedFile::data)
    }

    /// The contents of a section, checked against the file size.
    #[inline]
    pub fn section_contents(&self, idx: usize) -> &'static [u8] {
        self.section_contents_checked(idx, &self.filename)
    }

    fn section_contents_checked(&self, idx: usize, display: &dyn fmt::Display) -> &'static [u8] {
        if idx >= self.shdrs.len() {
            fatal!("{display}: invalid section index: {idx}");
        }
        let shdr = &self.shdrs[idx];
        let (sh_offset, sh_size) = (shdr.sh_offset.get(), shdr.sh_size.get());
        self.section_contents_range_checked(sh_offset, sh_size, display)
    }

    /// The contents described by a section header.
    #[inline]
    pub(crate) fn section_contents_from_shdr(&self, shdr: &ElfShdr<E>) -> &'static [u8] {
        self.section_contents_range_checked(
            shdr.sh_offset.get(),
            shdr.sh_size.get(),
            &self.filename,
        )
    }

    fn section_contents_range_checked(
        &self,
        sh_offset: u64,
        sh_size: u64,
        display: &dyn fmt::Display,
    ) -> &'static [u8] {
        let data = self.data();
        let start = sh_offset as usize;
        let end = start.saturating_add(sh_size as usize);
        if end > data.len() {
            fatal!("{display}: section header is out of range: {sh_offset}");
        }
        &data[start..end]
    }

    pub fn find_section(&self, sh_type: u32) -> Option<usize> {
        self.shdrs
            .iter()
            .position(|shdr| shdr.sh_type.get() == sh_type)
    }

    #[inline]
    pub fn section_name(&self, shndx: usize) -> &'static [u8] {
        cstr_at(self.shstrtab, self.shdrs[shndx].sh_name.get() as usize)
    }

    /// Returns a symbol name using the precomputed length when available.
    #[inline(always)]
    pub fn symbol_name_in(&self, i: usize) -> &'static [u8] {
        let offset = self.elf_syms[i].st_name().get() as usize;
        if let Some(len) = self.symbol_name_lengths.get(i) {
            len.get(self.symbol_strtab, offset)
        } else {
            cstr_at(self.symbol_strtab, offset)
        }
    }

    pub fn global_symbols(&self) -> &[SymbolId] {
        &self.symbols[self.first_global.min(self.symbols.len())..]
    }

    pub fn local_symbols(&self) -> &[SymbolId] {
        &self.symbols[..self.first_global.min(self.symbols.len())]
    }

    // Find the source filename. It should be listed in symtab as STT_FILE.
    pub fn source_name(&self, symbols: &SymbolTable) -> Option<&'static [u8]> {
        self.local_symbols()
            .iter()
            .map(|&id| &symbols[id])
            .find(|sym| sym.ty() == STT_FILE)
            .map(|sym| {
                let name: &'static BStr = sym.name();
                &**name
            })
    }
}

// A COMDAT group usually contains an inline function's code and related data,
// such as its string literals. Groups are identified by a signature. If two
// groups have the same signature, the linker keeps one and discards the
// sections in the other.
#[derive(Debug)]
pub struct ComdatGroupRef {
    pub sect_idx: u32,

    // The high bit records ownership; symbol IDs occupy the remaining bits.
    signature_and_owner: u32,
}

impl ComdatGroupRef {
    const IS_OWNER: u32 = 1 << 31;

    fn new(sect_idx: u32, signature: SymbolId) -> ComdatGroupRef {
        assert!(signature.0 < Self::IS_OWNER);
        ComdatGroupRef {
            sect_idx,
            signature_and_owner: signature.0,
        }
    }

    #[inline]
    pub fn signature(&self) -> SymbolId {
        SymbolId(self.signature_and_owner & !Self::IS_OWNER)
    }

    #[inline]
    pub fn is_owner(&self) -> bool {
        self.signature_and_owner & Self::IS_OWNER != 0
    }

    #[inline]
    pub fn set_owner(&mut self, is_owner: bool) {
        if is_owner {
            self.signature_and_owner |= Self::IS_OWNER;
        } else {
            self.signature_and_owner &= !Self::IS_OWNER;
        }
    }

    /// Returns the word that receives an exceptional signature while COMDAT
    /// metadata is gathered. Ownership has not been selected at that point.
    #[inline]
    pub(crate) fn signature_word_mut(&mut self) -> &mut u32 {
        debug_assert!(!self.is_owner());
        &mut self.signature_and_owner
    }
}

const _: () = assert!(std::mem::size_of::<ComdatGroupRef>() == 8);

/// An exceptional COMDAT signature that must be interned by name. Most groups
/// use their file's existing global symbol and need only `ComdatGroupRef`.
#[derive(Debug)]
pub(crate) struct PendingComdatSignature {
    pub key: &'static [u8],
    pub group_idx: u32,
    pub name_len: u32,
}

const _: () = assert!(std::mem::size_of::<PendingComdatSignature>() == 24);

/// RISC-V attributes read from `.riscv.attributes`.
#[derive(Debug, Default)]
pub struct RiscvAttributes {
    pub stack_align: Option<u64>,
    pub arch: Option<&'static [u8]>,
    pub unaligned_access: bool,
}

/// An ordinary relocation table decoded from CREL.
///
/// Allocated input sections rewrite relocation types while their output bytes
/// are copied. Each relocation section belongs to exactly one input section,
/// so those writes are disjoint even though the files themselves are shared
/// by the parallel copy tasks.
struct DecodedRelocations<R> {
    records: UnsafeCell<Box<[R]>>,
}

impl<R> DecodedRelocations<R> {
    fn new(records: Box<[R]>) -> DecodedRelocations<R> {
        DecodedRelocations {
            records: UnsafeCell::new(records),
        }
    }

    fn as_slice(&self) -> &[R] {
        // SAFETY: mutable access is restricted to exclusive linker phases and
        // disjoint relocation sections.
        unsafe { (&*self.records.get()).as_ref() }
    }

    fn as_mut_slice(&mut self) -> &mut [R] {
        self.records.get_mut().as_mut()
    }

    /// Gives `f` mutable access through a shared file reference.
    ///
    /// # Safety
    ///
    /// The caller must exclusively own this relocation table until `f`
    /// returns.
    unsafe fn with_mut_slice(&self, f: impl FnOnce(&mut [R])) {
        // SAFETY: the caller upholds the exclusive-access requirement.
        f(unsafe { (&mut *self.records.get()).as_mut() });
    }

    fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }
}

impl<R: fmt::Debug> fmt::Debug for DecodedRelocations<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("DecodedRelocations")
            .field(&self.as_slice())
            .finish()
    }
}

// SAFETY: mutable access is permitted only for disjoint relocation tables
// owned by separate copy tasks, as documented by with_mut_slice().
unsafe impl<R: Send + Sync> Sync for DecodedRelocations<R> {}

/// Whether an object is native code, plugin input, or plugin output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectOrigin {
    Regular,
    LtoInput,
    LtoOutput,
}

// ObjectFile represents an input .o file.
#[derive(Debug)]
pub struct ObjectFile<E: Arch> {
    pub base: InputFile<E>,
    pub archive_name: PathBuf,

    /// The sections by section header index, plus sections synthesized
    /// for common symbols.
    pub sections: SectionList<E>,
    pub sections_parsed: bool,

    pub elf_sections2: Vec<ElfShdr<E>>,

    /// CREL relocation tables decoded into ordinary records, indexed by
    /// relocation section. Records remain in the target's file layout.
    decoded_crel: Vec<Option<DecodedRelocations<ElfRel<E>>>>,

    /// The number of section headers in the file.
    pub num_elf_sections: usize,

    pub cies: Vec<CieRecord>,
    pub fdes: Vec<FdeRecord>,
    pub has_symver: Vec<bool>,
    pub comdat_groups: Vec<ComdatGroupRef>,
    pub(crate) pending_comdat_signatures: Vec<PendingComdatSignature>,
    pub comdat_discarded: Vec<bool>,

    pub eh_frame_sections: Vec<u32>,
    pub sframe_sections: Vec<u32>,
    pub sframe_fdes: Vec<SFrameFde>,

    pub exclude_libs: bool,
    pub gnu_properties: BTreeMap<u32, u32>,
    pub needs_executable_stack: bool,
    pub origin: ObjectOrigin,
    pub is_gcc_offload_obj: bool,
    pub is_rust_obj: bool,
    pub is_dwarf32: bool,
    pub has_init_array: bool,
    pub has_ctors: bool,

    // Output .eh_frame layout
    pub fde_idx: u64,
    pub fde_offset: u64,
    pub fde_size: u64,

    // For ICF
    pub llvm_addrsig: Option<InputSection<E>>,

    // .debug_info sections
    pub debug_info_sections: Vec<u32>,
    // For .gdb_index
    pub debug_pubnames: Option<u32>,
    pub debug_pubtypes: Option<u32>,

    // For LTO
    /// COMDAT group signatures of the symbols of an IR object, by symbol
    /// index. IR objects have no sections, so their COMDAT groups are
    /// tracked per symbol.
    pub lto_comdat_keys: Vec<Option<&'static [u8]>>,
    pub lto_comdat_signatures: Vec<Option<SymbolId>>,
    pub lto_comdat_discarded: Vec<bool>,

    // Target-specific member
    pub riscv_attributes: RiscvAttributes,
    /// `.got2` for PPC32.
    pub got2: Option<u32>,

    symtab_shndx: Vec<u32>,
    num_common_symbols: u32,
}

impl<E: Arch> fmt::Display for ObjectFile<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.archive_name.as_os_str().is_empty() {
            write!(f, "{}", path_clean(&self.base.filename))
        } else {
            write!(
                f,
                "{}({})",
                crate::util::clean_path(&self.archive_name).display(),
                self.base.filename
            )
        }
    }
}

impl<E: Arch> FileInPool for ObjectFile<E> {
    fn set_file_index(&mut self, index: u32) {
        self.base.file_index = index;
    }
}

impl<E: Arch> ObjectFile<E> {
    #[inline]
    pub fn is_lto_input(&self) -> bool {
        self.origin == ObjectOrigin::LtoInput
    }

    /// The section header at `shndx`, including headers synthesized for
    /// common symbols.
    #[inline]
    pub fn shdr(&self, shndx: usize) -> &ElfShdr<E> {
        if shndx < self.num_elf_sections {
            &self.base.shdrs[shndx]
        } else {
            &self.elf_sections2[shndx - self.num_elf_sections]
        }
    }

    /// Returns a relocation table from the file unless a compressed table
    /// was decoded into the side table.
    #[inline(always)]
    pub(crate) fn relocations(&self, relsec_idx: Option<u32>) -> &[E::Rel] {
        let Some(relsec_idx) = relsec_idx else {
            return &[];
        };
        if let Some(Some(rels)) = self.decoded_crel.get(relsec_idx as usize) {
            return rels.as_slice();
        }

        rels_from_bytes::<E>(self.input_relocation_data(relsec_idx))
    }

    #[inline(always)]
    fn input_relocation_data(&self, relsec_idx: u32) -> &'static [u8] {
        // Relocation sections are range-checked when sections are parsed.
        let shdr = &self.base.shdrs[relsec_idx as usize];
        let (offset, size) = (shdr.sh_offset.get(), shdr.sh_size.get());
        &self.base.data()[offset as usize..(offset + size) as usize]
    }
}

/// Formats a file the way it appears in diagnostics before the file
/// object exists.
pub(crate) fn display_file<'a>(
    filename: &'a str,
    archive_name: &'a Path,
) -> impl fmt::Display + 'a {
    fmt::from_fn(move |f| {
        if archive_name.as_os_str().is_empty() {
            write!(f, "{}", path_clean(filename))
        } else {
            write!(
                f,
                "{}({})",
                crate::util::clean_path(archive_name).display(),
                filename
            )
        }
    })
}

fn is_debug_section<E: Layout>(shdr: &ElfShdr<E>, name: &[u8]) -> bool {
    shdr.sh_flags.get() & SHF_ALLOC as u64 == 0 && name.starts_with(b".debug_")
}

fn is_known_section_type<E: Arch>(shdr: &ElfShdr<E>) -> bool {
    let ty = shdr.sh_type.get();
    let flags = shdr.sh_flags.get() as u32;
    if matches!(
        ty,
        SHT_PROGBITS | SHT_NOTE | SHT_NOBITS | SHT_INIT_ARRAY | SHT_FINI_ARRAY | SHT_PREINIT_ARRAY
    ) {
        return true;
    }
    if (SHT_LOUSER..=SHT_HIUSER).contains(&ty) && flags & SHF_ALLOC == 0 {
        return true;
    }
    if (SHT_LOOS..=SHT_HIOS).contains(&ty) && flags & SHF_OS_NONCONFORMING == 0 {
        return true;
    }
    match E::FAMILY {
        Family::X86_64 if ty == SHT_X86_64_UNWIND => true,
        Family::Arm32 if ty == SHT_ARM_EXIDX || ty == SHT_ARM_ATTRIBUTES => true,
        Family::RiscV if ty == SHT_RISCV_ATTRIBUTES => true,
        _ => false,
    }
}

// Decode CREL entries one at a time so callers can either stream them or
// materialize them in an array.
struct CrelReader<'a, E: Layout> {
    data: &'a [u8],
    remaining: usize,
    scale: u32,
    is_rela: bool,
    offset: u64,
    r_type: i64,
    r_sym: i64,
    addend: i64,
    target: PhantomData<E>,
}

impl<'a, E: Arch> CrelReader<'a, E> {
    fn new(file: &dyn fmt::Display, mut data: &'a [u8]) -> Self {
        let hdr = read_uleb(&mut data);
        let is_rela = hdr & 0b100 != 0;
        if is_rela && !E::IS_RELA {
            fatal!("{file}: CREL with addends is not supported for {}", E::NAME);
        }
        CrelReader {
            data,
            remaining: (hdr >> 3) as usize,
            scale: (hdr & 0b11) as u32,
            is_rela,
            offset: 0,
            r_type: 0,
            r_sym: 0,
            addend: 0,
            target: PhantomData,
        }
    }
}

/// Reads only the relocation count from a CREL header. Invalid input is left
/// for the ordinary decoder to diagnose if the section is later selected.
fn crel_count(data: &[u8]) -> Option<usize> {
    let mut value = 0u64;
    for (i, &byte) in data.iter().take(10).enumerate() {
        let payload = u64::from(byte & 0x7f);
        let shift = i * 7;
        if shift == 63 && payload > 1 {
            return None;
        }
        value |= payload << shift;
        if byte & 0x80 == 0 {
            return usize::try_from(value >> 3).ok();
        }
    }
    None
}

impl<E: Layout> Iterator for CrelReader<'_, E> {
    type Item = ElfRel<E>;

    #[inline(always)]
    fn next(&mut self) -> Option<ElfRel<E>> {
        self.remaining = self.remaining.checked_sub(1)?;
        let nflags = if self.is_rela { 3 } else { 2 };
        let flags = self.data[0];
        self.data = &self.data[1..];

        // The first byte combines flags with the low bits of an offset
        // delta. A large delta continues as ULEB128 and can wrap the
        // current offset.
        let delta = if flags & 0x80 != 0 {
            (read_uleb(&mut self.data) << (7 - nflags)) | ((flags & 0x7f) as u64 >> nflags)
        } else {
            (flags >> nflags) as u64
        };
        self.offset = self.offset.wrapping_add(delta << self.scale);

        if flags & 1 != 0 {
            self.r_sym += util::read_sleb(&mut self.data);
        }
        if flags & 2 != 0 {
            self.r_type += util::read_sleb(&mut self.data);
        }
        if self.is_rela && flags & 4 != 0 {
            self.addend += util::read_sleb(&mut self.data);
        }

        Some(ElfRel::<E>::new(
            self.offset,
            self.r_type as u32,
            self.r_sym as u32,
            self.addend,
        ))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<E: Layout> ExactSizeIterator for CrelReader<'_, E> {}

// Keep next() always-inline: using rayon::iter::Either here can add a
// function call per relocation in hot loops.
enum RelocationIter<'a, E: Layout> {
    Ordinary(std::iter::Copied<std::slice::Iter<'a, E::Rel>>),
    Crel(CrelReader<'a, E>),
}

impl<E: Layout> Iterator for RelocationIter<'_, E> {
    type Item = ElfRel<E>;

    #[inline(always)]
    fn next(&mut self) -> Option<ElfRel<E>> {
        match self {
            RelocationIter::Ordinary(iter) => iter.next(),
            RelocationIter::Crel(iter) => iter.next(),
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            RelocationIter::Ordinary(iter) => iter.size_hint(),
            RelocationIter::Crel(iter) => iter.size_hint(),
        }
    }

    #[inline(always)]
    fn fold<B, F>(self, init: B, f: F) -> B
    where
        F: FnMut(B, ElfRel<E>) -> B,
    {
        match self {
            RelocationIter::Ordinary(iter) => iter.fold(init, f),
            RelocationIter::Crel(iter) => iter.fold(init, f),
        }
    }
}

impl<E: Layout> ExactSizeIterator for RelocationIter<'_, E> {}

// SHT_CREL is an experimental alternative relocation table format
// designed to reduce the size of the table. Only LLVM supports it
// at the moment.
//
// This function converts a CREL relocation table to a regular one.
fn decode_crel<E: Arch>(file: &dyn fmt::Display, data: &[u8]) -> Box<[ElfRel<E>]> {
    let reader = CrelReader::<E>::new(file, data);
    // Own a fixed-size array without value-initializing trivial elements
    // that the caller is about to overwrite.
    let mut rels = Box::<[ElfRel<E>]>::new_uninit_slice(reader.len());
    for (i, rel) in reader.enumerate() {
        rels[i].write(rel);
    }
    // SAFETY: CrelReader visits every index from zero to len once.
    unsafe { rels.assume_init() }
}

impl<E: Arch> ObjectFile<E> {
    pub fn id(&self) -> ObjId {
        ObjId(self.base.file_index)
    }

    /// Creates the internal object file that holds linker-synthesized
    /// symbols.
    pub fn internal() -> ObjectFile<E> {
        let mut file = ObjectFile::with_base(
            InputFile::<E>::empty(Cow::Borrowed("<internal>")),
            PathBuf::new(),
        );
        file.sections_parsed = true;
        file.base.set_reachable(true);
        file
    }

    fn with_base(base: InputFile<E>, archive_name: PathBuf) -> ObjectFile<E> {
        ObjectFile {
            num_elf_sections: base.shdrs.len(),
            base,
            archive_name,
            sections: SectionList::default(),
            sections_parsed: false,
            elf_sections2: Vec::new(),
            decoded_crel: Vec::new(),
            cies: Vec::new(),
            fdes: Vec::new(),
            has_symver: Vec::new(),
            comdat_groups: Vec::new(),
            pending_comdat_signatures: Vec::new(),
            comdat_discarded: Vec::new(),
            eh_frame_sections: Vec::new(),
            sframe_sections: Vec::new(),
            sframe_fdes: Vec::new(),
            exclude_libs: false,
            gnu_properties: BTreeMap::new(),
            needs_executable_stack: false,
            origin: ObjectOrigin::Regular,
            is_gcc_offload_obj: false,
            is_rust_obj: false,
            is_dwarf32: false,
            has_init_array: false,
            has_ctors: false,
            fde_idx: 0,
            fde_offset: 0,
            fde_size: 0,
            llvm_addrsig: None,
            debug_info_sections: Vec::new(),
            debug_pubnames: None,
            debug_pubtypes: None,
            lto_comdat_keys: Vec::new(),
            lto_comdat_signatures: Vec::new(),
            lto_comdat_discarded: Vec::new(),
            riscv_attributes: RiscvAttributes::default(),
            got2: None,
            symtab_shndx: Vec::new(),
            num_common_symbols: 0,
        }
    }

    /// Opens an object file and reads its symbol table. Sections are read
    /// later, once COMDAT group selection is done.
    pub fn new(mf: &'static MappedFile, archive_name: PathBuf) -> ObjectFile<E> {
        let base =
            InputFile::<E>::parse(mf, &display_file(&mf.name.to_string_lossy(), &archive_name));
        let mut file = ObjectFile::with_base(base, archive_name);
        file.parse_symbols();
        file
    }

    /// Creates the object for an IR file claimed by the LTO plugin. Its
    /// symbols come from the plugin rather than from an ELF symbol table,
    /// and it has no sections.
    pub fn lto_input(
        mf: &'static MappedFile,
        archive_name: PathBuf,
        elf_syms: Vec<ElfSym<E>>,
        strtab: &'static [u8],
        comdat_keys: Vec<Option<&'static [u8]>>,
    ) -> ObjectFile<E> {
        let mut base = InputFile::<E>::empty(mf.name.to_string_lossy());
        base.mf = Some(mf);
        base.elf_syms = Cow::Owned(elf_syms);
        base.symbol_strtab = strtab;
        base.first_global = 1;
        let mut file = ObjectFile::with_base(base, archive_name);
        file.origin = ObjectOrigin::LtoInput;
        file.lto_comdat_signatures = vec![None; comdat_keys.len()];
        file.lto_comdat_discarded = vec![false; comdat_keys.len()];
        file.lto_comdat_keys = comdat_keys;
        file
    }

    /// The section index of the symbol at `idx`. Indices too large for
    /// the 16-bit `st_shndx` field are stored in `.symtab_shndx`.
    #[inline]
    pub fn shndx_at(&self, idx: usize) -> usize {
        let st_shndx = self.base.elf_syms[idx].st_shndx().get();
        self.shndx_from(idx, st_shndx)
    }

    /// Like [`Self::shndx_at`] for code specialized for the target.
    #[inline]
    pub fn shndx_at_in(&self, idx: usize) -> usize {
        let st_shndx = self.base.elf_syms[idx].st_shndx().get();
        self.shndx_from(idx, st_shndx)
    }

    /// Resolves an already-read symbol's section index.
    #[inline]
    pub(crate) fn shndx_from(&self, idx: usize, st_shndx: u16) -> usize {
        if st_shndx as u32 == SHN_XINDEX {
            self.symtab_shndx.get(idx).copied().unwrap_or(0) as usize
        } else if st_shndx as u32 >= SHN_LORESERVE {
            0
        } else {
            st_shndx as usize
        }
    }

    /// The section at `shndx`. A section that has been converted to a
    /// mergeable one is returned as well, as a dead section: symbols
    /// defined in it keep referring to it until they're attached to
    /// fragments.
    #[inline]
    pub fn section(&self, shndx: usize) -> Option<&InputSection<E>> {
        self.sections.section(shndx)
    }

    /// Returns the logical ID of the section at `shndx`.
    #[inline]
    pub fn section_id(&self, shndx: usize) -> Option<InputSectionId> {
        self.sections
            .input_index(shndx)
            .map(|index| InputSectionId::new(self.id(), index))
    }

    /// Returns a section together with its logical ID without resolving the
    /// section-header index twice.
    #[inline]
    fn section_with_id(&self, shndx: usize) -> Option<(InputSectionId, &InputSection<E>)> {
        let index = self.sections.input_index(shndx)?;
        Some((
            InputSectionId::new(self.id(), index),
            // SAFETY: `input_index` only returns indices assigned by insertion.
            unsafe { self.sections.input_unchecked(index as usize) },
        ))
    }

    #[inline]
    pub fn section_mut(&mut self, shndx: usize) -> Option<&mut InputSection<E>> {
        self.sections.section_mut(shndx)
    }

    /// The regular section at `shndx`, which must exist.
    #[inline]
    pub fn section_at(&self, shndx: u32) -> &InputSection<E> {
        self.section(shndx as usize).expect("no such input section")
    }

    /// Iterates over relocations without materializing a deferred CREL table.
    #[inline(always)]
    pub(crate) fn relocation_iter(
        &self,
        relsec_idx: Option<u32>,
    ) -> impl ExactSizeIterator<Item = ElfRel<E>> + '_ {
        let Some(relsec_idx) = relsec_idx else {
            return RelocationIter::Ordinary([].iter().copied());
        };
        let index = relsec_idx as usize;
        if self.base.shdrs[index].sh_type.get() == SHT_CREL
            && !self.decoded_crel.get(index).is_some_and(Option::is_some)
        {
            let data = self.input_relocation_data(relsec_idx);
            return RelocationIter::Crel(CrelReader::<E>::new(self, data));
        }

        RelocationIter::Ordinary(self.relocations(Some(relsec_idx)).iter().copied())
    }

    fn relocation_span(&self, relsec_idx: Option<u32>) -> RelocationSpan {
        let Some(relsec_idx) = relsec_idx else {
            return RelocationSpan::Input(&[]);
        };
        let rels = self.decoded_crel.get(relsec_idx as usize);
        if rels.is_some_and(Option::is_some) {
            RelocationSpan::SideTable(relsec_idx)
        } else {
            RelocationSpan::Input(self.input_relocation_data(relsec_idx))
        }
    }

    /// Returns relocations for rewriting. Ordinary records live in the
    /// input's private writable mapping; decoded CREL records use the side
    /// table because they have no ordinary in-file representation.
    pub fn rels_mut(&mut self, shndx: u32) -> &mut [E::Rel] {
        let Some(relsec_idx) = self.section_at(shndx).relsec_idx() else {
            return &mut [];
        };
        let index = relsec_idx as usize;
        if self.decoded_crel.get(index).is_some_and(Option::is_some) {
            return self.decoded_crel[index].as_mut().unwrap().as_mut_slice();
        }

        let base = &self.base;
        let shdr = &base.shdrs[index];
        let (offset, size) = (shdr.sh_offset.get(), shdr.sh_size.get());
        let mf = base.mf.expect("input relocations without a mapped file");
        // SAFETY: a mutable ObjectFile owns this relocation section for the
        // duration of the pass, and section parsing checked its range.
        let data = unsafe { mf.data_mut_ptr(offset as usize..(offset + size) as usize) };
        // SAFETY: the same exclusive ownership applies while the returned
        // relocation view is alive.
        rels_from_bytes_mut::<E>(unsafe { &mut *data })
    }

    /// Gives `f` mutable access to a relocation table during the copy phase.
    ///
    /// # Safety
    ///
    /// The caller must ensure that no other task accesses this relocation
    /// table until `f` returns. Each relocation section is attached to one
    /// input section, so copying each input section exactly once satisfies
    /// that requirement.
    pub(crate) unsafe fn with_relocations_mut(
        &self,
        relsec_idx: Option<u32>,
        f: impl FnOnce(&mut [E::Rel]),
    ) {
        let Some(relsec_idx) = relsec_idx else {
            f(&mut []);
            return;
        };
        let index = relsec_idx as usize;
        if let Some(Some(rels)) = self.decoded_crel.get(index) {
            // SAFETY: the caller exclusively owns this relocation table.
            unsafe { rels.with_mut_slice(f) };
            return;
        }

        let base = &self.base;
        let shdr = &base.shdrs[index];
        let (offset, size) = (shdr.sh_offset.get(), shdr.sh_size.get());
        let mf = base.mf.expect("input relocations without a mapped file");
        // SAFETY: the caller exclusively owns this checked relocation range.
        let data = unsafe { mf.data_mut_ptr(offset as usize..(offset + size) as usize) };
        // SAFETY: the exclusive access lasts until f returns.
        f(rels_from_bytes_mut::<E>(unsafe { &mut *data }));
    }

    fn set_decoded_crel(&mut self, index: usize, rels: Box<[E::Rel]>) {
        if self.decoded_crel.len() <= index {
            self.decoded_crel
                .resize_with(self.num_elf_sections, || None);
        }
        debug_assert!(self.decoded_crel[index].is_none());
        self.decoded_crel[index] = Some(DecodedRelocations::new(rels));
    }

    #[inline]
    pub fn merge_info(&self, shndx: usize) -> Option<&MergeInfo> {
        self.sections.merge_info(shndx)
    }

    /// The section the symbol at `idx` is defined in.
    #[inline]
    pub fn symbol_section(&self, idx: usize) -> Option<&InputSection<E>> {
        self.section(self.shndx_at(idx))
    }

    /// Whether the symbol at `idx` is defined in a discarded COMDAT group.
    #[inline]
    pub fn is_discarded_comdat(&self, idx: usize) -> bool {
        if self.comdat_discarded.is_empty() {
            return false;
        }
        let st_shndx = self.base.elf_syms[idx].st_shndx().get() as u32;
        if st_shndx == SHN_ABS || st_shndx == SHN_COMMON {
            return false;
        }
        self.comdat_discarded[self.shndx_from(idx, st_shndx as u16)]
    }

    #[inline]
    fn is_discarded_comdat_sym(&self, idx: usize, esym: &ElfSym<E>) -> bool {
        if self.comdat_discarded.is_empty() || esym.is_abs() || esym.is_common() {
            return false;
        }
        self.comdat_discarded[self.shndx_from(idx, esym.st_shndx().get())]
    }

    /// Iterates over the live regular sections.
    #[inline]
    pub fn input_sections(&self) -> impl Iterator<Item = &InputSection<E>> {
        self.sections.regular()
    }

    #[inline]
    pub fn merge_infos(&self) -> impl Iterator<Item = &MergeInfo> {
        self.sections.merge_infos()
    }

    /// The section indices of a COMDAT group's members, read from the
    /// group section as they are needed.
    #[inline]
    pub fn comdat_members(&self, group: &ComdatGroupRef) -> impl Iterator<Item = u32> + '_ {
        let data = self.base.data();
        let shdr = &self.base.shdrs[group.sect_idx as usize];
        let (sh_offset, sh_size) = (shdr.sh_offset.get(), shdr.sh_size.get());
        let start = sh_offset as usize;
        let bytes = &data[start..start + sh_size as usize];
        let is_little_endian = self.base.is_little_endian;
        bytes.chunks_exact(4).skip(1).map(move |b| {
            let b = [b[0], b[1], b[2], b[3]];
            if is_little_endian {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            }
        })
    }

    /// Marks a section dead along with its FDEs.
    pub fn kill_section(&self, shndx: usize) {
        if let Some(isec) = self.section(shndx) {
            if isec.kill() {
                for fde in isec.fdes(self) {
                    fde.kill();
                }
            }
        }
    }

    // Read global symbols before archive extraction so they can participate in
    // symbol resolution without constructing input sections.
    fn parse_symbols(&mut self) {
        if let Some(idx) = self.base.find_section(SHT_SYMTAB) {
            let shdr = &self.base.shdrs[idx];
            // In ELF, all local symbols precede global symbols in the symbol table.
            // sh_info has an index of the first global symbol.
            self.base.first_global = shdr.sh_info.get() as usize;
            let contents = self.base.section_contents(idx);
            if !contents
                .len()
                .is_multiple_of(std::mem::size_of::<ElfSym<E>>())
            {
                fatal!("{self}: corrupted section");
            }
            self.base.elf_syms = Cow::Borrowed(records_from_bytes::<ElfSym<E>>(contents));
            self.base.symbol_strtab = self.base.section_contents(shdr.sh_link.get() as usize);

            if let Some(idx) = self.base.find_section(SHT_SYMTAB_SHNDX) {
                let bytes = self.base.section_contents(idx);
                self.symtab_shndx = bytes.chunks_exact(4).map(E::Endian::read_u32).collect();
            }
        }
    }

    pub(crate) fn register_global_symbols(&mut self, args: &Args, bins: &mut Bins<SymbolSlot>) {
        let n = self.base.elf_syms.len();
        if n == 0 {
            return;
        }
        static COUNTER: Counter = Counter::new("all_syms");
        COUNTER.add(n as i64);

        self.base.symbols = vec![SymbolId::DISCARDED_COMDAT; n];
        let num_globals = n.saturating_sub(self.base.first_global);
        self.has_symver = vec![false; num_globals];
        self.base.symbol_name_lengths.clear();
        self.base.symbol_name_lengths.reserve(n);
        for esym in self.base.elf_syms.iter().take(self.base.first_global) {
            let name = cstr_at(self.base.symbol_strtab, esym.st_name().get() as usize);
            self.base.symbol_name_lengths.push(NameLen::new(name.len()));
        }

        // Register global symbols
        for i in self.base.first_global..n {
            let esym = &self.base.elf_syms[i];
            if esym.is_common() {
                self.num_common_symbols += 1;
            }

            // Find the name length and version separator in one scan.
            let strtab = self
                .base
                .symbol_strtab
                .get(esym.st_name().get() as usize..)
                .unwrap_or_default();
            let pos = memchr::memchr2(0, b'@', strtab).unwrap_or(strtab.len());
            let len = pos
                + if strtab.get(pos) == Some(&b'@') {
                    cstr_at(strtab, pos).len()
                } else {
                    0
                };
            self.base.symbol_name_lengths.push(NameLen::new(len));
            let mut key = &strtab[..len];
            let name = &key[..pos];

            // Parse symbol version after atsign
            let mut ver_len = 0;
            if pos != len {
                let ver = &key[pos..];
                if ver.starts_with(b"@@") {
                    key = name;
                } else {
                    ver_len = ver.len();
                }
                if ver != b"@" {
                    self.has_symver[i - self.base.first_global] = true;
                }
            }

            // Handle --wrap option
            if esym.is_undef() && !args.wrap.is_empty() {
                if let Some(real) = name.strip_prefix(b"__real_") {
                    if args.wrap.contains(real) {
                        key = &key[7..];
                    }
                } else if args.wrap.contains(key) {
                    key = leak_bytes([b"__wrap_", key].concat());
                }
            }
            // Only record the symbol reference here; gather_symbols() creates
            // the Symbols and fills in the `symbols` slots once all files
            // have been read.
            bins.record_hashed(
                key,
                hash_key(key),
                key.len() - ver_len,
                SymbolSlot::new(&mut self.base.symbols[i]),
            );
        }
    }

    // Read COMDAT groups and detect GCC offload objects. Both affect which input
    // sections can be discarded before LTO.
    pub fn read_section_metadata(&mut self) {
        debug_assert!(!self.sections_parsed);

        for i in 0..self.num_elf_sections {
            // SAFETY: `i` comes from the section-header table's range.
            let shdr = unsafe { self.base.shdrs.get_unchecked(i) };
            let (sh_type, sh_flags) = (shdr.sh_type.get(), shdr.sh_flags.get());

            if sh_flags & SHF_EXCLUDE as u64 != 0
                && self
                    .base
                    .section_name(i)
                    .starts_with(b".gnu.offload_lto_.symtab.")
            {
                self.is_gcc_offload_obj = true;
            }

            if sh_type != SHT_GROUP {
                continue;
            }
            if shdr.sh_info.get() as usize >= self.base.elf_syms.len() {
                fatal!("{self}: invalid symbol index");
            }

            let esym = &self.base.elf_syms[shdr.sh_info.get() as usize];
            let name = if esym.st_type() == STT_SECTION {
                self.base.section_name(
                    self.shndx_from(shdr.sh_info.get() as usize, esym.st_shndx().get()),
                )
            } else {
                self.base.symbol_name_in(shdr.sh_info.get() as usize)
            };

            // Ignore a broken comdat group GCC emits for .debug_macros.
            // https://github.com/rui314/mold/issues/438
            if name.starts_with(b"wm4.") {
                continue;
            }

            let contents = self.base.section_contents_from_shdr(shdr);
            if contents.len() < 4 {
                fatal!("{self}: empty SHT_GROUP");
            }
            let kind = E::Endian::read_u32(contents);
            if kind == 0 {
                continue;
            }
            if kind != GRP_COMDAT {
                fatal!("{self}: unsupported SHT_GROUP format");
            }

            // Reuse the version flag from registration. A bare trailing '@'
            // has no version flag but still needs its complete signature name.
            let is_own_global = esym.st_type() != STT_SECTION
                && esym.st_bind() != STB_LOCAL
                && !esym.is_undef()
                && !self.has_symver[shdr.sh_info.get() as usize - self.base.first_global]
                && !name.ends_with(b"@");

            let signature = if is_own_global {
                self.base.symbols[shdr.sh_info.get() as usize]
            } else {
                SymbolId::DISCARDED_COMDAT
            };
            let group_idx = self.comdat_groups.len() as u32;
            self.comdat_groups
                .push(ComdatGroupRef::new(i as u32, signature));
            if !is_own_global {
                self.pending_comdat_signatures.push(PendingComdatSignature {
                    key: name,
                    group_idx,
                    name_len: crate::symbol::name_len(name) as u32,
                });
            }
        }
    }

    fn parse_note_gnu_property(&mut self, mut data: &'static [u8]) {
        while data.len() >= ElfNhdr::<E>::size() {
            let hdr = ElfNhdr::<E>::parse(data);
            data = &data[ElfNhdr::<E>::size()..];

            let name_len = hdr.n_namesz.get() as usize;
            let name = &data[..name_len.saturating_sub(1).min(data.len())];
            data = &data[(align_to(name_len as u64, 4) as usize).min(data.len())..];

            let desc_len = hdr.n_descsz.get() as usize;
            let mut desc = &data[..desc_len.min(data.len())];
            data =
                &data[(align_to(desc_len as u64, E::WORD_SIZE as u64) as usize).min(data.len())..];

            if hdr.n_type.get() != NT_GNU_PROPERTY_TYPE_0 || name != b"GNU" {
                continue;
            }

            while desc.len() >= 8 {
                let ty = E::Endian::read_u32(desc);
                let size = E::Endian::read_u32(&desc[4..]) as usize;
                desc = &desc[8..];

                // The majority of currently defined .note.gnu.property
                // use 32-bit values.
                // We don't know how to handle anything else, so if we encounter
                // one, skip it.
                //
                // The following properties have a different size:
                // - GNU_PROPERTY_STACK_SIZE
                // - GNU_PROPERTY_NO_COPY_ON_PROTECTED
                if size == 4 && desc.len() >= 4 {
                    *self.gnu_properties.entry(ty).or_insert(0) |= E::Endian::read_u32(desc);
                }
                desc =
                    &desc[(align_to(size as u64, E::WORD_SIZE as u64) as usize).min(desc.len())..];
            }
        }
    }

    // <format-version>
    // [ <section-length> "vendor-name" <file-tag> <size> <attribute>*]+ ]*
    fn read_riscv_attributes(&mut self, data: &'static [u8]) {
        if data.is_empty() {
            fatal!("{self}: corrupted .riscv.attributes section");
        }
        if data[0] != b'A' {
            return;
        }
        let mut data = &data[1..];

        while !data.is_empty() {
            let sz = E::Endian::read_u32(data) as usize;
            if data.len() < sz || sz < 4 {
                fatal!("{self}: corrupted .riscv.attributes section");
            }
            let mut p = &data[4..sz];
            data = &data[sz..];

            let Some(rest) = p.strip_prefix(b"riscv\0") else {
                continue;
            };
            p = rest;
            if p.first() != Some(&(ELF_TAG_FILE as u8)) {
                fatal!("{self}: corrupted .riscv.attributes section");
            }
            p = &p[5..]; // skip the tag and the sub-sub-section size

            while !p.is_empty() {
                let tag = read_uleb(&mut p) as u32;
                match tag {
                    ELF_TAG_RISCV_STACK_ALIGN => {
                        self.riscv_attributes.stack_align = Some(read_uleb(&mut p))
                    }
                    ELF_TAG_RISCV_ARCH => {
                        let end = p.iter().position(|&b| b == 0).unwrap_or(p.len());
                        self.riscv_attributes.arch = Some(&p[..end]);
                        p = &p[(end + 1).min(p.len())..];
                    }
                    ELF_TAG_RISCV_UNALIGNED_ACCESS => {
                        self.riscv_attributes.unaligned_access = read_uleb(&mut p) != 0
                    }
                    _ => {}
                }
            }
        }
    }

    fn initialize_sections(&mut self, args: &Args, id: ObjId) {
        // Read sections
        let nsections = self.num_elf_sections;
        let expected_reloc_type = if E::IS_RELA { SHT_RELA } else { SHT_REL };
        debug_assert!(self.comdat_discarded.is_empty() || self.comdat_discarded.len() == nsections);
        for i in 0..nsections {
            if !self.comdat_discarded.is_empty()
                // SAFETY: a nonempty COMDAT bitmap has one entry per input
                // section, and `i` is in that range.
                && unsafe { *self.comdat_discarded.get_unchecked(i) }
            {
                continue;
            }

            // SAFETY: `i` comes from the file's section-header range.
            let shdr = unsafe { self.base.shdrs.get_unchecked(i) };
            let (sh_type, flags) = (shdr.sh_type.get(), shdr.sh_flags.get());
            if flags & SHF_EXCLUDE as u64 != 0
                && flags & SHF_ALLOC as u64 == 0
                && sh_type != SHT_LLVM_ADDRSIG
                && !args.relocatable
            {
                continue;
            }

            if E::IS_ARM && sh_type == SHT_ARM_ATTRIBUTES {
                continue;
            }
            if E::IS_RISCV && sh_type == SHT_RISCV_ATTRIBUTES {
                let contents = self.base.section_contents_from_shdr(shdr);
                self.read_riscv_attributes(contents);
                continue;
            }

            match sh_type {
                SHT_GROUP | SHT_SYMTAB | SHT_SYMTAB_SHNDX | SHT_STRTAB | SHT_NULL => {}
                SHT_REL | SHT_RELA => {
                    // Relocations are attached to their sections below.
                }
                SHT_CREL => {
                    let target = shdr.sh_info.get() as usize;
                    let Some(target_flags) =
                        self.base.shdrs.get(target).map(|shdr| shdr.sh_flags.get())
                    else {
                        continue;
                    };
                    let target_is_alloc = target_flags & SHF_ALLOC as u64 != 0;
                    if target_is_alloc || args.relocatable || args.emit_relocs {
                        let contents = self.base.section_contents_from_shdr(shdr);
                        let decoded = decode_crel::<E>(self, contents);

                        self.set_decoded_crel(i, decoded);
                    }
                    // Relocations are attached to their sections below.
                }
                _ => {
                    let name = cstr_at(self.base.shstrtab, shdr.sh_name.get() as usize);
                    if !is_known_section_type::<E>(shdr) {
                        fatal!(
                            "{self}: {}: unsupported section type: 0x{:x}",
                            util::display(name),
                            shdr.sh_type.get()
                        );
                    }

                    // .note.GNU-stack section controls executable-ness of the stack
                    // area in GNU linkers. We ignore that section because silently
                    // making the stack area executable is too dangerous. Tell our
                    // users about the difference if that matters.
                    if name == b".note.GNU-stack" && !args.relocatable {
                        if flags & SHF_EXECINSTR as u64 != 0 {
                            if !args.z_execstack && !args.z_execstack_if_needed {
                                warn!("{self}: this file may cause a segmentation fault because it requires an executable stack. See https://github.com/rui314/mold/tree/main/docs/execstack.md for more info."
                                );
                            }
                            self.needs_executable_stack = true;
                        }
                        continue;
                    }

                    if name == b".note.gnu.property" {
                        let contents = self.base.section_contents_from_shdr(shdr);
                        self.parse_note_gnu_property(contents);
                        continue;
                    }

                    // Ignore a build-id section in an input file. This doesn't normally
                    // happen, but you can create such object file with
                    // `ld.bfd -r --build-id`.
                    if name == b".note.gnu.build-id" {
                        continue;
                    }

                    // Ignore these sections for compatibility with old glibc i386 CRT files.
                    if name == b".gnu.linkonce.t.__x86.get_pc_thunk.bx"
                        || name == b".gnu.linkonce.t.__i686.get_pc_thunk.bx"
                    {
                        continue;
                    }

                    // Also ignore this for compatibility with ICC
                    if name == b".gnu.linkonce.d.DW.ref.__gxx_personality_v0" {
                        continue;
                    }

                    // Ignore debug sections if --strip-all or --strip-debug is given.
                    if (args.strip_all || args.strip_debug) && is_debug_section(shdr, name) {
                        continue;
                    }

                    // Ignore section is specified by --discard-section.
                    if !args.discard_section.is_empty() && args.discard_section.contains(name) {
                        continue;
                    }

                    if name == b".comment"
                        && self
                            .base
                            .section_contents_from_shdr(shdr)
                            .starts_with(b"rustc ")
                    {
                        self.is_rust_obj = true;
                    }

                    // If an output file doesn't have a section header (i.e.
                    // --oformat=binary is given), we discard all non-memory-allocated
                    // sections. This is because without a section header, we can't find
                    // their places in an output file in the first place.
                    if args.oformat_binary && flags & SHF_ALLOC as u64 == 0 {
                        continue;
                    }

                    let isec = InputSection::new(self, id, i as u32, shdr, BStr::new(name));

                    // Save .llvm_addrsig for --icf=safe.
                    if shdr.sh_type.get() == SHT_LLVM_ADDRSIG && !args.relocatable {
                        // sh_link should be the index of the symbol table section.
                        // Tools that mutates the symbol table, such as objcopy or `ld -r`
                        // tend to not preserve sh_link, so we ignore such section.
                        if shdr.sh_link.get() != 0 {
                            self.llvm_addrsig = Some(isec);
                        }
                        continue;
                    }

                    if matches!(
                        shdr.sh_type.get(),
                        SHT_INIT_ARRAY | SHT_FINI_ARRAY | SHT_PREINIT_ARRAY
                    ) {
                        self.has_init_array = true;
                    }
                    if name == b".ctors"
                        || name.starts_with(b".ctors.")
                        || name == b".dtors"
                        || name.starts_with(b".dtors.")
                    {
                        self.has_ctors = true;
                    }
                    if name == b".eh_frame" {
                        self.eh_frame_sections.push(i as u32);
                    }
                    if E::SUPPORTS_SFRAME && name == b".sframe" {
                        self.sframe_sections.push(i as u32);
                    }
                    if name == b".debug_info" && flags & SHF_ALLOC as u64 == 0 {
                        self.debug_info_sections.push(i as u32);
                    }
                    if E::FAMILY == Family::Ppc32 && name == b".got2" {
                        self.got2 = Some(i as u32);
                    }

                    if args.gdb_index {
                        // Save debug sections for --gdb-index.
                        if name == b".debug_gnu_pubnames" {
                            self.debug_pubnames = Some(i as u32);
                            isec.kill();
                        }
                        if name == b".debug_gnu_pubtypes" {
                            // If --gdb-index is given, contents of .debug_gnu_pubnames and
                            // .debug_gnu_pubtypes are copied to .gdb_index, so keeping them in
                            // an output file is just a waste of space.
                            self.debug_pubtypes = Some(i as u32);
                            isec.kill();
                        }
                        if name == b".debug_types" {
                            // .debug_types is similar to .debug_info but contains type info only.
                            // It exists only in DWARF 4, has been removed in DWARF 5 and neither
                            // GCC nor Clang generate it by default (-fdebug-types-section is
                            // needed). As such there is probably little need to support it.
                            fatal!("{self}: mold's --gdb-index is not compatible with .debug_types; to fix this error, remove -fdebug-types-section and recompile"
                            );
                        }
                    }

                    static COUNTER: Counter = Counter::new("regular_sections");
                    COUNTER.increment();
                    self.sections.insert(i, isec);
                }
            }
        }

        // Attach relocation sections to their target sections.
        for i in 0..nsections {
            // SAFETY: `i` comes from the file's section-header range.
            let shdr = unsafe { self.base.shdrs.get_unchecked(i) };
            let sh_type = shdr.sh_type.get();
            if sh_type != expected_reloc_type && sh_type != SHT_CREL {
                continue;
            }
            let target = shdr.sh_info.get() as usize;
            if self.section(target).is_none() {
                continue;
            }

            let has_relocs = if sh_type == SHT_CREL {
                if let Some(Some(decoded)) = self.decoded_crel.get(i) {
                    !decoded.is_empty()
                } else {
                    let contents = self.base.section_contents_from_shdr(shdr);
                    CrelReader::<E>::new(self, contents).len() != 0
                }
            } else {
                shdr.sh_size.get() != 0
            };

            let isec = self.section_mut(target).unwrap();
            debug_assert!(!isec.has_relsec());
            isec.set_relsec(i as u32, has_relocs);
        }

        // Attach .arm.exidx sections to their corresponding sections
        if E::FAMILY == Family::Arm32 {
            let pairs: Vec<(usize, usize)> = self
                .input_sections()
                .filter(|isec| isec.sh_type(self) == SHT_ARM_EXIDX)
                .map(|isec| {
                    (
                        self.base.shdrs[isec.shndx as usize].sh_link.get() as usize,
                        isec.shndx as usize,
                    )
                })
                .collect();
            for (target, exidx) in pairs {
                if let Some(isec) = self.section_mut(target) {
                    isec.set_exidx(exidx as u32);
                }
            }
        }
    }

    // Relocations are usually sorted by r_offset in relocation tables,
    // but for some reason only RISC-V does not follow that convention.
    // We expect them to be sorted, so sort them if necessary.
    fn sort_relocations(&mut self) {
        if !E::IS_RISCV && !E::IS_LOONGARCH {
            return;
        }
        let sections: Vec<u32> = self.input_sections().map(|isec| isec.shndx).collect();
        for shndx in sections {
            let isec = self.section_at(shndx);
            if !isec.is_alive() || !isec.is_alloc() {
                continue;
            }
            let rels = isec.rels(self);
            if !rels.iter().map(|r| r.r_offset()).is_sorted() {
                self.rels_mut(shndx).sort_by_key(|r| r.r_offset());
            }
        }
    }

    // Construct sections after COMDAT ownership is known. Members of losing groups
    // are normally skipped. If another selection will run after LTO, construct
    // them too so a different copy can become live.
    pub(crate) fn parse_sections(
        &mut self,
        args: &Args,
        id: ObjId,
        allocator: &ParallelSymbolAllocator<'_>,
        keep_discarded_comdat: bool,
    ) {
        debug_assert!(!self.sections_parsed);
        let n = self.base.shdrs.len();
        self.sections = SectionList::new(n, self.num_common_symbols as usize);

        if !keep_discarded_comdat && !self.comdat_groups.is_empty() {
            let mut discarded = vec![false; n];
            for group in self.comdat_groups.iter().filter(|group| !group.is_owner()) {
                for member in self.comdat_members(group) {
                    if let Some(slot) = discarded.get_mut(member as usize) {
                        *slot = true;
                    }
                }
            }
            self.comdat_discarded = discarded;
        }

        let count = self.num_local_symbols();
        // SAFETY: initialize_local_symbols fills all `count` slots, including
        // any unused tail, before the parallel allocator publishes the final
        // symbol-table length.
        unsafe {
            allocator.allocate(count, |base_id, slots| {
                self.initialize_sections(args, id);
                self.initialize_local_symbols(id, base_id, slots);
                self.sort_relocations();
                self.sections_parsed = true;
            });
        }
    }

    /// The number of local symbols this file contributes to the symbol
    /// table: the null symbol plus every local not in a discarded COMDAT.
    pub fn num_local_symbols(&self) -> usize {
        if self.base.elf_syms.is_empty() {
            return 0;
        }
        1 + (1..self.base.first_global)
            .filter(|&i| !self.is_discarded_comdat(i))
            .count()
    }

    /// Fills in the local symbols. `slots` has `num_local_symbols()`
    /// entries whose ids start at `base_id`; every one of them is
    /// initialized, those left over with blank symbols.
    pub fn initialize_local_symbols(
        &mut self,
        id: ObjId,
        base_id: SymbolId,
        slots: &mut [MaybeUninit<Symbol>],
    ) {
        let mut next = 0;
        if !slots.is_empty() && !self.base.elf_syms.is_empty() {
            next = self.initialize_local_symbols_into(id, base_id, slots);
        }
        for slot in &mut slots[next..] {
            slot.write(Symbol::new(BStr::new(b"")));
        }
    }

    /// Returns the number of slots written.
    fn initialize_local_symbols_into(
        &mut self,
        id: ObjId,
        base_id: SymbolId,
        slots: &mut [MaybeUninit<Symbol>],
    ) -> usize {
        let file_id = FileId::Obj(id);

        let mut first = Symbol::new(BStr::new(b""));
        first.set_file(file_id);
        first.set_sym_idx(0);
        slots[0].write(first);
        self.base.symbols[0] = base_id;

        let mut next = 1;
        for i in 1..self.base.first_global {
            let esym = &self.base.elf_syms[i];
            if esym.is_common() {
                fatal!("{self}: common local symbol?");
            }
            if self.is_discarded_comdat_sym(i, esym) {
                self.base.symbols[i] = SymbolId::DISCARDED_COMDAT;
                continue;
            }

            let shndx = (!esym.is_abs()).then(|| self.shndx_from(i, esym.st_shndx().get()));

            let name: &'static [u8] = if esym.st_type() == STT_SECTION {
                let shndx = shndx.unwrap();
                match self.section(shndx) {
                    Some(isec) => isec.name(self),
                    None => self.base.section_name(shndx),
                }
            } else {
                self.base.symbol_name_in(i)
            };

            let mut sym = Symbol::new(BStr::new(name));
            sym.set_file(file_id);
            sym.value = esym.st_value().get();
            sym.set_sym_idx(i as u32);
            sym.set_esym(esym);
            sym.set_rust(self.is_rust_obj);
            if let Some(shndx) = shndx {
                if let Some(section) = self.section_id(shndx) {
                    sym.set_input_section(section);
                }
            }
            slots[next].write(sym);
            self.base.symbols[i] = SymbolId(base_id.0 + next as u32);
            next += 1;
        }
        debug_assert_eq!(next, slots.len());
        next
    }

    // .eh_frame contains data records explaining how to handle exceptions.
    // When an exception is thrown, the runtime searches a record from
    // .eh_frame with the current program counter as a key. A record that
    // covers the current PC explains how to find a handler and how to
    // transfer the control ot it.
    //
    // Unlike the most other sections, linker has to parse .eh_frame contents
    // because of the following reasons:
    //
    // - There's usually only one .eh_frame section for each object file,
    //   which explains how to handle exceptions for all functions in the same
    //   object. If we just copy them, the resulting .eh_frame section will
    //   contain lots of records for dead sections (i.e. de-duplicated inline
    //   functions). We want to copy only records for live functions.
    //
    // - .eh_frame contains two types of records: CIE and FDE. There's usually
    //   only one CIE at beginning of .eh_frame section followed by FDEs.
    //   Compiler usually emits the identical CIE record for all object files.
    //   We want to merge identical CIEs in an output .eh_frame section to
    //   reduce the section size.
    //
    // - Scanning a .eh_frame section to find a record is an O(n) operation
    //   where n is the number of records in the section. To reduce it to
    //   O(log n), linker creates a .eh_frame_hdr section. The section
    //   contains a sorted list of [an address in .text, an FDE address whose
    //   coverage starts at the .text address] to make binary search doable.
    //   In order to create .eh_frame_hdr, linker has to read .eh_frame.
    //
    // This function parses an input .eh_frame section.
    pub fn parse_ehframe(&mut self) {
        let eh_frame_sections = std::mem::take(&mut self.eh_frame_sections);
        for &shndx in &eh_frame_sections {
            let isec = self.section_at(shndx);
            let contents = isec.contents();
            let relocations = self.relocation_span(isec.relsec_idx());
            let rels = isec.rels(self);
            let cies_begin = self.cies.len();
            let mut new_cies: Vec<CieRecord> = Vec::new();
            let mut new_fdes: Vec<FdeRecord> = Vec::new();

            // Read CIEs and FDEs until empty.
            let mut rel_idx = 0;
            let mut pos = 0;
            while pos + 4 <= contents.len() {
                let size = E::Endian::read_u32(&contents[pos..]) as usize;
                if size == 0 {
                    break;
                }
                let begin_offset = pos;
                let end_offset = pos + size + 4;
                let id = E::Endian::read_u32(&contents[pos + 4..]);
                pos = end_offset;

                let rel_begin = rel_idx;
                while rel_idx < rels.len() && (rels[rel_idx].r_offset() as usize) < end_offset {
                    rel_idx += 1;
                }

                if id == 0 {
                    // This is CIE.
                    let mut cie = CieRecord {
                        section: shndx,
                        contents,
                        input_offset: begin_offset as u32,
                        output_offset: 0,
                        rel_idx: rel_begin as u32,
                        relocations,
                        icf_idx: 0,
                        fde_ptr_size: 0,
                        is_leader: false,
                    };
                    cie.fde_ptr_size =
                        parse_fde_encoding::<E>(self, isec, &contents[begin_offset..end_offset]);
                    new_cies.push(cie);
                } else {
                    // This is FDE.
                    if rel_begin == rel_idx || rels[rel_begin].r_sym() == 0 {
                        // FDE has no valid relocation, which means FDE is dead from
                        // the beginning. Compilers usually don't create such FDE, but
                        // `ld -r` tend to generate such dead FDEs.
                        continue;
                    }
                    if rels[rel_begin].r_offset() as usize - begin_offset != 8 {
                        fatal!(
                            "{}: FDE's first relocation should have offset 8",
                            isec.display(self)
                        );
                    }
                    // The function may belong to a discarded COMDAT group.
                    if self
                        .symbol_section(rels[rel_begin].r_sym() as usize)
                        .is_none()
                    {
                        continue;
                    }
                    new_fdes.push(FdeRecord::new(begin_offset as u32, rel_begin as u32));
                }
            }

            // Associate CIEs to FDEs.
            for fde in &mut new_fdes {
                let off = fde.input_offset as usize + 4;
                let cie_offset = E::Endian::read_i32(&contents[off..]) as i64;
                let target = off as i64 - cie_offset;
                let Some(ci) = new_cies
                    .iter()
                    .position(|c| c.input_offset as i64 == target)
                else {
                    fatal!("{}: bad FDE pointer", isec.display(self));
                };
                fde.cie_idx = (cies_begin + ci) as u16;
            }

            self.cies.extend(new_cies);
            self.fdes.extend(new_fdes);
            self.kill_section(shndx as usize);
        }
        self.eh_frame_sections = eh_frame_sections;

        // We assume that FDEs for the same input sections are contiguous
        // in `fdes` vector.
        let section_of = |file: &ObjectFile<E>, fde: &FdeRecord| -> usize {
            let rel = fde.rels(file)[0];
            file.shndx_at_in(rel.r_sym() as usize)
        };
        let mut order: Vec<(u64, usize, usize)> = self
            .fdes
            .iter()
            .enumerate()
            .map(|(i, fde)| {
                let shndx = section_of(self, fde);
                (self.section_at(shndx as u32).priority(self), i, shndx)
            })
            .collect();
        order.sort();
        let fdes = std::mem::take(&mut self.fdes);
        let mut sorted: Vec<Option<FdeRecord>> = fdes.into_iter().map(Some).collect();
        self.fdes = order
            .iter()
            .map(|&(_, i, _)| sorted[i].take().unwrap())
            .collect();
        let shndxs: Vec<usize> = order.iter().map(|&(_, _, s)| s).collect();

        // Associate FDEs to input sections.
        let mut i = 0;
        while i < shndxs.len() {
            let begin = i;
            let shndx = shndxs[i];
            while i < shndxs.len() && shndxs[i] == shndx {
                i += 1;
            }
            self.fdes[i - 1].is_last = true;
            if self.section_at(shndx as u32).is_alive() {
                self.section_mut(shndx).unwrap().fde_begin = begin as u32;
            } else {
                for fde in &self.fdes[begin..i] {
                    fde.kill();
                }
            }
        }
    }

    // .sframe is a compact stack-unwinding format. Just like .eh_frame, the
    // linker has to understand its contents: an output .sframe section is a
    // single header followed by a PC-sorted index of function descriptor
    // entries (FDEs) and a blob of frame row entries (FREs). This function
    // parses an input .sframe section into individual FDEs so that we can
    // later drop the dead ones, sort the survivors and rewrite the header.
    //
    // Unlike .eh_frame, the variable-length FRE data carries no relocations,
    // so we copy it to the output verbatim. The only relocated field is each
    // FDE's func_start, which points to the function the FDE describes; we
    // use it both to find the function (for garbage collection and to obtain
    // its output address) and to re-emit a PC-relative offset.
    pub fn parse_sframe(&mut self) {
        let Some(abi) = E::SFRAME_ABI else {
            return;
        };
        let sections = std::mem::take(&mut self.sframe_sections);
        for &shndx in &sections {
            // The input section is consumed by the linker; it's not copied to the
            // output as-is but reconstructed into ctx.sframe.
            self.kill_section(shndx as usize);
            let isec = self.section_at(shndx);
            let data = isec.contents();

            // GNU assembler emits an empty .sframe section for an input file that
            // needs no unwind info (e.g. glibc's Scrt1.o assembled by gas 2.45).
            if data.is_empty() {
                continue;
            }

            if data.len() < SFrameHeader::<E>::size() {
                fatal!("{}: corrupted .sframe section", isec.display(self));
            }
            let hdr = SFrameHeader::<E>::parse(data);
            if hdr.magic.get() != SFRAME_MAGIC {
                fatal!("{}: corrupted .sframe section", isec.display(self));
            }
            if hdr.abi_arch != abi {
                fatal!(
                    "{}: .sframe is incompatible with {}",
                    isec.display(self),
                    E::NAME
                );
            }
            // We support only SFrame Version 3. A section written in an older
            // version (e.g. by an old assembler) isn't an error; we just ignore
            // it, so the functions it covers won't have SFrame info in the output.
            if hdr.version != 3 {
                continue;
            }

            let hdr_len = SFrameHeader::<E>::size() + hdr.auxhdr_len as usize;
            let fde_off = hdr_len + hdr.fdeoff.get() as usize;
            let fre_off = hdr_len + hdr.freoff.get() as usize;
            let rels = isec.rels(self);
            let mut rel_idx = 0;
            let mut new_fdes = Vec::new();

            for i in 0..hdr.num_fdes.get() as usize {
                let idx_off = fde_off + i * SFrameFdeIdx::<E>::size();
                let ent = SFrameFdeIdx::<E>::parse(&data[idx_off..]);

                // Find the relocation for this FDE's func_start field. An FDE without
                // one isn't tied to any function (`ld -r` can emit such dead FDEs),
                // so we drop it.
                while rel_idx < rels.len() && (rels[rel_idx].r_offset() as usize) < idx_off {
                    rel_idx += 1;
                }
                if rel_idx == rels.len() || rels[rel_idx].r_offset() as usize != idx_off {
                    continue;
                }
                let rel = &rels[rel_idx];
                let off = fre_off + ent.func_start_fre_off.get() as usize;

                let Some(func) = self.symbol_section(rel.r_sym() as usize) else {
                    continue;
                };
                let fre = &data[off..off + sframe_fre_block_size::<E>(data, off)];
                new_fdes.push(SFrameFde {
                    section: func.shndx,
                    sym: self.base.symbols[rel.r_sym() as usize],
                    addend: rel.r_addend(),
                    fre,
                    func_size: ent.func_size.get(),
                    num_fres: E::Endian::read_u16(&data[off..]) as u32,
                });
            }
            self.sframe_fdes.extend(new_fdes);
        }
        self.sframe_sections = sections;
    }

    // Create fragment metadata for eligible SHF_MERGE input sections and mark
    // the original sections dead.
    pub fn convert_mergeable_sections(
        &mut self,
        ctx_args: &Args,
        merged: &RwLock<Vec<MergedSection<E>>>,
        cache: &mut MergedSectionCache,
    ) {
        let mut sections = std::mem::take(&mut self.sections);
        for i in 0..sections.len() {
            let Some(isec) = sections.section(i) else {
                continue;
            };
            if isec.sh_size == 0 || isec.has_relsec() || isec.sh_flags & SHF_MERGE as u64 == 0 {
                continue;
            }
            let name = isec.name(self);
            let Some(parent) =
                MergedSection::get_instance(ctx_args, merged, name, &self.base.shdrs[i], cache)
            else {
                continue;
            };
            sections
                .regular_section_mut(i)
                .expect("a regular section")
                .uncompress(self, name, self.base.shdrs[i].sh_size.get() as usize);
            sections.set_merge_info(i, parent);
        }
        self.sections = sections;
    }

    // Usually a section is an atomic unit of inclusion or exclusion.
    // Linker doesn't care about its contents. However, if a section is a
    // mergeable section (a section with SHF_MERGE bit set), the linker is
    // expected to split it into smaller pieces and merge each piece with
    // other pieces from different object files. In mold, we call the
    // atomic unit of mergeable section "section pieces".
    //
    // This feature is typically used for string literals. String literals
    // are usually put into a mergeable section by the compiler. If the same
    // string literal happens to occur in two different translation units,
    // the linker merges them into a single instance of a string, so that
    // the linker's output doesn't contain duplicate string literals.
    //
    // Handling symbols in the mergeable sections is a bit tricky. Assume
    // that we have a mergeable section with the following contents and
    // symbols:
    //
    //   Hello world\0foo bar\0
    //   ^            ^
    //   .rodata      .L.str1
    //   .L.str0
    //
    // '\0' represents a NUL byte. This mergeable section contains two
    // section pieces, "Hello world" and "foo bar". The first string is
    // referred to by two symbols, .rodata and .L.str0, and the second by
    // .L.str1. .rodata is a section symbol and therefore a local symbol
    // and refers to the beginning of the section.
    //
    // In this example, there are actually two different ways to point to
    // string "foo bar", because .rodata+12 and .L.str1+0 refer to the same
    // place in the section. This kind of "out-of-bound" reference occurs
    // only when a symbol is a section symbol. In other words, the compiler
    // may use an offset from the beginning of a section to refer to any
    // section piece in a section, but it doesn't do for any other types
    // of symbols.
    //
    // Section garbage collection and Identical Code Folding work on graphs
    // where sections or section pieces are vertices and relocations are
    // edges. To make it easy to handle them, we rewrite symbols and
    // relocations so that each non-absolute symbol always refers to either
    // a non-mergeable section or a section piece.
    //
    // We do that only for SHF_ALLOC sections because GC and ICF work only
    // on memory-allocated sections. Non-memory-allocated mergeable sections
    // are not handled here for performance reasons.
    pub(crate) fn reattach_section_symbols(
        &self,
        id: ObjId,
        symbols: &SymbolEditor<'_>,
        merged: &[MergedSection<E>],
    ) {
        // Attach section pieces to symbols.
        for i in 1..self.base.elf_syms.len() {
            let esym = &self.base.elf_syms[i];
            if esym.is_abs() || esym.is_common() || esym.is_undef() {
                continue;
            }
            let sym_id = self.base.symbols[i];
            let shndx = self.shndx_from(i, esym.st_shndx().get());
            let Some(m) = self.merge_info(shndx) else {
                continue;
            };
            if !merged[m.parent.index()].resolved {
                continue;
            }
            let Some((frag, offset)) = m.fragment(esym.st_value().get()) else {
                fatal!("{self}: bad symbol value: {}", esym.st_value().get());
            };
            let frag = FragmentRef {
                section: m.parent,
                entry: frag,
            };
            symbols.with_symbol(sym_id, |sym| {
                // If the symbol resolved to a definition in another file, leave it
                // alone. Overwriting it would discard the chosen definition, and since
                // this function runs on all files in parallel, it would also be a data
                // race.
                if sym.file() != Some(FileId::Obj(id)) {
                    return;
                }
                sym.set_fragment(frag);
                sym.value = offset as u64;
            });
        }
    }

    /// Reads only CREL headers to estimate fragment-symbol demand before
    /// archive extraction. This cheap upper bound avoids relocating the entire
    /// central symbol vector when actual fragment symbols are appended later.
    pub(crate) fn crel_fragment_dummy_upper_bound(&self) -> usize {
        self.base
            .shdrs
            .iter()
            .filter(|shdr| shdr.sh_type.get() == SHT_CREL)
            .filter_map(|shdr| {
                let target = self.base.shdrs.get(shdr.sh_info.get() as usize)?;
                (target.sh_flags.get() & SHF_ALLOC as u64 != 0)
                    .then(|| crel_count(self.base.section_contents_from_shdr(shdr)).unwrap_or(0))
            })
            .fold(0, usize::saturating_add)
    }

    // For each relocation referring to a mergeable section symbol, we
    // create a new dummy non-section symbol and redirect the relocation
    // to the newly created symbol.
    pub(crate) fn reattach_fragment_relocations(
        &mut self,
        merged: &[MergedSection<E>],
    ) -> Vec<FragmentSymbol> {
        let mut fragments = Vec::new();

        for shndx in 0..self.sections.len() {
            let (relsec_idx, contents) = {
                let Some(isec) = self.section(shndx) else {
                    continue;
                };
                if !isec.is_alloc() {
                    continue;
                }
                let Some(relsec_idx) = isec.relsec_idx() else {
                    continue;
                };
                (relsec_idx, isec.contents())
            };

            // Rewrite a relocation table in one visit, as the in-memory
            // ElfRel array is updated in place in C++. Ordinary records live
            // in the private input mapping; decoded CREL records remain in
            // the side table.
            let relsec_idx = relsec_idx as usize;
            let mut decoded = self.decoded_crel.get_mut(relsec_idx).and_then(Option::take);
            let rels = match decoded.as_mut() {
                Some(data) => data.as_mut_slice(),
                None => {
                    let base = &self.base;
                    let shdr = &base.shdrs[relsec_idx];
                    let (offset, size) = (shdr.sh_offset.get(), shdr.sh_size.get());
                    let mf = base.mf.expect("input relocations without a mapped file");
                    // SAFETY: this file-parallel pass exclusively owns the
                    // relocation section, whose range was checked at parse
                    // time. No overlapping shared slice is used here.
                    let data =
                        unsafe { mf.data_mut_ptr(offset as usize..(offset + size) as usize) };
                    // SAFETY: the exclusive access described above lasts
                    // until this relocation view is dropped.
                    rels_from_bytes_mut::<E>(unsafe { &mut *data })
                }
            };

            for rel in rels.iter_mut() {
                let record = *rel;
                let r_sym = record.r_sym() as usize;
                if self.base.elf_syms[r_sym].st_type() != STT_SECTION {
                    continue;
                }
                let found = {
                    let esym = &self.base.elf_syms[r_sym];
                    let sym_shndx = self.shndx_from(r_sym, esym.st_shndx().get());
                    self.merge_info(sym_shndx).map(|m| {
                        debug_assert!(merged[m.parent.index()].resolved);
                        let addend = if E::IS_RELA && E::FAMILY != Family::Sh4 {
                            record.r_addend()
                        } else {
                            E::get_addend(&contents[record.r_offset() as usize..], &record)
                        };
                        let Some((frag, in_frag_offset)) =
                            m.fragment(esym.st_value().get().wrapping_add(addend as u64))
                        else {
                            fatal!("{self}: bad relocation at {}", record.r_sym());
                        };
                        (
                            FragmentRef {
                                section: m.parent,
                                entry: frag,
                            },
                            (in_frag_offset - addend) as u64,
                        )
                    })
                };
                let Some((frag, value)) = found else {
                    continue;
                };

                let dummy_idx = (self.base.elf_syms.len() + fragments.len()) as u32;

                fragments.push(FragmentSymbol {
                    fragment: frag,
                    value,
                    sym_idx: record.r_sym(),
                });

                rel.set_r_sym(dummy_idx);
            }

            if let Some(data) = decoded {
                debug_assert!(self.decoded_crel[relsec_idx].is_none());
                self.decoded_crel[relsec_idx] = Some(data);
            }
        }

        fragments
    }

    pub fn scan_relocations(&self, ctx: &Context<E>) {
        // Scan relocations against seciton contents
        for isec in self.input_sections() {
            if isec.is_alive() && isec.is_alloc() {
                E::scan_relocations(ctx, isec);
            }
        }

        // Scan relocations against exception frames
        for cie in &self.cies {
            for rel in cie.rels(self) {
                let sym = &ctx.symbols[self.base.symbols[rel.r_sym() as usize]];
                if ctx.args.pic && rel.r_type() == E::R_ABS {
                    error!("{self}: relocation {} in .eh_frame can not be used when making a position-independent output; recompile with -fPIE or -fPIC",
                        rel.type_name::<E>()
                    );
                }
                if sym.is_imported() {
                    if sym.ty() != STT_FUNC {
                        fatal!("{self}: {sym}: .eh_frame CIE record with an external data reference is not supported"
                        );
                    }
                    sym.add_flags(NEEDS_PLT);
                }
            }
        }
    }

    // Common symbols are used by C's tantative definitions. Tentative
    // definition is an obscure C feature which allows users to omit `extern`
    // from global variable declarations in a header file. For example, if you
    // have a tentative definition `int foo;` in a header which is included
    // into multiple translation units, `foo` will be included into multiple
    // object files, but it won't cause the duplicate symbol error. Instead,
    // the linker will merge them into a single instance of `foo`.
    //
    // If a header file contains a tentative definition `int foo;` and one of
    // a C file contains a definition with initial value such as `int foo = 5;`,
    // then the "real" definition wins. The symbol for the tentative definition
    // will be resolved to the real definition. If there is no "real"
    // definition, the tentative definition gets the default initial value 0.
    //
    // Tentative definitions are represented as "common symbols" in an object
    // file. In this function, we allocate spaces in .common or .tls_common
    // for remaining common symbols that were not resolved to usual defined
    // symbols in previous passes.
    pub fn convert_common_symbols(
        &mut self,
        args: &Args,
        id: ObjId,
        symbols: &mut SymbolTable,
        default_version: u16,
    ) {
        if self.num_common_symbols == 0 {
            return;
        }
        for i in self.base.first_global..self.base.elf_syms.len() {
            let esym = &self.base.elf_syms[i];
            if !esym.is_common() {
                continue;
            }
            let sym_id = self.base.symbols[i];
            let sym = &symbols[sym_id];
            if sym.file() != Some(FileId::Obj(id)) {
                if args.warn_common {
                    warn!("{self}: multiple common symbols: {sym}");
                }
                continue;
            }

            let mut shdr = ElfShdr::<E>::default();
            shdr.sh_type.set(SHT_NOBITS);
            shdr.sh_size.set(esym.st_size().get());
            shdr.sh_addralign.set(esym.st_value().get());
            shdr.sh_flags.set(if sym.ty() == STT_TLS {
                (SHF_ALLOC | SHF_WRITE | SHF_TLS) as u64
            } else {
                (SHF_ALLOC | SHF_WRITE) as u64
            });
            let name: &'static [u8] = if sym.ty() == STT_TLS {
                b".tls_common"
            } else {
                b".common"
            };

            self.elf_sections2.push(shdr);
            let shndx = self.num_elf_sections + self.elf_sections2.len() - 1;
            let isec = InputSection::new(self, id, shndx as u32, &shdr, BStr::new(name));
            let section = self.sections.push(isec);

            let sym = &mut symbols[sym_id];
            sym.set_input_section(section);
            sym.value = 0;
            sym.set_sym_idx(i as u32);
            sym.ver_idx = default_version;
            sym.set_weak(false);
        }
    }

    /// Decides which symbols go to the output symbol table and sizes the
    /// file's block of `.symtab` and `.strtab`.
    pub fn plan_symtab(&self, ctx: &Context<E>, id: ObjId) -> SymtabPlan {
        let mut plan = SymtabPlan {
            output_sym_indices: vec![-1; self.base.elf_syms.len()],
            ..SymtabPlan::default()
        };
        let file_id = FileId::Obj(id);

        // Symbols in dead sections and fragments are dropped along with them.
        let is_alive = |sym: &Symbol| -> bool {
            match sym.origin() {
                OriginValue::Fragment(frag) => ctx.fragment(frag).is_alive(),
                OriginValue::InputSection(section) => ctx.input_section(section).is_alive(),
                _ => true,
            }
        };

        // Compute the size of local symbols
        if !ctx.args.discard_all && !ctx.args.strip_all && ctx.args.retain_symbols_file.is_none() {
            for i in 1..self.base.first_global.min(self.base.elf_syms.len()) {
                if self.is_discarded_comdat(i) {
                    continue;
                }
                let sym_id = self.base.symbols[i];
                let sym = &ctx.symbols[sym_id];
                if is_alive(sym) && should_write_to_local_symtab(ctx, sym) {
                    plan.strtab_size += sym.name().len() as u64 + 1;
                    plan.output_sym_indices[i] = plan.num_local_symtab as i32;
                    plan.num_local_symtab += 1;
                    sym.set_write_to_symtab();
                }
            }
        }

        // Compute the size of global symbols.
        for i in self.base.first_global..self.base.elf_syms.len() {
            let sym_id = self.base.symbols[i];
            let sym = &ctx.symbols[sym_id];
            if sym.file() == Some(file_id)
                && is_alive(sym)
                && (ctx.args.retain_symbols_file.is_none() || sym.write_to_symtab())
            {
                plan.strtab_size += sym.name().len() as u64 + 1;
                // Global symbols can be demoted to local symbols based on visibility,
                // version scripts etc.
                if sym.is_local(ctx) {
                    plan.output_sym_indices[i] = plan.num_local_symtab as i32;
                    plan.num_local_symtab += 1;
                } else {
                    plan.output_sym_indices[i] = plan.num_global_symtab as i32;
                    plan.num_global_symtab += 1;
                }
                sym.set_write_to_symtab();
            }
        }
        plan
    }

    // Returns true if a given section contains a DWARF32 debug record.
    // `isec` must be a .debug_info section.
    pub fn is_dwarf32(&mut self) -> bool {
        let name = display_file(&self.base.filename, &self.archive_name);
        for i in 0..self.debug_info_sections.len() {
            let shndx = self.debug_info_sections[i];
            let isec = self.section_at(shndx);
            let section_name = isec.name(self);
            if isec.sh_size < 12 {
                // The section is too short. This is a user error, but instead of
                // being nitpicky about it, we simply handle it on a garbage-in,
                // garbage-out basis.
                return true;
            }
            let mut buf = [0u8; 12];
            let input_size = self.shdr(shndx as usize).sh_size.get() as usize;
            isec.copy_contents_to(&name, section_name, input_size, &mut buf);
            // A .debug_info section contains compilation units (CUs). A 32-bit CU
            // starts with a 32-bit size field, while a 64-bit CU starts with a
            // magic number 0xffff'ffff followed by a 64-bit size field.
            //
            // Note that size doesn't take the size field itself into account, so
            // the actual size of a 64-bit CU including the size field is 12 bytes
            // larger than the value in the size field.
            if E::Endian::read_u32(&buf) != 0xffff_ffff {
                return true;
            }
            let first_size = E::Endian::read_u64(&buf[4..]) as usize + 12;
            if first_size as u64 == isec.sh_size {
                continue;
            }
            // An input .debug_info section usually contains a single CU. However,
            // if the linker combines multiple object files using `-r`, the
            // resulting object file may have a .debug_info section with as many CUs
            // as there are in the input files. Therefore, if the first CU doesn't
            // cover the entire .debug_info section, we need to keep reading until
            // the end of the section.
            //
            // An input .debug_info section may be compressed using zlib or zstd, so
            // we need to uncompress it before accessing `isec->contents`.
            let isec = self.sections.section_mut(shndx as usize).unwrap();
            isec.uncompress(&name, section_name, input_size);
            let contents = isec.contents();
            let mut p = first_size;
            while contents.len() - p >= 12 {
                if E::Endian::read_u32(&contents[p..]) != 0xffff_ffff {
                    return true;
                }
                p += E::Endian::read_u64(&contents[p + 4..]) as usize + 12;
            }
        }
        false
    }

    /// Produces this file's block of the output symbol table.
    pub fn populate_symtab(&self, ctx: &Context<E>, id: ObjId, block: &mut SymtabBlock<'_>) {
        let id = FileId::Obj(id);

        for i in 1..self.base.first_global.min(self.base.symbols.len()) {
            let sym = &ctx.symbols[self.base.symbols[i]];
            if sym.write_to_symtab() && self.base.output_sym_indices[i] >= 0 {
                block.push_local::<E>(ctx, sym);
            }
        }
        for i in self.base.first_global..self.base.elf_syms.len() {
            let sym = &ctx.symbols[self.base.symbols[i]];
            if sym.file() == Some(id) && sym.write_to_symtab() {
                if sym.is_local(ctx) {
                    block.push_local::<E>(ctx, sym);
                } else {
                    block.push_global::<E>(ctx, sym);
                }
            }
        }
    }
}

/// The layout of a file's block of the output symbol table.
#[derive(Debug, Default)]
pub struct SymtabPlan {
    pub output_sym_indices: Vec<i32>,
    pub num_local_symtab: u32,
    pub num_global_symtab: u32,
    pub strtab_size: u64,
}

impl<E: Arch> InputFile<E> {
    pub fn apply_symtab_plan(&mut self, plan: SymtabPlan) {
        self.output_sym_indices = plan.output_sym_indices;
        self.num_local_symtab = plan.num_local_symtab;
        self.num_global_symtab = plan.num_global_symtab;
        self.strtab_size = plan.strtab_size;
    }
}

/// A file's or chunk's part of `.symtab`, `.strtab` and `.symtab_shndx`:
/// its local symbols, its global symbols and their names, written in
/// place. The parts of different files don't overlap, so they are
/// written in parallel.
pub struct SymtabBlock<'a> {
    locals: SymtabEntries<'a>,
    globals: SymtabEntries<'a>,
    strtab: &'a mut [u8],
    /// The offset of `strtab` within `.strtab`.
    strtab_base: u64,
    strtab_len: usize,
}

/// A run of `.symtab` entries and, if the output has the section, the
/// matching `.symtab_shndx` entries.
pub struct SymtabEntries<'a> {
    syms: &'a mut [u8],
    xindex: Option<&'a mut [u8]>,
    len: usize,
}

impl<'a> SymtabEntries<'a> {
    pub fn new(syms: &'a mut [u8], xindex: Option<&'a mut [u8]>) -> SymtabEntries<'a> {
        SymtabEntries {
            syms,
            xindex,
            len: 0,
        }
    }

    fn push<E: Arch>(&mut self, esym: ElfSym<E>, xindex: u32) {
        let size = std::mem::size_of::<ElfSym<E>>();
        esym.write(&mut self.syms[self.len * size..(self.len + 1) * size]);
        if let Some(entries) = &mut self.xindex {
            E::Endian::write_u32(&mut entries[self.len * 4..], xindex);
        }
        self.len += 1;
    }
}

impl<'a> SymtabBlock<'a> {
    pub fn new(
        locals: SymtabEntries<'a>,
        globals: SymtabEntries<'a>,
        strtab: &'a mut [u8],
        strtab_base: u64,
    ) -> SymtabBlock<'a> {
        SymtabBlock {
            locals,
            globals,
            strtab,
            strtab_base,
            strtab_len: 0,
        }
    }

    /// Zeroes reserved space that was not used by emitted symbols.
    pub fn zero_unused<E: Arch>(&mut self) {
        let size = std::mem::size_of::<ElfSym<E>>();
        self.locals.syms[self.locals.len * size..].fill(0);
        self.globals.syms[self.globals.len * size..].fill(0);
        self.strtab[self.strtab_len..].fill(0);
    }

    /// Adds a name made of `parts`, returning its `.strtab` offset.
    fn add_string(&mut self, parts: &[&[u8]]) -> u32 {
        let offset = self.strtab_base + self.strtab_len as u64;
        for part in parts {
            self.strtab[self.strtab_len..self.strtab_len + part.len()].copy_from_slice(part);
            self.strtab_len += part.len();
        }
        self.strtab[self.strtab_len] = 0;
        self.strtab_len += 1;
        offset as u32
    }

    pub fn push_local<E: Arch>(&mut self, ctx: &Context<E>, sym: &Symbol) {
        let st_name = self.add_string(&[sym.name().as_ref()]);
        let (esym, xindex) = crate::chunks::symtab::to_output_esym(ctx, sym, st_name);
        self.locals.push::<E>(esym, xindex);
    }

    pub fn push_global<E: Arch>(&mut self, ctx: &Context<E>, sym: &Symbol) {
        let st_name = self.add_string(&[sym.name().as_ref()]);
        let (esym, xindex) = crate::chunks::symtab::to_output_esym(ctx, sym, st_name);
        self.globals.push::<E>(esym, xindex);
    }

    /// Adds a synthesized local symbol with a name built from `name` and
    /// `suffix`.
    pub fn push_synthetic<E: Arch>(&mut self, name: &[u8], suffix: &[u8], esym: ElfSym<E>) {
        let st_name = self.add_string(&[name, suffix]);
        let mut esym = esym;
        esym.st_name_mut().set(st_name);
        self.locals.push::<E>(esym, 0);
    }

    /// Adds a local symbol whose name is a fixed `.strtab` entry, such as
    /// an ARM32 mapping symbol.
    pub fn push_mapping_symbol<E: Arch>(&mut self, st_name: u32, esym: ElfSym<E>) {
        let mut esym = esym;
        esym.st_name_mut().set(st_name);
        self.locals.push::<E>(esym, 0);
    }
}

fn should_write_to_local_symtab<E: Arch>(ctx: &Context<E>, sym: &Symbol) -> bool {
    if sym.ty() == STT_SECTION {
        return false;
    }

    // Temporary local symbols such as .L.str.42 are compiler-internal
    // and numerous; a Chromium debug build contains more than two
    // million of them, adding ~180 MiB of .symtab and .strtab. They are
    // not referenced by DWARF, so we discard them by default, as lld
    // does. GNU ld keeps them unless -X is given; --discard-none
    // restores that behavior.
    //
    // Even with --discard-none, we discard temporary symbols in
    // mergeable sections. I *believe* they are excluded because (1)
    // there are too many and (2) they are merged, so their origins
    // shouldn't matter, but I don't really know the rationale. Anyway,
    // this is the behavior of the traditional linkers.
    let name: &[u8] = sym.name();
    if name.starts_with(b".L") || name == b"L0\x01" {
        if ctx.args.discard_locals {
            return false;
        }
        if let Some(isec) = sym.input_section_ref(ctx) {
            if isec.sh_flags & SHF_MERGE as u64 != 0 {
                return false;
            }
        }
    }
    true
}

// Initialize cie's fde_ptr_size member by parsing the augmentation
// string. We need this member to remove FDE records referring to an
// empty segment from the output .eh_frame_hdr.
fn parse_fde_encoding<E: Arch>(file: &ObjectFile<E>, isec: &InputSection<E>, data: &[u8]) -> u8 {
    // Returns the size in bytes of a value in the DWARF exception header
    // encoding `enc`.
    let ptr_size = |enc: u8| -> u8 {
        match enc as u32 & 0xf {
            DW_EH_PE_absptr => E::WORD_SIZE as u8,
            DW_EH_PE_udata4 | DW_EH_PE_sdata4 => 4,
            DW_EH_PE_udata8 | DW_EH_PE_sdata8 => 8,
            _ => fatal!(
                "{}: unsupported FDE pointer encoding: {enc}",
                isec.display(file)
            ),
        }
    };

    // Skip the length, CIE ID and version fields.
    let version = data[8];
    if version != 1 && version != 3 {
        fatal!("{}: unsupported CIE version: {version}", isec.display(file));
    }
    let mut rest = &data[9..];
    // Read the augmentation string
    let aug_len = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
    let aug = &rest[..aug_len];
    rest = &rest[(aug_len + 1).min(rest.len())..];

    let enc = 'enc: {
        // An empty augmentation string means FDE pointers are raw absolute
        // addresses. A string not starting with 'z' denotes some legacy
        // augmentation (e.g. "eh") whose layout we don't know.
        if aug.is_empty() {
            break 'enc DW_EH_PE_absptr as u8;
        }
        if aug[0] != b'z' {
            fatal!(
                "{}: unsupported CIE augmentation string: {}",
                isec.display(file),
                util::display(aug)
            );
        }

        // ULEB128 and SLEB128 values have the same framing, so read_uleb
        // skips both.
        read_uleb(&mut rest); // code alignment factor
        read_uleb(&mut rest); // data alignment factor
        if version == 1 {
            rest = &rest[1..]; // return address register
        } else {
            read_uleb(&mut rest);
        }
        read_uleb(&mut rest); // augmentation data length

        // Walk the augmentation data, looking for 'R', whose data byte
        // specifies how FDE pointers are encoded.
        for &c in &aug[1..] {
            match c {
                b'R' => break 'enc rest[0],
                b'L' => {
                    // A byte specifying the LSDA pointer encoding
                    rest = &rest[1..];
                }
                b'P' => {
                    // A byte specifying the personality function pointer encoding,
                    // followed by the pointer itself
                    rest = &rest[ptr_size(rest[0]) as usize + 1..];
                }
                b'S' | b'B' | b'G' => {
                    // 'S' (signal frame), 'B' (AArch64 pointer authentication) and
                    // 'G' (AArch64 memory tagging) are not followed by data
                }
                _ => fatal!(
                    "{}: unsupported CIE augmentation string: {}",
                    isec.display(file),
                    util::display(aug)
                ),
            }
        }
        // Without 'R', FDE pointers are raw absolute addresses.
        DW_EH_PE_absptr as u8
    };

    // We support only raw absolute and PC-relative pointers.
    if enc & 0xf0 != 0 && enc as u32 & 0xf0 != DW_EH_PE_pcrel {
        fatal!(
            "{}: unsupported FDE pointer encoding: {enc}",
            isec.display(file)
        );
    }
    ptr_size(enc)
}

// Returns the byte length of the SFrame FRE block at offset `offset`:
// a 5-byte attribute header followed by a series of frame row
// entries, each of which is a start address (whose width is given by
// the attribute header), a one-byte info field and a number of
// variable-width data words encoded in that info field.
fn sframe_fre_block_size<E: Arch>(data: &[u8], offset: usize) -> usize {
    let num_fres = E::Endian::read_u16(&data[offset..]) as usize;
    let addr_size = 1usize << bits(data[offset + 2] as u64, 3, 0);
    let mut p = offset + 5;
    for _ in 0..num_fres {
        let info = data[p + addr_size] as u64;
        let num_words = bits(info, 4, 1) as usize;
        let word_size = 1usize << bits(info, 6, 5);
        p += addr_size + 1 + num_words * word_size;
    }
    p - offset
}

// SharedFile represents an input .so file.
#[derive(Debug)]
pub struct SharedFile<E: Layout> {
    pub base: InputFile<E>,
    pub soname: Vec<u8>,
    pub version_strings: Vec<&'static [u8]>,

    /// For each symbol, the `foo@VERSION` alias of a default-versioned
    /// definition `foo@@VERSION`.
    pub symbols2: Vec<SymbolId>,
    pub versyms: Vec<u16>,

    // Used by get_symbols_at()
    sorted_syms: OnceLock<Vec<SymbolId>>,
}

impl<E: Layout> fmt::Display for SharedFile<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", path_clean(&self.base.filename))
    }
}

impl<E: Layout> FileInPool for SharedFile<E> {
    fn set_file_index(&mut self, index: u32) {
        self.base.file_index = index;
    }
}

impl<E: Arch> SharedFile<E> {
    pub fn id(&self) -> DsoId {
        DsoId(self.base.file_index)
    }

    pub(crate) fn new(mf: &'static MappedFile, bins: &mut Bins<SymbolSlot>) -> SharedFile<E> {
        let base =
            InputFile::<E>::parse(mf, &display_file(&mf.name.to_string_lossy(), Path::new("")));
        let mut file = SharedFile {
            base,
            soname: Vec::new(),
            version_strings: Vec::new(),
            symbols2: Vec::new(),
            versyms: Vec::new(),
            sorted_syms: OnceLock::new(),
        };
        file.parse(bins);
        file
    }

    /// The strings of the dynamic entries with the given tag, such as the
    /// DT_NEEDED libraries.
    fn dynamic_strings(&self, tag: u64) -> Vec<&'static [u8]> {
        let Some(idx) = self.base.find_section(SHT_DYNAMIC) else {
            return Vec::new();
        };
        let shdr = &self.base.shdrs[idx];
        let strtab = self.base.section_contents(shdr.sh_link.get() as usize);
        ElfDyn::<E>::parse_all(self.base.section_contents(idx))
            .into_iter()
            .filter(|entry| entry.d_tag.get() == tag)
            .map(|entry| cstr_at(strtab, entry.d_val.get() as usize))
            .collect()
    }

    fn get_soname(&self) -> Vec<u8> {
        if let Some(soname) = self.dynamic_strings(DT_SONAME as u64).first() {
            return soname.to_vec();
        }
        if let Some(mf) = self.base.mf {
            let name = if mf.given_fullpath {
                mf.name.as_os_str()
            } else {
                mf.name.file_name().unwrap_or_default()
            };
            return name.as_encoded_bytes().to_vec();
        }
        self.base.filename.as_bytes().to_vec()
    }

    fn parse(&mut self, bins: &mut Bins<SymbolSlot>) {
        let Some(symtab_idx) = self.base.find_section(SHT_DYNSYM) else {
            return;
        };
        let symtab_shdr = &self.base.shdrs[symtab_idx];
        self.base.symbol_strtab = self
            .base
            .section_contents(symtab_shdr.sh_link.get() as usize);
        self.soname = self.get_soname();
        self.version_strings = self.read_version_strings();

        // Read a symbol table.
        let esyms = records_from_bytes::<ElfSym<E>>(self.base.section_contents(symtab_idx));
        let first = symtab_shdr.sh_info.get() as usize;
        if esyms.len() < first {
            fatal!("{self}: invalid symbol table");
        }
        // Only the symbols this file exports are kept, so the table is
        // rebuilt rather than read in place.
        let num_syms = esyms.len() - first;
        self.base.elf_syms = Cow::Owned(Vec::with_capacity(num_syms));
        self.versyms.reserve(num_syms);
        // These reservations keep recorded slots stable until gather, even
        // while parsing appends symbols. FileList retains the backing files
        // when duplicate SONAMEs are removed from its live list.
        self.base.symbols.reserve(num_syms);
        self.symbols2.reserve(num_syms);

        let vers: Vec<u16> = match self.base.find_section(SHT_GNU_VERSYM) {
            Some(idx) => self
                .base
                .section_contents(idx)
                .chunks_exact(2)
                .map(E::Endian::read_u16)
                .collect(),
            None => Vec::new(),
        };

        for i in first..esyms.len() {
            let esym = esyms[i];
            let mut ver = if vers.is_empty() {
                VER_NDX_GLOBAL as u16
            } else {
                vers[i] & !(VERSYM_HIDDEN as u16)
            };

            // A version index of 0 (VER_NDX_LOCAL) is valid only for unversioned
            // undefined symbols. A symbol that's actually local to a DSO doesn't
            // appear in .dynsym in the first place, so index 0 on a defined
            // symbol is ill-formed. GNU ld briefly emitted it
            // (https://sourceware.org/bugzilla/show_bug.cgi?id=33577). We used
            // to silently ignore such symbols, which turned into confusing
            // "undefined symbol" errors down the line. Reject the file instead,
            // like lld.
            if ver as u32 == VER_NDX_LOCAL {
                if !esym.is_undef() {
                    fatal!(
                        "{self}: invalid version index 0 for defined symbol {}",
                        util::display(cstr_at(
                            self.base.symbol_strtab,
                            esym.st_name().get() as usize
                        ))
                    );
                }
                ver = VER_NDX_GLOBAL as u16;
            }

            // A definition whose versym is exactly VERSYM_HIDDEN | VER_NDX_GLOBAL
            // is a compatibility alias that exists only for binaries linked before
            // the library adopted symbol versioning. GNU ld creates one for
            // `.symver foo_impl, foo@`. New references must not bind to it; skip
            // it so that an unversioned reference binds to the default-versioned
            // `foo@@VERSION` definition instead. Versyms of undefined symbols
            // encode required versions, so they are exempt.
            if !vers.is_empty()
                && vers[i] == (VERSYM_HIDDEN | VER_NDX_GLOBAL) as u16
                && !esym.is_undef()
            {
                continue;
            }

            self.base.elf_syms.to_mut().push(esym);
            // resolve_symbols only consults versyms[] for defined symbols
            // (see SharedFile::resolve_symbols), so VER_NDX_GLOBAL is fine
            // for undefined entries.
            self.versyms.push(if esym.is_undef() {
                VER_NDX_GLOBAL as u16
            } else {
                ver
            });

            let name = cstr_at(self.base.symbol_strtab, esym.st_name().get() as usize);
            let has_version = ver as u32 != VER_NDX_GLOBAL
                && (ver as usize) < self.version_strings.len()
                && !self.version_strings[ver as usize].is_empty();

            let versioned_key = || {
                leak_bytes([name, b"@", self.version_strings[ver as usize]].concat())
            };

            // Symbol resolution involving symbol versioning is tricky because one
            // symbol can be resolved with two different identifiers. Among
            // symbols with the same name but different versions, one of them is
            // always marked as the "default" one. This symbol is often denoted
            // with two atsigns as `foo@@VERSION` and can be referred to either
            // as `foo` or `foo@VERSION`. No other symbols have two names like that.
            //
            // On contrary, a versioned non-default symbol can be referred only
            // with an explicit version suffix, e.g., `foo@VERSION`.
            //
            // Here is how we resolve versioned default symbols. We resolve `foo`
            // and `foo@VERSION` as usual, but with information to forward
            // references to `foo@VERSION` to `foo`. After name resolution, we
            // visit all symbol references to redirect `foo@VERSION` to `foo`.
            let (key, alias) = if !has_version {
                // Unversioned symbol
                (name, None)
            } else if esym.is_undef() || vers[i] & VERSYM_HIDDEN as u16 != 0 {
                // Versioned non-default symbol, or undefined reference whose
                // version comes from .gnu.version_r.
                (versioned_key(), None)
            } else {
                // Versioned default symbol
                (name, Some(versioned_key()))
            };
            self.base.symbols.push(SymbolId::DISCARDED_COMDAT);
            self.symbols2.push(SymbolId::NONE);
            bins.record(
                key,
                name.len(),
                SymbolSlot::new(self.base.symbols.last_mut().unwrap()),
            );
            if let Some(key) = alias {
                bins.record(
                    key,
                    name.len(),
                    SymbolSlot::new(self.symbols2.last_mut().unwrap()),
                );
            }
        }

        self.base.first_global = 0;

        static COUNTER: Counter = Counter::new("dso_syms");
        COUNTER.add(self.base.elf_syms.len() as i64);
    }

    pub fn dt_needed(&self) -> Vec<&'static [u8]> {
        self.dynamic_strings(DT_NEEDED as u64)
    }

    pub fn dt_audit(&self) -> &'static [u8] {
        self.dynamic_strings(DT_AUDIT as u64)
            .first()
            .copied()
            .unwrap_or(b"")
    }

    // Symbol versioning is a GNU extension to the ELF file format. I don't
    // particularly like the feature as it complicates the semantics of
    // dynamic linking, but we need to support it anyway because it is
    // mandatory on glibc-based systems such as most Linux distros.
    //
    // Let me explain what symbol versioning is. Symbol versioning is a
    // mechanism to allow multiple symbols of the same name but of different
    // versions live together in a shared object file. It's convenient if you
    // want to make an API-breaking change to some function but want to keep
    // old programs working with the newer libraries.
    //
    // With symbol versioning, dynamic symbols are resolved by (name, version)
    // tuple instead of just by name. For example, glibc 2.35 defines two
    // different versions of `posix_spawn`, `posix_spawn` of version
    // "GLIBC_2.15" and that of version "GLIBC_2.2.5". Any executable that
    // uses `posix_spawn` is linked either to that of "GLIBC_2.15" or that of
    // "GLIBC_2.2.5"
    //
    // Versions are just strings, and no ordering is defined between them.
    // For example, "GLIBC_2.15" is not considered a newer version of
    // "GLIBC_2.2.5" or vice versa. They are considered just different.
    //
    // If a shared object file has versioned symbols, it contains a parallel
    // array for the symbol table. Version strings can be found in that
    // parallel table.
    //
    // One version is considered the "default" version for each shared object.
    // If an undefiend symbol `foo` is resolved to a symbol defined by the
    // shared object, it's marked so that it'll be resolved to (`foo`, the
    // default version of the library) at load-time.
    //
    // Reads .gnu.version_d and .gnu.version_r and returns a vector of
    // version names indexed by versym value. The versym index space is
    // shared between the two sections (vd_ndx for defined symbols and
    // vna_other for undefined ones), so a single vector covers both.
    fn read_version_strings(&self) -> Vec<&'static [u8]> {
        let mut vec: Vec<&'static [u8]> = Vec::new();
        let mut set = |idx: usize, name: &'static [u8]| {
            if vec.len() <= idx {
                vec.resize(idx + 1, b"");
            }
            vec[idx] = name;
        };

        if let Some(idx) = self.base.find_section(SHT_GNU_VERDEF) {
            let verdef = self.base.section_contents(idx);
            let strtab = self
                .base
                .section_contents(self.base.shdrs[idx].sh_link.get() as usize);
            let mut pos = 0;
            loop {
                let ver = ElfVerdef::<E>::parse(&verdef[pos..]);
                if u32::from(ver.vd_ndx.get()) == VER_NDX_UNSPECIFIED {
                    fatal!("{self}: symbol version too large");
                }
                let aux = ElfVerdaux::<E>::parse(&verdef[pos + ver.vd_aux.get() as usize..]);
                set(
                    ver.vd_ndx.get() as usize,
                    cstr_at(strtab, aux.vda_name.get() as usize),
                );
                if ver.vd_next.get() == 0 {
                    break;
                }
                pos += ver.vd_next.get() as usize;
            }
        }

        if let Some(idx) = self.base.find_section(SHT_GNU_VERNEED) {
            let verneed = self.base.section_contents(idx);
            let strtab = self
                .base
                .section_contents(self.base.shdrs[idx].sh_link.get() as usize);
            let mut pos = 0;
            loop {
                let vn = ElfVerneed::<E>::parse(&verneed[pos..]);
                let mut aux_pos = pos + vn.vn_aux.get() as usize;
                for _ in 0..vn.vn_cnt.get() {
                    let aux = ElfVernaux::<E>::parse(&verneed[aux_pos..]);
                    let idx = (aux.vna_other.get() & !(VERSYM_HIDDEN as u16)) as usize;
                    set(idx, cstr_at(strtab, aux.vna_name.get() as usize));
                    if aux.vna_next.get() == 0 {
                        break;
                    }
                    aux_pos += aux.vna_next.get() as usize;
                }
                if vn.vn_next.get() == 0 {
                    break;
                }
                pos += vn.vn_next.get() as usize;
            }
        }
        vec
    }

    /// The symbols this file defines at the same address as `sym`.
    pub fn symbols_at(&self, ctx: &Context<E>, sym: &Symbol, id: DsoId) -> &[SymbolId] {
        let sorted = self.sorted_syms.get_or_init(|| {
            let mut syms: Vec<SymbolId> = self
                .base
                .symbols
                .iter()
                .copied()
                .filter(|&s| ctx.symbols[s].file() == Some(FileId::Dso(id)))
                .collect();
            syms.sort_by_key(|&s| (ctx.symbols[s].esym(ctx).st_value().get(), s));
            syms
        });
        let value = sym.esym(ctx).st_value().get();
        let begin = sorted.partition_point(|&s| ctx.symbols[s].esym(ctx).st_value().get() < value);
        let end = sorted.partition_point(|&s| ctx.symbols[s].esym(ctx).st_value().get() <= value);
        &sorted[begin..end]
    }

    // Infer an alignment of a DSO symbol. An alignment of a symbol in other
    // .so is not something we usually care about, but when we create a copy
    // relocation for a symbol, we need to preserve its alignment requirement.
    //
    // Symbol alignment is not explicitly represented in an ELF file. In this
    // function, we conservatively infer it from a symbol address and a
    // section alignment requirement.
    pub fn alignment(&self, sym: &Symbol) -> u64 {
        let shndx = self.base.elf_syms[sym.sym_idx() as usize].st_shndx().get() as usize;
        let shdr = &self.base.shdrs[shndx];
        let mut align = shdr.sh_addralign.get().max(1);
        if sym.value != 0 {
            align = align.min(1 << sym.value.trailing_zeros());
        }
        align
    }

    /// Whether a symbol lives in a read-only segment.
    pub fn is_readonly(&self, sym: &Symbol) -> bool {
        let data = self.base.data();
        let ehdr = record_from_bytes::<ElfEhdr<E>>(data);
        let val = self.base.elf_syms[sym.sym_idx() as usize].st_value().get();
        let phoff = ehdr.e_phoff.get() as usize;
        let size = std::mem::size_of::<ElfPhdr<E>>();
        let phnum = ehdr.e_phnum.get() as usize;
        let phdrs = records_from_bytes::<ElfPhdr<E>>(&data[phoff..phoff + phnum * size]);
        phdrs.iter().any(|phdr| {
            (phdr.p_type().get() == PT_LOAD || phdr.p_type().get() == PT_GNU_RELRO)
                && phdr.p_flags().get() & PF_W == 0
                && phdr.p_vaddr().get() <= val
                && val < phdr.p_vaddr().get() + phdr.p_memsz().get()
        })
    }

    pub fn plan_symtab(&self, ctx: &Context<E>, id: DsoId) -> SymtabPlan {
        let mut plan = SymtabPlan {
            output_sym_indices: vec![-1; self.base.elf_syms.len()],
            ..SymtabPlan::default()
        };
        // Compute the size of global symbols.
        for i in 0..self.base.symbols.len() {
            let sym_id = self.base.symbols[i];
            let sym = &ctx.symbols[sym_id];
            if sym.file() == Some(FileId::Dso(id))
                && (sym.is_imported() || sym.is_exported())
                && (ctx.args.retain_symbols_file.is_none() || sym.write_to_symtab())
            {
                plan.strtab_size += sym.name().len() as u64 + 1;
                plan.output_sym_indices[i] = plan.num_global_symtab as i32;
                plan.num_global_symtab += 1;
                sym.set_write_to_symtab();
            }
        }
        plan
    }

    pub fn populate_symtab(&self, ctx: &Context<E>, id: DsoId, block: &mut SymtabBlock<'_>) {
        for &sym_id in &self.base.symbols {
            let sym = &ctx.symbols[sym_id];
            if sym.file() == Some(FileId::Dso(id)) && sym.write_to_symtab() {
                block.push_global::<E>(ctx, sym);
            }
        }
    }
}

// Symbols with higher priorities overwrites symbols with lower priorities.
// Here is the list of priorities, from the highest to the lowest.
//
//  1. Strong defined symbol
//  2. Weak defined symbol
//  3. Strong defined symbol in a DSO/archive
//  4. Weak Defined symbol in a DSO/archive
//  5. Common symbol
//  6. Common symbol in an archive
//  7. Unclaimed (nonexistent) symbol
//
// Ties are broken by file priority.
//
// Note that the above priorities are based on heuristics and not on exact
// science. We tried several different orders and settled on the current
// one just because it avoids link errors in all programs we've tested.
#[inline]
fn symbol_rank_from_fields(
    is_common: bool,
    st_bind: u32,
    is_dso: bool,
    is_in_archive: bool,
) -> u64 {
    if is_common {
        debug_assert!(!is_dso);
        return if is_in_archive { 6 } else { 5 };
    }
    if is_dso || is_in_archive {
        return if st_bind == STB_WEAK { 4 } else { 3 };
    }
    if st_bind == STB_WEAK {
        2
    } else {
        1
    }
}

#[inline]
pub fn symbol_rank<R: SymbolRecord>(esym: &R, is_dso: bool, is_in_archive: bool) -> u64 {
    symbol_rank_from_fields(esym.is_common(), esym.st_bind(), is_dso, is_in_archive)
}

/// The rank used to choose a symbol definition. Ties in definition
/// strength are broken in favor of the file occurring first on the command
/// line.
#[inline]
pub fn symbol_resolution_rank<R: SymbolRecord>(
    esym: &R,
    is_dso: bool,
    is_in_archive: bool,
    priority: u32,
) -> u64 {
    (symbol_rank(esym, is_dso, is_in_archive) << 32) | priority as u64
}

/// Returns the resolution rank of a symbol that already owns a definition.
/// `Symbol` caches the defining ELF symbol's fields, so we don't need to
/// decode that record again for every competing definition.
#[inline]
pub fn resolved_symbol_rank(sym: &Symbol, is_dso: bool, is_in_archive: bool, priority: u32) -> u64 {
    (symbol_rank_from_fields(sym.is_common(), sym.st_bind(), is_dso, is_in_archive) << 32)
        | priority as u64
}

/// An exclusive view of the symbol table shared by file-parallel passes.
/// Individual symbols are serialized by their byte-sized spin locks.
pub(crate) struct SymbolEditor<'a> {
    symbols: *mut Symbol,
    len: usize,
    _symbols: PhantomData<&'a mut [Symbol]>,
}

// The exclusive slice represented by `symbols` cannot be accessed except
// through `with_symbol`, which serializes accesses to each element.
unsafe impl Sync for SymbolEditor<'_> {}

impl<'a> SymbolEditor<'a> {
    pub(crate) fn new(symbols: &'a mut [Symbol]) -> SymbolEditor<'a> {
        SymbolEditor {
            symbols: symbols.as_mut_ptr(),
            len: symbols.len(),
            _symbols: PhantomData,
        }
    }

    #[inline]
    pub(crate) fn with_symbol<R>(&self, id: SymbolId, f: impl FnOnce(&mut Symbol) -> R) -> R {
        debug_assert!(id.index() < self.len);
        // SAFETY: the editor owns an exclusive borrow of the whole table,
        // and Symbol::with_resolution_lock serializes accesses to this slot.
        unsafe { Symbol::with_resolution_lock(self.symbols.add(id.index()), f) }
    }

    #[inline]
    pub(crate) fn skip_dso(&self, id: SymbolId) -> bool {
        debug_assert!(id.index() < self.len);
        // SAFETY: the editor owns a live symbol array; skip_dso_at reads only
        // the atomic state byte shared with the resolution lock.
        unsafe { Symbol::skip_dso_at(self.symbols.add(id.index())) }
    }
}

/// The files and defaults needed while editing symbols during resolution.
pub struct SymbolResolver<'a, E: Arch> {
    editor: SymbolEditor<'a>,
    objs: &'a FileList<ObjectFile<E>>,
    dsos: &'a FileList<SharedFile<E>>,
    default_version: u16,
}

// SymbolResolver's mutable symbol-table access is serialized by its editor.
unsafe impl<E: Arch> Sync for SymbolResolver<'_, E> {}

impl<'a, E: Arch> SymbolResolver<'a, E> {
    pub fn new(
        symbols: &'a mut [Symbol],
        objs: &'a FileList<ObjectFile<E>>,
        dsos: &'a FileList<SharedFile<E>>,
        default_version: u16,
    ) -> SymbolResolver<'a, E> {
        SymbolResolver {
            editor: SymbolEditor::new(symbols),
            objs,
            dsos,
            default_version,
        }
    }

    #[inline]
    fn with_symbol<R>(&self, id: SymbolId, f: impl FnOnce(&mut Symbol) -> R) -> R {
        self.editor.with_symbol(id, f)
    }

    #[inline]
    fn skip_dso(&self, id: SymbolId) -> bool {
        self.editor.skip_dso(id)
    }

    #[inline]
    fn current_rank(&self, sym: &Symbol) -> u64 {
        let Some(file) = sym.file() else {
            return 7 << 32;
        };
        let base = match file {
            FileId::Obj(id) => &self.objs[id.index()].base,
            FileId::Dso(id) => &self.dsos[id.index()].base,
        };
        resolved_symbol_rank(sym, file.is_dso(), !base.is_reachable(), base.priority)
    }
}

impl<E: Arch> ObjectFile<E> {
    pub fn resolve_symbols(&self, resolver: &SymbolResolver<'_, E>, id: ObjId) {
        let in_archive = !self.base.is_reachable();
        for i in self.base.first_global..self.base.elf_syms.len() {
            let sym_id = self.base.symbols[i];
            self.resolve_symbol(resolver, id, i, sym_id, in_archive);
        }
    }

    /// Resolves only symbols marked to ignore DSO definitions. This is the
    /// rare hidden-symbol retry; filtering before `resolve_symbol` avoids
    /// decoding and locking every other definition, as in C++.
    pub fn resolve_skip_dso_symbols(&self, resolver: &SymbolResolver<'_, E>, id: ObjId) {
        let in_archive = !self.base.is_reachable();
        for i in self.base.first_global..self.base.elf_syms.len() {
            let sym_id = self.base.symbols[i];
            if resolver.skip_dso(sym_id) {
                self.resolve_symbol(resolver, id, i, sym_id, in_archive);
            }
        }
    }

    // Makes this file's i'th symbol the definition of the global symbol it
    // refers to if it is the best one seen so far.
    fn resolve_symbol(
        &self,
        resolver: &SymbolResolver<'_, E>,
        id: ObjId,
        i: usize,
        sym_id: SymbolId,
        in_archive: bool,
    ) {
        let esym = &self.base.elf_syms[i];
        if esym.is_undef() {
            return;
        }

        // Before sections are parsed, pre-liveness resolution treats all
        // definitions as live. The final round uses the actual section state.
        let mut origin = None;
        if !esym.is_abs() && !esym.is_common() && self.sections_parsed {
            let shndx = self.shndx_from(i, esym.st_shndx().get());
            let Some((section, isec)) = self.section_with_id(shndx) else {
                return;
            };
            if !isec.is_alive() {
                return;
            }
            origin = Some(section);
        }

        let rank = symbol_resolution_rank(esym, false, in_archive, self.base.priority);
        resolver.with_symbol(sym_id, |sym| {
            if rank < resolver.current_rank(sym) {
                sym.set_file(FileId::Obj(id));
                match origin {
                    Some(section) => sym.set_input_section(section),
                    None => sym.clear_origin(),
                }
                sym.value = esym.st_value().get();
                sym.set_sym_idx(i as u32);
                sym.set_esym(esym);
                sym.ver_idx = resolver.default_version;
                sym.set_weak(esym.is_weak());
                sym.set_versioned_default(false);
                sym.set_rust(self.is_rust_obj);
            }
        });
    }
}

impl<E: Arch> SharedFile<E> {
    /// Resolves this shared library's definitions in place, including the
    /// forwarding aliases for default symbol versions.
    pub fn resolve_symbols(&self, resolver: &SymbolResolver<'_, E>, id: DsoId) {
        for i in 0..self.base.elf_syms.len() {
            let esym = &self.base.elf_syms[i];
            let sym_id = self.base.symbols[i];
            if esym.is_undef() || resolver.skip_dso(sym_id) {
                continue;
            }

            let rank = symbol_resolution_rank(esym, true, false, self.base.priority);
            resolver.with_symbol(sym_id, |sym| {
                if rank < resolver.current_rank(sym) {
                    sym.set_file(FileId::Dso(id));
                    sym.clear_origin();
                    sym.value = esym.st_value().get();
                    sym.set_sym_idx(i as u32);
                    sym.set_esym(esym);
                    sym.ver_idx = self.versyms[i];
                    sym.set_weak(true);
                    sym.set_versioned_default(false);
                    sym.set_rust(false);
                }
            });

            // A symbol with the default version is a special case because, unlike
            // other symbols, the symbol can be referred by two names, `foo` and
            // `foo@VERSION`. Here, we resolve `foo@VERSOIN` as a proxy of `foo`.
            let alias_id = self.symbols2[i];
            if alias_id != SymbolId::NONE && alias_id != sym_id {
                resolver.with_symbol(alias_id, |sym| {
                    if rank < resolver.current_rank(sym) {
                        sym.set_file(FileId::Dso(id));
                        sym.set_symbol_origin(sym_id);
                        sym.set_sym_idx(i as u32);
                        sym.set_esym(esym);
                        sym.set_rust(false);
                        sym.set_versioned_default(true);
                    }
                });
            }
        }
    }
}

/// Prints a `--trace-symbol` line for a reference or definition.
pub fn print_trace_symbol<R: SymbolRecord>(file: &dyn fmt::Display, esym: &R, sym: &Symbol) {
    if !esym.is_undef() {
        out!("trace-symbol: {file}: definition of {sym}");
    } else if esym.is_weak() {
        out!("trace-symbol: {file}: weak reference to {sym}");
    } else {
        out!("trace-symbol: {file}: reference to {sym}");
    }
}
