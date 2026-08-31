//! The passes of a link, in roughly the order the driver runs them.

use std::borrow::Cow;
use std::cell::UnsafeCell;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use bstr::BStr;
use rayon::prelude::*;

use crate::arch::{Arch, Family};
use crate::cmdline::{
    BsymbolicKind, BuildIdKind, CetReportKind, DefsymValue, SectionOrderKind, SeparateCodeKind,
    ShuffleSectionsKind, UnresolvedKind,
};
use crate::context::Context;
use crate::elf::*;
use crate::input_files::{
    resolved_symbol_rank, symbol_resolution_rank, ComdatGroupRef, FileId, FileList, ObjId,
    ObjectFile, SymbolEditor, SymbolResolver,
};
use crate::input_sections::{InputSectionId, SectionRef};
use crate::linker_script::VersionPattern;
use crate::mapped_file::MappedFile;
use crate::output_chunks::dynamic::{DynamicSection, RelrDynSection};
use crate::output_chunks::eh_frame::{EhFrameHdrSection, EhFrameRelocSection};
use crate::output_chunks::misc::{
    self, BuildIdSection, GnuDebuglinkSection, InterpSection, NotePropertySection,
    RelroPaddingSection,
};
use crate::output_chunks::output_section::OutputSection;
use crate::output_chunks::symtab::{
    self, GnuHashSection, HashSection, ShstrtabSection, SymtabShndxSection,
};
use crate::output_chunks::version::VerdefSection;
use crate::output_chunks::{
    self, ChunkHeader, ChunkId, GdbIndexSection, OutputEhdr, OutputPhdr, OutputSectionId,
    OutputShdr,
};
use crate::output_file::OutputFile;
use crate::symbol::{
    is_c_identifier, Bins, Symbol, SymbolId, NEEDS_CANONICAL, NEEDS_GOT, NEEDS_GOTTP, NEEDS_PLT,
    NEEDS_PPC_OPD, NEEDS_TLSDESC, NEEDS_TLSGD,
};
use crate::util::glob::Glob;
use crate::util::{align_down, align_to, leak_bytes, path_filename};
use crate::{error, fatal, out, warn};

pub fn apply_exclude_libs<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("apply_exclude_libs");
    let set = &ctx.args.exclude_libs;
    if set.is_empty() {
        return;
    }
    for file in &mut ctx.objs {
        if !file.archive_name.is_empty()
            && (set.contains(&path_filename(&file.archive_name)) || set.contains("ALL"))
        {
            file.exclude_libs = true;
        }
    }
}

fn has_debug_info_section<E: Arch>(ctx: &Context<E>) -> bool {
    ctx.objs.iter().any(|f| !f.debug_info_sections.is_empty())
}

pub fn create_synthetic_sections<E: Arch>(ctx: &mut Context<E>) {
    let mut chunks = Vec::new();

    if !ctx.args.oformat_binary {
        let find = |name: &str| {
            ctx.args
                .section_order
                .iter()
                .any(|o| o.kind == SectionOrderKind::Section && o.name == name)
        };
        let ehdr_flags = if ctx.args.section_order.is_empty() || find("EHDR") {
            SHF_ALLOC as u64
        } else {
            0
        };
        let phdr_flags = if ctx.args.section_order.is_empty() || find("PHDR") {
            SHF_ALLOC as u64
        } else {
            0
        };
        ctx.ehdr = Some(OutputEhdr::<E>::new(ehdr_flags));
        chunks.push(ChunkId::Ehdr);
        ctx.phdr = Some(OutputPhdr::<E>::new(phdr_flags));
        chunks.push(ChunkId::Phdr);
        if ctx.args.z_sectionheader {
            ctx.shdr = Some(OutputShdr::<E>::new());
            chunks.push(ChunkId::Shdr);
        }
    }

    chunks.push(ChunkId::Got);
    if !E::IS_SPARC {
        chunks.push(ChunkId::GotPlt);
    }
    chunks.push(ChunkId::RelDyn);
    chunks.push(ChunkId::RelPlt);
    if ctx.args.pack_dyn_relocs_relr {
        ctx.relrdyn = Some(RelrDynSection::<E>::new(&ctx.args));
        chunks.push(ChunkId::RelrDyn);
    }
    chunks.push(ChunkId::Strtab);
    chunks.push(ChunkId::Plt);
    chunks.push(ChunkId::PltGot);
    chunks.push(ChunkId::Symtab);
    chunks.push(ChunkId::Dynsym);
    chunks.push(ChunkId::Dynstr);
    chunks.push(ChunkId::EhFrame);
    chunks.push(ChunkId::SFrame);
    chunks.push(ChunkId::Copyrel);
    chunks.push(ChunkId::CopyrelRelro);

    if ctx.shdr.is_some() {
        ctx.shstrtab = Some(ShstrtabSection::new());
        chunks.push(ChunkId::Shstrtab);
    }
    if !ctx.args.dynamic_linker.is_empty() {
        ctx.interp = Some(InterpSection::new());
        chunks.push(ChunkId::Interp);
    }
    if ctx.args.build_id.kind != BuildIdKind::None {
        ctx.buildid = Some(BuildIdSection::new());
        chunks.push(ChunkId::BuildId);
    }
    if ctx.args.eh_frame_hdr {
        ctx.eh_frame_hdr = Some(EhFrameHdrSection::new());
        chunks.push(ChunkId::EhFrameHdr);
    }
    if ctx.args.gdb_index && has_debug_info_section(ctx) {
        ctx.gdb_index = Some(GdbIndexSection::new());
        chunks.push(ChunkId::GdbIndex);
    }
    if ctx.args.z_relro && ctx.args.section_order.is_empty() {
        ctx.relro_padding = Some(RelroPaddingSection::new());
        chunks.push(ChunkId::RelroPadding);
    }
    if ctx.args.hash_style_sysv {
        ctx.hash = Some(HashSection::<E>::new());
        chunks.push(ChunkId::Hash);
    }
    if ctx.args.hash_style_gnu {
        ctx.gnu_hash = Some(GnuHashSection::<E>::new());
        chunks.push(ChunkId::GnuHash);
    }
    if !ctx.args.version_definitions.is_empty() {
        ctx.verdef = Some(VerdefSection::new());
        chunks.push(ChunkId::Verdef);
    }
    if ctx.args.emit_relocs {
        ctx.eh_frame_reloc = Some(EhFrameRelocSection::<E>::new());
        chunks.push(ChunkId::EhFrameReloc);
    }
    if !ctx.args.separate_debug_file.is_empty() {
        ctx.gnu_debuglink = Some(GnuDebuglinkSection::new());
        chunks.push(ChunkId::GnuDebuglink);
    }

    if ctx.args.shared || !ctx.dsos.is_empty() || ctx.args.pie {
        ctx.dynamic = Some(DynamicSection::<E>::new(&ctx.args));
        chunks.push(ChunkId::Dynamic);
        // If .dynamic exists, .dynsym and .dynstr must exist as well
        // since .dynamic refers to them.
        ctx.dynstr.add_string(b"");
        if ctx.dynsym.symbols.is_empty() {
            ctx.dynsym.symbols.push(None);
        }
    }

    chunks.push(ChunkId::Versym);
    chunks.push(ChunkId::Verneed);
    chunks.push(ChunkId::NotePackage);

    if !ctx.args.oformat_binary {
        let mut shdr = ElfShdr::<E>::default();
        shdr.sh_type.set(SHT_PROGBITS);
        shdr.sh_flags.set((SHF_MERGE | SHF_STRINGS) as u64);
        let merged = RwLock::new(std::mem::take(&mut ctx.merged_sections));
        ctx.comment = crate::output_chunks::merged::MergedSection::get_instance(
            &ctx.args,
            &merged,
            BStr::new(b".comment"),
            &shdr,
        );
        ctx.merged_sections = merged.into_inner().unwrap();
    }

    if E::IS_X86 {
        ctx.note_property = Some(NotePropertySection::<E>::new());
        chunks.push(ChunkId::NoteProperty);
    }
    if E::IS_RISCV {
        ctx.riscv_attributes = Some(crate::output_chunks::misc::RiscvAttributesSection::new());
        chunks.push(ChunkId::RiscvAttributes);
    }
    if E::FAMILY == Family::Ppc64V2 {
        ctx.ppc64_save_restore = Some(crate::output_chunks::misc::Ppc64SaveRestoreSection::new());
        chunks.push(ChunkId::Ppc64SaveRestore);
    }
    if E::FAMILY == Family::Ppc64V1 {
        ctx.ppc64_opd = Some(crate::output_chunks::opd::Ppc64OpdSection::new());
        chunks.push(ChunkId::Ppc64Opd);
    }

    ctx.chunks.extend(chunks);
}

/// Marks the files that the given files depend on reachable, recursively.
/// Marks the files reachable from `roots` live. A file found live is
/// visited as a task of its own: there are at most thousands of files,
/// so the tasks are cheap, and the pool stays busy rather than draining
/// between rounds of a search whose frontier is often small.
fn mark_live_files<E: Arch>(ctx: &Context<E>, roots: Vec<FileId>) {
    fn visit<'s, E: Arch>(ctx: &'s Context<E>, id: FileId, scope: &rayon::Scope<'s>) {
        for found in mark_live_file(ctx, id) {
            scope.spawn(move |scope| visit(ctx, found, scope));
        }
    }
    rayon::scope(|scope| {
        for id in roots {
            scope.spawn(move |scope| visit(ctx, id, scope));
        }
    });
}

fn mark_live_file<E: Arch>(ctx: &Context<E>, id: FileId) -> Vec<FileId> {
    let mut found = Vec::new();
    match id {
        FileId::Obj(obj_id) => {
            let file = &ctx.objs[obj_id.index()];
            debug_assert!(file.base.is_reachable());
            for i in file.base.first_global..file.base.elf_syms.len() {
                let esym = &file.base.elf_syms[i];
                let sym = &ctx.symbols[file.base.symbols[i]];

                if !esym.is_undef() && file.exclude_libs {
                    sym.merge_visibility(STV_HIDDEN);
                } else {
                    sym.merge_visibility(esym.st_visibility());
                }
                if sym.is_traced() {
                    crate::input_files::print_trace_symbol(&ctx.diag, file, esym, sym);
                }

                if let Some(target) = sym.file() {
                    let undef_ref = esym.is_undef() && (!esym.is_weak() || target.is_dso());
                    let common_ref = esym.is_common() && !sym.is_common();
                    if (undef_ref || common_ref) && ctx.file(target).mark_reachable() {
                        found.push(target);
                        if sym.is_traced() {
                            out!(
                                ctx,
                                "trace-symbol: {file} keeps {} for {sym}",
                                ctx.file_display(target)
                            );
                        }
                    }
                }
            }
        }
        FileId::Dso(dso_id) => {
            let file = &ctx.dsos[dso_id.index()];
            for i in 0..file.base.elf_syms.len() {
                let esym = &file.base.elf_syms[i];
                let sym = &ctx.symbols[file.base.symbols[i]];
                if sym.is_traced() {
                    crate::input_files::print_trace_symbol(&ctx.diag, file, esym, sym);
                }
                // We follow undefined symbols in a DSO only to handle
                // --no-allow-shlib-undefined.
                if esym.is_undef() && !esym.is_weak() {
                    if let Some(target) = sym.file() {
                        if (!target.is_dso() || !ctx.args.allow_shlib_undefined)
                            && ctx.file(target).mark_reachable()
                        {
                            found.push(target);
                            if sym.is_traced() {
                                out!(
                                    ctx,
                                    "trace-symbol: {file} keeps {} for {sym}",
                                    ctx.file_display(target)
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    found
}

fn mark_live_objects<E: Arch>(ctx: &mut Context<E>) {
    // Symbols named on the command line pull in their files, and keep
    // their sections under --gc-sections; so does the entry point.
    let names: Vec<String> = ctx
        .args
        .undefined
        .iter()
        .chain(&ctx.args.require_defined)
        .cloned()
        .collect();
    for name in names {
        let id = ctx.get_symbol(name.as_bytes());
        ctx.symbols[id].set_gc_root(true);
        if let Some(file) = ctx.symbols[id].file() {
            ctx.file(file).set_reachable(true);
        }
    }
    let entry = ctx.syms.entry;
    ctx.symbols[entry].set_gc_root(true);

    if !ctx.args.undefined_glob.is_empty() {
        let roots: Vec<SymbolId> = {
            let Context {
                objs,
                symbols,
                args,
                ..
            } = &*ctx;
            objs.par_iter()
                .filter(|file| !file.base.is_reachable())
                .filter_map(|file| {
                    file.base.global_symbols().iter().copied().find(|&id| {
                        let sym = &symbols[id];
                        sym.file() == Some(FileId::Obj(file.id()))
                            && args.undefined_glob.find(sym.name()) != -1
                    })
                })
                .collect()
        };
        for id in roots {
            let file = ctx.symbols[id].file().unwrap();
            ctx.file(file).set_reachable(true);
            ctx.symbols[id].set_gc_root(true);
        }
    }

    let mut roots: Vec<FileId> = Vec::new();
    for file in &ctx.objs {
        if !file.base.as_needed {
            file.base.set_reachable(true);
        }
        if file.base.is_reachable() {
            roots.push(FileId::Obj(file.id()));
        }
    }
    for file in &ctx.dsos {
        if !file.base.as_needed {
            file.base.set_reachable(true);
        }
        if file.base.is_reachable() {
            roots.push(FileId::Dso(file.id()));
        }
    }
    mark_live_files(ctx, roots);
}

// Symbol resolution involving a default symbol version is tricky because
// a symbol that provides the default version has two names by which it
// can be referred. Specifically, a symbol `foo` with the default version
// `VER1` can be referred to either as `foo` or `foo@VER1`. No other
// symbols have two names like that.
//
// By default, we insert symbols with a default version without an at-sign
// (i.e. `foo` instead of `foo@VER1`) into our internal symbol table.
// Therefore, if the symbol is referenced with an at-sign (i.e.
// `foo@VER1`), the reference fails to resolve. This function corrects
// that error.
//
// In this function, we check all unresolved versioned symbols of the form
// `foo@VER1` by removing the version part and see if `foo` has version
// `VER1`. If it does, that's the symbol we are looking for.
fn resolve_default_symver<E: Arch>(ctx: &mut Context<E>) {
    let Context {
        objs,
        dsos,
        symbols,
        ..
    } = ctx;
    objs.par_iter_mut().for_each(|file| {
        let start = file.base.first_global.min(file.base.symbols.len());
        for id in &mut file.base.symbols[start..] {
            if symbols[*id].is_versioned_default() {
                *id = symbols[*id].symbol_origin().unwrap();
            }
        }
    });
    dsos.par_iter_mut().for_each(|file| {
        for id in &mut file.base.symbols {
            if symbols[*id].is_versioned_default() {
                *id = symbols[*id].symbol_origin().unwrap();
            }
        }
    });
}

fn clear_symbol<E: Arch>(sym: &mut Symbol) {
    sym.clear_file();
    sym.clear_origin();
    sym.value = 0;
    sym.sym_idx = u32::MAX;
    sym.set_esym(&ElfSym::<E>::default());
    sym.ver_idx = VER_NDX_UNSPECIFIED as u16;
    sym.set_weak(false);
    sym.set_imported(false);
    sym.set_exported(false);
    sym.set_versioned_default(false);
}

/// Resets the resolution of every global symbol. Local symbols belong to
/// their files and aren't resolved, so they're left alone.
fn clear_symbols<E: Arch>(ctx: &mut Context<E>) {
    ctx.symbols.par_for_each_global_mut(|sym| {
        if sym.file().is_some() {
            clear_symbol::<E>(sym);
        }
    });
}

// Creates a Symbol for each global symbol name recorded during file
// parsing and fills in the files' symbol pointers. Each hash shard of
// the symbol table is populated by a single thread, so no
// synchronization is needed, unlike interning symbols directly into a
// concurrent hash table as files are parsed.
pub fn gather_symbols<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("gather_symbols");
    let bins = ctx.take_symbol_bins();
    let Context { symbols, .. } = ctx;

    // Each hash shard is populated by one thread and writes directly to the
    // stable slots recorded while files were parsed, so no synchronization
    // or final scatter pass is needed.
    symbols.gather_symbol_slots(bins);
}

fn current_rank<E: Arch>(ctx: &Context<E>, sym: &Symbol) -> u64 {
    match sym.file() {
        None => 7 << 32,
        Some(file) => {
            let base = ctx.file(file);
            resolved_symbol_rank(sym, file.is_dso(), !base.is_reachable(), base.priority)
        }
    }
}

/// Resolves every global symbol in place. Files run in parallel, taking
/// each symbol's lock before comparing and updating its definition.
fn resolve_symbols_pass<E: Arch>(ctx: &mut Context<E>, files: &[FileId], only_reachable: bool) {
    let _t = ctx.timer("resolve_symbols_pass");
    let Context {
        objs,
        dsos,
        symbols,
        default_version,
        ..
    } = ctx;
    let resolver = SymbolResolver::new(symbols.as_mut_slice(), objs, dsos, *default_version);
    files.par_iter().for_each(|&id| match id {
        FileId::Obj(id) => {
            let file = &objs[id.index()];
            if !only_reachable || file.base.is_reachable() {
                file.resolve_symbols(&resolver, id);
            }
        }
        FileId::Dso(id) => {
            let file = &dsos[id.index()];
            if !only_reachable || file.base.is_reachable() {
                file.resolve_symbols(&resolver, id);
            }
        }
    });
}

/// Resolves the rare hidden-symbol retry while ignoring DSO definitions.
fn resolve_skip_dso_symbols_pass<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("resolve_symbols_pass");
    let Context {
        objs,
        dsos,
        symbols,
        default_version,
        ..
    } = ctx;
    let resolver = SymbolResolver::new(symbols.as_mut_slice(), objs, dsos, *default_version);
    objs.par_iter().for_each(|file| {
        file.resolve_skip_dso_symbols(&resolver, file.id());
    });
}

/// An exceptional COMDAT signature whose Symbol slot is filled by gathering.
/// The owning ObjectFile and its group vector stay in place through the
/// gather, just as they do for the files' ordinary SymbolSlots.
struct PendingComdatOwner {
    group: NonNull<ComdatGroupRef>,
    priority: u32,
    is_lto_output: bool,
}

// SAFETY: every pending record names a distinct group, and it is consumed only
// after gathering has stopped writing the group's signature slot.
unsafe impl Send for PendingComdatOwner {}

/// A packed COMDAT signature word that receives the interned SymbolId before
/// ownership is selected.
#[derive(Clone, Copy)]
struct ComdatSymbolSlot(NonNull<u32>);

// SAFETY: each exceptional COMDAT signature has one pending slot, and one
// symbol-map shard writes it before the groups are examined again.
unsafe impl Send for ComdatSymbolSlot {}
unsafe impl Sync for ComdatSymbolSlot {}

impl ComdatSymbolSlot {
    fn new(group: &mut ComdatGroupRef) -> ComdatSymbolSlot {
        ComdatSymbolSlot(NonNull::from(group.signature_word_mut()))
    }

    fn assign(self, id: SymbolId) {
        assert!(id.0 < 1 << 31);
        // SAFETY: guaranteed by the construction and synchronization rules
        // documented on ComdatSymbolSlot.
        unsafe { self.0.write(id.0) };
    }
}

/// COMDAT signature keys and owners collected by each Rayon worker.
struct ComdatWorkBins(Vec<UnsafeCell<(Bins<ComdatSymbolSlot>, Vec<PendingComdatOwner>)>>);

// SAFETY: a Rayon worker has a unique index and executes at most one closure
// at a time. The only caller does not invoke nested parallel work while using
// its bin, and the bins are consumed after the traversal has joined.
unsafe impl Sync for ComdatWorkBins {}

impl ComdatWorkBins {
    fn new() -> ComdatWorkBins {
        ComdatWorkBins(
            (0..=rayon::current_num_threads())
                .map(|_| UnsafeCell::new((Bins::new(), Vec::new())))
                .collect(),
        )
    }

    #[inline]
    fn with_local(
        &self,
        f: impl FnOnce(&mut Bins<ComdatSymbolSlot>, &mut Vec<PendingComdatOwner>),
    ) {
        let fallback = self.0.len() - 1;
        let i = rayon::current_thread_index()
            .unwrap_or(fallback)
            .min(fallback);
        // SAFETY: the Sync invariant above gives this worker exclusive access
        // to its indexed buffers for the duration of this non-nested closure.
        let (bins, pending) = unsafe { &mut *self.0[i].get() };
        f(bins, pending);
    }

    fn into_parts(self) -> (Vec<Bins<ComdatSymbolSlot>>, Vec<Vec<PendingComdatOwner>>) {
        self.0.into_iter().map(UnsafeCell::into_inner).unzip()
    }
}

// Select COMDAT groups and construct input sections. If LTO will run,
// the first invocation also constructs the losing copies of COMDAT
// members because this function runs again after LTO and may then
// select a different winner.
fn parse_input_sections<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("parse_input_sections");

    // Symbol resolution is clear while COMDAT groups are selected, so sym_idx
    // can temporarily hold the winning file priority.
    let record_owner = |sym: &Symbol, priority: u32| {
        // SAFETY: sym_idx is aligned for u32. During this parallel phase it is
        // accessed only through this atomic reference, and the phase is joined
        // before ordinary accesses resume.
        let owner = unsafe { AtomicU32::from_ptr(std::ptr::addr_of!(sym.sym_idx).cast_mut()) };
        let mut old = owner.load(Ordering::Relaxed);
        while priority < old {
            match owner.compare_exchange_weak(old, priority, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(actual) => old = actual,
            }
        }
    };

    // Read COMDAT metadata and choose an owner among reachable regular objects.
    // Ordinary global signatures already refer to the files' symbols; record
    // the other signatures for interning while each file's metadata is hot.
    let t = ctx.timer("read_section_metadata");
    let (bins, pending): (Vec<Bins<ComdatSymbolSlot>>, Vec<Vec<PendingComdatOwner>>) = {
        let Context {
            objs,
            symbols,
            diag,
            ..
        } = ctx;
        let work = ComdatWorkBins::new();
        objs.par_iter_mut().for_each(|file| {
            work.with_local(|bins, pending| {
                if file.base.is_reachable() {
                    if file.base.mf.is_some() && !file.is_lto_input && !file.sections_parsed {
                        file.read_section_metadata(diag);
                    }
                    let priority = file.base.priority;
                    let is_lto_output = file.is_lto_output;
                    for group in &mut file.comdat_groups {
                        if group.signature() != SymbolId::DISCARDED_COMDAT {
                            let sym = &symbols[group.signature()];
                            // A group claimed by an IR file belongs to the LTO result, so only an
                            // LTO-generated file may own it. LLVM keeps claimed groups in its output;
                            // GCC emits their contents without a group, so no file owns them.
                            if !sym.comdat_claimed_by_ir() || is_lto_output {
                                record_owner(sym, priority);
                            }
                        }
                    }
                    let signatures = &file.pending_comdat_signatures;
                    let groups = &mut file.comdat_groups;
                    for signature in signatures {
                        let group = &mut groups[signature.group_idx as usize];
                        bins.record(
                            signature.key,
                            signature.name_len as usize,
                            ComdatSymbolSlot::new(group),
                        );
                        pending.push(PendingComdatOwner {
                            group: NonNull::from(group),
                            priority,
                            is_lto_output,
                        });
                    }
                }
            });
        });
        work.into_parts()
    };

    drop(t);

    // Intern the signature symbols all at once, as for the files' own symbols.
    let t = ctx.timer("comdat_signatures");
    {
        let Context { objs, symbols, .. } = ctx;
        symbols.gather(bins, ComdatSymbolSlot::assign);

        // Signatures just interned could not participate in the metadata
        // traversal above. Record them now.
        let symbols: &crate::symbol::SymbolTable = symbols;
        pending.into_par_iter().flatten().for_each(|pending| {
            // SAFETY: each pending pointer came from a distinct group that
            // stays in place, and symbol gathering has joined before this
            // parallel traversal begins.
            let group = unsafe { &*pending.group.as_ptr() };
            let sym = &symbols[group.signature()];
            if !sym.comdat_claimed_by_ir() || pending.is_lto_output {
                record_owner(sym, pending.priority);
            }
        });
        objs.par_iter_mut()
            .for_each(|file| file.pending_comdat_signatures = Vec::new());
    }

    let obj_ids: Vec<ObjId> = ctx.objs.iter().map(ObjectFile::id).collect();

    // IR objects name their COMDAT groups per symbol.
    for obj_id in obj_ids.iter().copied() {
        let fi = obj_id.index();
        for i in 0..ctx.objs[fi].lto_comdat_keys.len() {
            if let (Some(key), None) = (
                ctx.objs[fi].lto_comdat_keys[i],
                ctx.objs[fi].lto_comdat_signatures[i],
            ) {
                let sig = ctx.symbols.intern(key);
                ctx.objs[fi].lto_comdat_signatures[i] = Some(sig);
            }
        }
    }

    drop(t);

    // Choose an owner for each group: the reachable object with the
    // lowest priority. A group claimed by an IR file belongs to the LTO
    // result, so only an LTO-generated object may own it.
    let t = ctx.timer("comdat_owners");

    // Set each group's result after all reachable files have recorded their
    // claims.
    let symbols = &ctx.symbols;
    ctx.objs.par_iter_mut().for_each(|file| {
        if file.base.is_reachable() {
            let priority = file.base.priority;
            for group in &mut file.comdat_groups {
                group.set_owner(symbols[group.signature()].sym_idx == priority);
            }
        }
    });

    // LTO plugin symbol tables may not enumerate all section-level helper
    // symbols (e.g. some thunks). Therefore, an IR file may claim a signature
    // only if no reachable regular object has already claimed it.
    //
    // An IR file's claim is permanent: the LTO result provides the claimed
    // definitions, so a regular object extracted after LTO must not win the
    // group and resurrect a copy of them
    // (https://github.com/rui314/mold/issues/1637).
    for obj_id in obj_ids {
        let fi = obj_id.index();
        let file = &ctx.objs[fi];
        if !file.base.is_reachable() || file.lto_comdat_signatures.is_empty() {
            continue;
        }
        let priority = file.base.priority;
        let mut discarded = Vec::with_capacity(file.lto_comdat_signatures.len());
        for &sig in &file.lto_comdat_signatures {
            let Some(sig) = sig else {
                discarded.push(false);
                continue;
            };
            if ctx.symbols[sig].sym_idx == u32::MAX {
                ctx.symbols[sig].sym_idx = priority;
                ctx.symbols[sig].set_comdat_claimed_by_ir(true);
            }
            discarded.push(ctx.symbols[sig].sym_idx != priority);
        }
        ctx.objs[fi].lto_comdat_discarded = discarded;
    }

    // Restore sym_idx before the final symbol-resolution pass.
    ctx.symbols
        .par_for_each_global_mut(|sym| sym.sym_idx = u32::MAX);

    drop(t);

    // LTO can change archive extraction and therefore the winning COMDAT
    // group. Construct the losing copies too so one can become the winner
    // after LTO.
    let keep_discarded_comdat = ctx
        .objs
        .iter()
        .any(|f| f.base.is_reachable() && (f.is_lto_input || f.is_gcc_offload_obj));

    let t = ctx.timer("parse_sections");
    let maximum: usize = ctx
        .objs
        .iter()
        .filter(|file| {
            file.base.is_reachable()
                && file.base.mf.is_some()
                && !file.is_lto_input
                && !file.sections_parsed
        })
        .map(|file| {
            if file.base.elf_syms.is_empty() {
                0
            } else {
                file.base.first_global.max(1)
            }
        })
        .sum();
    {
        let Context {
            objs,
            symbols,
            section_arena,
            diag,
            args,
            ..
        } = ctx;
        symbols.with_parallel_appender(maximum, |allocator| {
            objs.par_iter_mut().for_each(|file| {
                if file.base.is_reachable()
                    && file.base.mf.is_some()
                    && !file.is_lto_input
                    && !file.sections_parsed
                {
                    file.parse_sections(
                        diag,
                        args,
                        file.id(),
                        section_arena,
                        allocator,
                        keep_discarded_comdat,
                    );
                }
            });
        });
    }
    drop(t);

    // Apply the selection to all group members. This also updates sections
    // parsed before LTO if ownership has changed.
    let _t = ctx.timer("comdat_members");
    ctx.objs.par_iter().for_each(|file| {
        if !file.base.is_reachable() {
            return;
        }
        for group in &file.comdat_groups {
            for member in file.comdat_members(group) {
                if let Some(isec) = file.section(member as usize) {
                    if group.is_owner() {
                        isec.revive();
                    } else {
                        file.kill_section(member as usize);
                    }
                }
            }
        }
    });
}

pub fn resolve_symbols<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("resolve_symbols");
    gather_symbols(ctx);

    let files: Vec<FileId> = ctx
        .objs
        .iter()
        .map(|file| FileId::Obj(file.id()))
        .chain(ctx.dsos.iter().map(|file| FileId::Dso(file.id())))
        .collect();

    // Call resolve_symbols() to find the most appropriate file for each
    // symbol. And then mark reachable objects to decide which files to
    // include into an output.
    resolve_symbols_pass(ctx, &files, false);
    let t = ctx.timer("resolve_default_symver");
    resolve_default_symver(ctx);
    drop(t);
    let t = ctx.timer("mark_live_objects");
    mark_live_objects(ctx);
    drop(t);

    // Symbols with hidden visibility need to be resolved within the
    // output file. If a hidden symbol was resolved to a DSO, we resolve
    // that symbol again, skipping DSOs. Its new definition may be in an
    // archive member that is not reachable yet; making it reachable can
    // change other symbols' visibility, so we repeat until no hidden
    // symbol resolves to a DSO. This should be rare.
    loop {
        let hidden = ctx.symbols.par_find_globals(|sym| {
            matches!(sym.file(), Some(FileId::Dso(dso))
                if ctx.dsos[dso.index()].base.is_reachable()
                    && sym.visibility() == STV_HIDDEN)
        });
        if hidden.is_empty() {
            break;
        }
        for &id in &hidden {
            let sym = &mut ctx.symbols[id];
            sym.set_skip_dso(true);
            clear_symbol::<E>(sym);
        }
        resolve_skip_dso_symbols_pass(ctx);

        let mut roots = Vec::new();
        for &id in &hidden {
            if let Some(file) = ctx.symbols[id].file() {
                if !ctx.file(file).is_reachable() {
                    ctx.file(file).set_reachable(true);
                    roots.push(file);
                }
            }
        }
        mark_live_files(ctx, roots);
    }

    // Now that we know the exact set of input files that are to be
    // included in the output file, we want to redo symbol resolution.
    // This is because symbols defined by object files in archive files
    // may have risen as a result of mark_live_objects().
    //
    // To redo symbol resolution, we want to clear the state first.
    let t = ctx.timer("clear_symbols");
    clear_symbols(ctx);
    drop(t);

    // Parse input sections after archive extraction, so COMDAT selection
    // considers only reachable objects. This must happen before final symbol
    // resolution; otherwise discarding a group could leave a dangling reference.
    parse_input_sections(ctx);

    // Redo symbol resolution
    resolve_symbols_pass(ctx, &files, true);
}

/// Drops files that didn't make it into the link and renumbers the rest.
pub fn remove_unreachable_files<E: Arch>(ctx: &mut Context<E>) {
    remove_objects(ctx, |file| !file.base.is_reachable());
    remove_unreachable_dsos(ctx);
}

/// Drops the object files `remove` selects and renumbers the rest.
fn remove_objects<E: Arch>(ctx: &mut Context<E>, remove: impl Fn(&ObjectFile<E>) -> bool) {
    ctx.objs.retain(|file| !remove(file));
}

/// Whether the link involves the LTO plugin.
pub fn has_lto_obj<E: Arch>(ctx: &Context<E>) -> bool {
    ctx.objs
        .iter()
        .any(|file| file.base.is_reachable() && (file.is_lto_input || file.is_gcc_offload_obj))
}

// Do link-time optimization. We pass all IR object files to the compiler
// backend to compile them into a few ELF object files.
pub fn do_lto<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("do_lto");

    // The compiler backend needs to know how symbols are resolved, so
    // compute symbol visibility, import/export bits, etc early.
    apply_version_script(ctx);
    parse_symbol_version(ctx);
    compute_import_export(ctx);

    // If multiple IR object files define the same symbol, the LTO backend
    // would choose one of them randomly instead of reporting an error.
    // So we need to check for symbol duplication error before doing an LTO.
    if !ctx.args.allow_multiple_definition {
        check_duplicate_symbols(ctx);
    }

    // Invoke the LTO plugin. This step compiles IR object files into a few
    // big ELF files.
    crate::lto::run_plugin(ctx);

    // Redo name resolution.
    clear_symbols(ctx);

    // Remove IR object files and reset reachability for archive members.
    // Archive members that were extracted pre-LTO to satisfy references from
    // IR objects may no longer be needed now that LTO output provides those
    // symbols. Reset their reachability so that resolve_symbols() below can
    // re-derive which archive members are actually needed.
    for file in &ctx.objs {
        if file.is_lto_input || file.base.as_needed {
            file.base.set_reachable(false);
        }
    }
    let ir_files = ctx
        .objs
        .iter()
        .filter(|file| file.is_lto_input)
        .filter_map(|file| Some((file.base.priority, file.base.mf?)));
    ctx.lto_input_files.extend(ir_files);
    remove_objects(ctx, |file| file.is_lto_input);
    resolve_symbols(ctx);
}

pub fn parse_eh_frame_sections<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("parse_eh_frame_sections");
    let Context { objs, diag, .. } = ctx;
    objs.par_iter_mut()
        .for_each(|file| file.parse_ehframe(diag));
}

pub fn parse_sframe_sections<E: Arch>(ctx: &mut Context<E>) {
    if !E::SUPPORTS_SFRAME {
        return;
    }
    let _t = ctx.timer("parse_sframe_sections");
    let Context { objs, diag, .. } = ctx;
    objs.par_iter_mut().for_each(|file| file.parse_sframe(diag));
}

/// Registers direct, stable member borrows with their merged sections for a
/// parallel resolution phase.
fn merged_resolve_members<E: Arch>(
    objs: &mut FileList<ObjectFile<E>>,
    count: usize,
) -> Vec<Vec<crate::output_chunks::merged::ResolveMember<'_>>> {
    let mut members: Vec<Vec<crate::output_chunks::merged::ResolveMember<'_>>> =
        (0..count).map(|_| Vec::new()).collect();
    for file in objs {
        let filename = file.base.filename.as_str();
        let archive_name = file.archive_name.as_str();
        let shstrtab = file.base.shstrtab;
        let num_elf_sections = file.num_elf_sections;
        for (mergeable, input) in file.sections.mergeable_sections_with_inputs_mut() {
            let parent = mergeable.parent.index();
            members[parent].push(crate::output_chunks::merged::ResolveMember {
                mergeable,
                section: input,
                filename,
                archive_name,
                name: input.name_in(shstrtab, num_elf_sections),
            });
        }
    }
    members
}

pub fn create_merged_sections<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("create_merged_sections");

    // Convert InputSections to MergeableSections.
    let t = ctx.timer("convert_mergeable_sections");
    {
        let Context {
            objs,
            merged_sections,
            args,
            diag,
            ..
        } = ctx;
        let merged = RwLock::new(std::mem::take(merged_sections));
        objs.par_iter_mut()
            .for_each(|file| file.convert_mergeable_sections(args, &merged, diag));
        *merged_sections = merged.into_inner().unwrap();
    }
    drop(t);

    // Register each mergeable section with its merged section. There are
    // only a few merged sections, so doing this from the parallel loop
    // above under a lock would serialize all threads on it.
    let t = ctx.timer("register_members");
    for file in &ctx.objs {
        for m in file.mergeable_sections() {
            ctx.merged_sections[m.parent.index()]
                .members
                .push(SectionRef {
                    file: file.id(),
                    shndx: m.shndx,
                });
        }
    }
    drop(t);

    let t = ctx.timer("resolve");
    let mut members = merged_resolve_members(&mut ctx.objs, ctx.merged_sections.len());
    crate::output_chunks::merged::resolve_sections::<E>(
        &mut ctx.merged_sections,
        &mut members,
        crate::output_chunks::merged::ResolveOptions {
            allocated_only: true,
            gc_sections: ctx.args.gc_sections,
            diag: &ctx.diag,
            comment: ctx.comment,
            cmdline_args: &ctx.cmdline_args,
            timers: &ctx.timers,
        },
    );
    drop(t);

    let _t = ctx.timer("reattach_section_pieces");
    let Context {
        objs,
        symbols,
        merged_sections,
        diag,
        ..
    } = ctx;

    // Arena allocations cannot be reclaimed, so grow the symbol table only
    // once. num_frag_syms, counted when the sections were parsed, may include
    // references to mergeable sections that were not converted; the extra
    // symbols stay unused.
    let counts: Vec<usize> = objs
        .iter()
        .map(|file| file.num_fragment_dummies())
        .collect();
    let base = symbols.len();
    // SAFETY: the new slots are split among the files exactly, and each file
    // initializes every slot in its slice. The editor covers only the
    // disjoint, initialized prefix.
    unsafe {
        symbols.add_many_with_existing(counts.iter().sum(), |existing, slots| {
            let editor = SymbolEditor::new(existing);
            let mut rest = slots;
            let mut slices = Vec::with_capacity(counts.len());
            let mut offset = base;
            for &count in &counts {
                let (head, tail) = rest.split_at_mut(count);
                slices.push((SymbolId(offset as u32), head));
                rest = tail;
                offset += count;
            }
            objs.par_iter_mut()
                .zip(slices)
                .for_each(|(file, (base_id, slots))| {
                    let id = file.id();
                    file.reattach_section_symbols(diag, id, &editor, merged_sections);
                    file.reattach_fragment_relocations(diag, id, merged_sections, base_id, slots);
                });
        });
    }
}

pub fn convert_common_symbols<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("convert_common_symbols");
    let default_version = ctx.default_version;
    let Context {
        objs,
        symbols,
        diag,
        args,
        section_arena,
        ..
    } = ctx;
    for file in objs {
        file.convert_common_symbols(
            diag,
            args,
            file.id(),
            symbols,
            default_version,
            section_arena,
        );
    }
}

fn has_ctors_and_init_array<E: Arch>(ctx: &Context<E>) -> bool {
    ctx.objs.iter().any(|f| f.has_ctors) && ctx.objs.iter().any(|f| f.has_init_array)
}

fn canonicalize_type<E: Arch>(name: &[u8], ty: u32) -> u32 {
    // Some old assemblers don't recognize these section names and create
    // them as SHT_PROGBITS.
    if ty == SHT_PROGBITS {
        if name == b".init_array" || name.starts_with(b".init_array.") {
            return SHT_INIT_ARRAY;
        }
        if name == b".fini_array" || name.starts_with(b".fini_array.") {
            return SHT_FINI_ARRAY;
        }
    }
    // The x86-64 psABI defines SHT_X86_64_UNWIND for .eh_frame, allowing
    // the linker to recognize the section not by name but by section type.
    // However, that spec change was generally considered a mistake; it has
    // just complicated the situation. As a result, .eh_frame on x86-64 may
    // be either SHT_PROGBITS or SHT_X86_64_UNWIND. We use SHT_PROGBITS
    // consistently.
    if E::FAMILY == Family::X86_64 && ty == SHT_X86_64_UNWIND {
        return SHT_PROGBITS;
    }
    ty
}

fn output_name<E: Arch>(
    args: &crate::cmdline::Args,
    name: &'static BStr,
    flags: u64,
) -> &'static BStr {
    if args.relocatable && !args.relocatable_merge_sections {
        return name;
    }
    if !args.unique.is_empty() && args.unique.find(name) != -1 {
        return name;
    }
    if flags & SHF_MERGE as u64 != 0 {
        return name;
    }
    if E::FAMILY == Family::Arm32 {
        if name.starts_with(b".ARM.exidx") {
            return BStr::new(b".ARM.exidx");
        }
        if name.starts_with(b".ARM.extab") {
            return BStr::new(b".ARM.extab");
        }
    }

    if args.z_keep_text_section_prefix {
        for prefix in [
            ".text.hot.",
            ".text.unknown.",
            ".text.unlikely.",
            ".text.startup.",
            ".text.exit.",
        ] {
            let stem = &prefix[..prefix.len() - 1];
            if name == stem.as_bytes() || name.starts_with(prefix.as_bytes()) {
                return BStr::new(stem.as_bytes());
            }
        }
    }

    const PREFIXES: &[&str] = &[
        ".text.",
        ".data.rel.ro.",
        ".data.",
        ".rodata.",
        ".bss.rel.ro.",
        ".bss.",
        ".init_array.",
        ".fini_array.",
        ".tbss.",
        ".tdata.",
        ".gcc_except_table.",
        ".ctors.",
        ".dtors.",
        ".gnu.warning.",
        ".openbsd.randomdata.",
        ".sdata.",
        ".sbss.",
        ".srodata.",
        ".gnu.build.attributes.",
    ];
    for prefix in PREFIXES {
        let stem = &prefix[..prefix.len() - 1];
        if name == stem.as_bytes() || name.starts_with(prefix.as_bytes()) {
            return BStr::new(stem.as_bytes());
        }
    }
    name
}

fn output_section_key<E: Arch>(
    args: &crate::cmdline::Args,
    isec: &crate::input_sections::InputSection,
    name: &'static BStr,
    sh_type: u32,
    ctors_in_init_array: bool,
) -> (&'static BStr, u32) {
    // If .init_array/.fini_array exist, .ctors/.dtors must be merged
    // with them.
    //
    // CRT object files contain .ctors/.dtors sections without any
    // relocations. They contain sentinel values, 0 and -1, to mark the
    // beginning and the end of the initializer/finalizer pointer arrays.
    // We do not place them into .init_array/.fini_array because such
    // invalid pointer values would simply make the program to crash.
    if ctors_in_init_array && isec.has_relocations() {
        if name == b".ctors" || name.starts_with(b".ctors.") {
            return (BStr::new(b".init_array"), SHT_INIT_ARRAY);
        }
        if name == b".dtors" || name.starts_with(b".dtors.") {
            return (BStr::new(b".fini_array"), SHT_FINI_ARRAY);
        }
    }
    let name = output_name::<E>(args, name, isec.sh_flags);
    (name, canonicalize_type::<E>(name, sh_type))
}

type OutputSectionKey = (&'static BStr, u32);

/// One file's contribution to an output section while members are gathered.
struct OutputSectionFileMembers {
    members: Vec<InputSectionId>,
    sh_flags: u64,
    p2align: u8,
}

// Scratch buffer used by create_output_sections() to build `members`.
// Grouping input sections by file allows appending them in parallel
// without synchronization while keeping their order deterministic.
struct OutputSectionBuilder {
    section: OutputSectionId,
    files: Box<[UnsafeCell<OutputSectionFileMembers>]>,
}

type OutputSectionShared<E> = (
    HashMap<OutputSectionKey, Arc<OutputSectionBuilder>>,
    Vec<OutputSection<E>>,
);

// SAFETY: the file-parallel traversal gives each task a distinct slot. The
// slots are read only after that traversal joins.
unsafe impl Sync for OutputSectionBuilder {}

impl OutputSectionBuilder {
    fn new(section: OutputSectionId, num_files: usize) -> OutputSectionBuilder {
        OutputSectionBuilder {
            section,
            files: (0..num_files)
                .map(|_| {
                    UnsafeCell::new(OutputSectionFileMembers {
                        members: Vec::new(),
                        sh_flags: 0,
                        p2align: 0,
                    })
                })
                .collect(),
        }
    }

    /// Returns the slot owned by the task processing `file`.
    ///
    /// # Safety
    ///
    /// Only one task may call this for a given file index, and all calls must
    /// finish before the slots are read.
    #[inline]
    unsafe fn with_file_mut<R>(
        &self,
        file: usize,
        f: impl FnOnce(&mut OutputSectionFileMembers) -> R,
    ) -> R {
        unsafe { f(&mut *self.files[file].get()) }
    }
}

/// The output-section map cached by each Rayon worker. This is the equivalent
/// of C++ mold's `enumerable_thread_specific<MapType>`.
struct OutputSectionCaches(Vec<UnsafeCell<HashMap<OutputSectionKey, Arc<OutputSectionBuilder>>>>);

// SAFETY: a Rayon worker has a unique index and executes at most one closure
// at a time. The only caller does not invoke nested parallel work while using
// its cache, and the caches are dropped after the parallel traversal joins.
unsafe impl Sync for OutputSectionCaches {}

impl OutputSectionCaches {
    fn new() -> OutputSectionCaches {
        OutputSectionCaches(
            (0..=rayon::current_num_threads())
                .map(|_| UnsafeCell::new(HashMap::new()))
                .collect(),
        )
    }

    #[inline]
    fn with_local<R>(
        &self,
        f: impl FnOnce(&mut HashMap<OutputSectionKey, Arc<OutputSectionBuilder>>) -> R,
    ) -> R {
        let fallback = self.0.len() - 1;
        let i = rayon::current_thread_index()
            .unwrap_or(fallback)
            .min(fallback);
        // SAFETY: the Sync invariant above gives this worker exclusive access
        // to its indexed map for the duration of this non-nested closure.
        f(unsafe { &mut *self.0[i].get() })
    }
}

// PT_GNU_RELRO segment is a security mechanism to make more pages
// read-only than we could have done without it.
//
// Traditionally, sections are either read-only or read-write. If a
// section contains dynamic relocations, it must have been put into a
// read-write segment so that the program loader can mutate its
// contents in memory, even if no one will write to it at runtime.
//
// RELRO segment allows us to make such pages writable only when a
// program is being loaded. After that, the page becomes read-only.
//
// Some sections, such as .init, .fini, .got, .dynamic, contain
// dynamic relocations but doesn't have to be writable at runtime,
// so they are put into a RELRO segment.
fn is_relro<E: Layout>(osec: &OutputSection<E>) -> bool {
    let name = osec.hdr.name;
    let ty = osec.hdr.shdr.sh_type.get();
    let flags = osec.hdr.shdr.sh_flags.get();
    name == b".toc"
        || name.ends_with(b".rel.ro")
        || name.ends_with(b".rel.ro.hot")
        || name.ends_with(b".rel.ro.unlikely")
        || matches!(ty, SHT_INIT_ARRAY | SHT_FINI_ARRAY | SHT_PREINIT_ARRAY)
        || flags & SHF_TLS as u64 != 0
}

// Create output sections for input sections.
//
// Since one output section could contain millions of input sections,
// we need to do it efficiently.
pub fn create_output_sections<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("create_output_sections");
    let ctors_in_init_array = has_ctors_and_init_array(ctx);
    let first_new = ctx.output_sections.len();

    // Make a per-thread cache of the main map to avoid lock contention.
    // It makes a noticeable difference if we have millions of input sections.
    let num_files = ctx.objs.len();
    let shared: Mutex<OutputSectionShared<E>> =
        Mutex::new((HashMap::new(), std::mem::take(&mut ctx.output_sections)));
    let caches = OutputSectionCaches::new();

    // Instantiate output sections and assign input sections to them
    {
        let Context { objs, args, .. } = ctx;
        objs.par_iter_mut().enumerate().for_each(|(fi, file)| {
            caches.with_local(|cache| {
                let shstrtab = file.base.shstrtab;
                let num_elf_sections = file.num_elf_sections;
                let shdrs = &file.base.shdrs;
                let extra_shdrs = &file.elf_sections2;
                for (member, isec) in file
                    .sections
                    .regular_ids_mut()
                    .filter(|(_, isec)| isec.is_alive())
                {
                    let name = isec.name_in(shstrtab, num_elf_sections);
                    let sh_type = if isec.is_nobits() {
                        SHT_NOBITS
                    } else if isec.shndx as usize >= num_elf_sections {
                        extra_shdrs[isec.shndx as usize - num_elf_sections]
                            .sh_type
                            .get()
                    } else {
                        shdrs[isec.shndx as usize].sh_type.get()
                    };
                    let sh_flags = isec.sh_flags
                        & !(SHF_MERGE | SHF_STRINGS | SHF_COMPRESSED | SHF_GNU_RETAIN) as u64;

                    if args.relocatable && sh_flags & SHF_GROUP as u64 != 0 {
                        // COMDAT group members keep their own output sections
                        // in a relocatable output.
                        let mut osec = OutputSection::<E>::new(name, sh_type);
                        osec.hdr.shdr.sh_flags.set(sh_flags);
                        osec.hdr.shdr.sh_addralign.set(1 << isec.p2align());
                        osec.hdr.is_relro = is_relro(&osec);
                        osec.members.push(member);
                        let mut shared = shared.lock().unwrap();
                        shared.1.push(osec);
                        isec.output_section = Some(OutputSectionId::new(shared.1.len() as u32 - 1));
                        continue;
                    }

                    let key =
                        output_section_key::<E>(args, isec, name, sh_type, ctors_in_init_array);
                    let builder = match cache.entry(key) {
                        std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            let mut shared = shared.lock().unwrap();
                            let (map, sections) = &mut *shared;
                            let builder = map
                                .entry(key)
                                .or_insert_with(|| {
                                    sections.push(OutputSection::<E>::new(key.0, key.1));
                                    let id = OutputSectionId::new(sections.len() as u32 - 1);
                                    Arc::new(OutputSectionBuilder::new(id, num_files))
                                })
                                .clone();
                            entry.insert(builder)
                        }
                    };
                    isec.output_section = Some(builder.section);

                    // SAFETY: this closure is the only task for file fi;
                    // flattening starts after the parallel traversal joins.
                    unsafe {
                        builder.with_file_mut(fi, |file| {
                            file.members.push(member);
                            file.sh_flags |= sh_flags & !(SHF_GROUP as u64);
                            file.p2align = file.p2align.max(isec.p2align());
                        });
                    }
                }
            })
        });
    }
    let (map, sections) = shared.into_inner().unwrap();
    ctx.output_sections = sections;

    // Flatten members_vec into an arena-allocated members array and
    // compute the section alignment. Both are done in parallel over the
    // files; an output section such as .text has a million members.
    let builders: Vec<Arc<OutputSectionBuilder>> = map.into_values().collect();
    drop(caches);
    let flattened: Vec<(OutputSectionId, Vec<InputSectionId>, u64, u8)> = builders
        .par_iter()
        .map(|builder| {
            let parts: Vec<&OutputSectionFileMembers> = builder
                .files
                .iter()
                // SAFETY: the file traversal has joined, so the slots are no
                // longer being mutated.
                .map(|file| unsafe { &*file.get() })
                .collect();

            let n = parts.iter().map(|g| g.members.len()).sum();
            let mut members = vec![InputSectionId::NONE; n];
            let mut rest = members.as_mut_slice();
            let mut slices = Vec::with_capacity(parts.len());
            for g in &parts {
                let (head, tail) = rest.split_at_mut(g.members.len());
                slices.push(head);
                rest = tail;
            }
            parts.par_iter().zip(slices).for_each(|(g, slice)| {
                slice.copy_from_slice(&g.members);
            });

            let sh_flags = parts.iter().fold(0, |flags, g| flags | g.sh_flags);
            let p2align = parts.iter().map(|g| g.p2align).max().unwrap_or(0);
            (builder.section, members, sh_flags, p2align)
        })
        .collect();
    for (id, members, sh_flags, p2align) in flattened {
        let osec = &mut ctx.output_sections[id.index()];
        osec.members = members;
        osec.hdr.shdr.sh_flags.set(sh_flags);
        osec.hdr.shdr.sh_addralign.set(1 << p2align);
        osec.hdr.is_relro = is_relro(osec);
    }

    // Add output sections and mergeable sections to ctx.chunks
    let mut chunks: Vec<ChunkId> = (first_new..ctx.output_sections.len())
        .map(|i| ChunkId::Output(OutputSectionId::new(i as u32)))
        .chain(
            (0..ctx.merged_sections.len())
                .map(|i| ChunkId::Merged(crate::output_chunks::merged::MergedSectionId(i as u32))),
        )
        .collect();
    // Sections are added to the section lists in an arbitrary order
    // because they are created in parallel. Sort them to to make the
    // output deterministic.
    chunks.sort_by_cached_key(|&id| {
        let hdr = ctx.chunk_header(id);
        (
            hdr.name.to_vec(),
            hdr.shdr.sh_type.get(),
            hdr.shdr.sh_flags.get(),
        )
    });
    ctx.chunks.extend(chunks);
}

// Create a dummy object file containing linker-synthesized
// symbols.
pub fn create_internal_file<E: Arch>(ctx: &mut Context<E>) {
    let mut obj = ObjectFile::internal();
    obj.base.priority = 0;

    // Create linker-synthesized symbols.
    ctx.internal_esyms = vec![ElfSym::<E>::default()];
    let dummy = ctx.symbols.add(Symbol::new(BStr::new(b"")));
    obj.base.symbols.push(dummy);
    obj.base.first_global = 1;

    let add = |ctx: &mut Context<E>, obj: &mut ObjectFile<E>, name: &str| {
        let id = ctx.get_symbol(name.as_bytes());
        obj.base.symbols.push(id);
        // An actual value will be set to a linker-synthesized symbol by
        // fix_synthetic_symbols(). Until then, `value` doesn't have a valid
        // value. 0xdeadbeef is a unique dummy value to make debugging easier
        // if the field is accidentally used before it gets a valid one.
        ctx.symbols[id].value = 0xdeadbeef;
        let mut esym = ElfSym::<E>::default();
        esym.st_shndx_mut().set(SHN_ABS as u16);
        esym.set_type(STT_NOTYPE);
        esym.set_bind(STB_GLOBAL);
        esym.set_visibility(STV_DEFAULT);
        ctx.internal_esyms.push(esym);
    };

    // Add --defsym'd symbols
    for (name, _) in ctx.args.defsyms.clone() {
        add(ctx, &mut obj, &name);
    }
    // Add --section-order symbols
    for order in ctx.args.section_order.clone() {
        if order.kind == SectionOrderKind::Symbol {
            add(ctx, &mut obj, &order.name);
        }
    }

    obj.base.elf_syms = Cow::Owned(ctx.internal_esyms.clone());
    let id = ObjId(ctx.objs.push(Box::new(obj)));
    ctx.internal_obj = Some(id);
    if ctx.file_by_priority.is_empty() {
        ctx.file_by_priority.push(None);
    }
    ctx.file_by_priority[0] = Some(FileId::Obj(id));
}

fn start_stop_name<E: Arch>(ctx: &Context<E>, id: ChunkId) -> Option<String> {
    let hdr = ctx.chunk_header(id);
    if !hdr.is_alloc() || hdr.name.is_empty() {
        return None;
    }
    if is_c_identifier(hdr.name) {
        return Some(String::from_utf8_lossy(hdr.name).into_owned());
    }
    if ctx.args.start_stop {
        let s = String::from_utf8_lossy(hdr.name).into_owned();
        let s = s.strip_prefix('.').unwrap_or(&s);
        return Some(
            s.chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect(),
        );
    }
    None
}

/// Resolves the internal object's symbols; its definitions win ties by
/// virtue of its priority 0.
fn resolve_internal_symbols<E: Arch>(ctx: &mut Context<E>) {
    let id = ctx.internal_obj.unwrap();
    let obj = &ctx.objs[id.index()];
    for i in obj.base.first_global..obj.base.elf_syms.len() {
        let esym = &obj.base.elf_syms[i];
        let sym_id = obj.base.symbols[i];
        let rank = symbol_resolution_rank(esym, false, false, 0);
        if rank < current_rank(ctx, &ctx.symbols[sym_id]) {
            let sym = &mut ctx.symbols[sym_id];
            sym.set_file(FileId::Obj(id));
            sym.clear_origin();
            sym.value = esym.st_value().get();
            sym.sym_idx = i as u32;
            sym.set_esym(esym);
            sym.ver_idx = ctx.default_version;
            sym.set_weak(esym.is_weak());
            sym.set_versioned_default(false);
        }
    }
}

pub fn add_synthetic_symbols<E: Arch>(ctx: &mut Context<E>) {
    let obj_id = ctx.internal_obj.unwrap();

    fn add<E: Arch>(ctx: &mut Context<E>, name: &str, ty: u32) -> SymbolId {
        let mut esym = ElfSym::<E>::default();
        esym.st_shndx_mut().set(SHN_ABS as u16);
        esym.set_type(ty);
        esym.set_bind(STB_GLOBAL);
        esym.set_visibility(STV_HIDDEN);
        ctx.internal_esyms.push(esym);
        let id = ctx.get_symbol(name.as_bytes());
        ctx.symbols[id].value = 0xdeadbeef; // unique dummy value
        let obj = ctx.internal_obj.unwrap();
        ctx.objs[obj.index()].base.symbols.push(id);
        id
    }

    let s = |ctx: &mut Context<E>, name: &str| add(ctx, name, STT_NOTYPE);

    ctx.syms.ehdr_start = Some(s(ctx, "__ehdr_start"));
    ctx.syms.init_array_start = Some(s(ctx, "__init_array_start"));
    ctx.syms.init_array_end = Some(s(ctx, "__init_array_end"));
    ctx.syms.fini_array_start = Some(s(ctx, "__fini_array_start"));
    ctx.syms.fini_array_end = Some(s(ctx, "__fini_array_end"));
    ctx.syms.preinit_array_start = Some(s(ctx, "__preinit_array_start"));
    ctx.syms.preinit_array_end = Some(s(ctx, "__preinit_array_end"));
    ctx.syms.dynamic = Some(s(ctx, "_DYNAMIC"));
    ctx.syms.global_offset_table = Some(s(ctx, "_GLOBAL_OFFSET_TABLE_"));
    ctx.syms.procedure_linkage_table = Some(s(ctx, "_PROCEDURE_LINKAGE_TABLE_"));
    ctx.syms.bss_start = Some(s(ctx, "__bss_start"));
    ctx.syms.end_ = Some(s(ctx, "_end"));
    ctx.syms.etext_ = Some(s(ctx, "_etext"));
    ctx.syms.edata_ = Some(s(ctx, "_edata"));
    ctx.syms.executable_start = Some(s(ctx, "__executable_start"));
    ctx.syms.rel_iplt_start = Some(s(
        ctx,
        if E::IS_RELA {
            "__rela_iplt_start"
        } else {
            "__rel_iplt_start"
        },
    ));
    ctx.syms.rel_iplt_end = Some(s(
        ctx,
        if E::IS_RELA {
            "__rela_iplt_end"
        } else {
            "__rel_iplt_end"
        },
    ));

    if ctx.args.eh_frame_hdr {
        ctx.syms.gnu_eh_frame_hdr = Some(s(ctx, "__GNU_EH_FRAME_HDR"));
    }

    for (name, slot) in [("end", 0), ("etext", 1), ("edata", 2), ("__dso_handle", 3)] {
        let id = ctx.get_symbol(name.as_bytes());
        if ctx.symbols[id].file().is_none() {
            let id = s(ctx, name);
            match slot {
                0 => ctx.syms.end = Some(id),
                1 => ctx.syms.etext = Some(id),
                2 => ctx.syms.edata = Some(id),
                _ => ctx.syms.dso_handle = Some(id),
            }
        }
    }

    if E::SUPPORTS_TLSDESC {
        ctx.syms.tls_module_base = Some(add(ctx, "_TLS_MODULE_BASE_", STT_TLS));
    }
    if E::IS_RISCV {
        let id = s(ctx, "__global_pointer$");
        ctx.syms.global_pointer = Some(id);
        if ctx.dynamic.is_some() && !ctx.args.shared {
            ctx.symbols[id].set_exported(true);
        }
    }
    if E::FAMILY == Family::Arm32 {
        ctx.syms.exidx_start = Some(s(ctx, "__exidx_start"));
        ctx.syms.exidx_end = Some(s(ctx, "__exidx_end"));
    }
    if E::IS_PPC64 {
        ctx.syms.toc = Some(s(ctx, ".TOC."));
    }
    if E::FAMILY == Family::Ppc64V2 {
        for &(label, _) in crate::arch::ppc64v2::SAVE_RESTORE_INSNS {
            if !label.is_empty() {
                s(ctx, label);
            }
        }
    }
    if E::FAMILY == Family::Ppc32 {
        ctx.syms.sda_base = Some(s(ctx, "_SDA_BASE_"));
    }
    if E::IS_SPARC {
        // TLS_GD_CALL and TLS_LDM_CALL relocations call it implicitly.
        ctx.syms.tls_get_addr = Some(ctx.get_symbol(b"__tls_get_addr"));
    }

    let add_start_stop = |ctx: &mut Context<E>, name: String| {
        let id = s(ctx, &name);
        if ctx.args.z_start_stop_visibility_protected {
            ctx.symbols[id].set_exported(true);
        }
    };
    for id in ctx.chunks.clone() {
        if let Some(name) = start_stop_name(ctx, id) {
            add_start_stop(ctx, format!("__start_{name}"));
            add_start_stop(ctx, format!("__stop_{name}"));
            if ctx.args.physical_image_base.is_some() {
                add_start_stop(ctx, format!("__phys_start_{name}"));
                add_start_stop(ctx, format!("__phys_stop_{name}"));
            }
        }
    }

    ctx.objs[obj_id.index()].base.elf_syms = Cow::Owned(ctx.internal_esyms.clone());
    resolve_internal_symbols(ctx);

    // Make all synthetic symbols relative ones by associating them to
    // a dummy output section.
    let syms = ctx.objs[obj_id.index()].base.symbols.clone();
    for id in &syms {
        let sym = &mut ctx.symbols[*id];
        if sym.file() == Some(FileId::Obj(obj_id)) {
            sym.set_output_chunk(ChunkId::Symtab);
            sym.set_imported(false);
        }
    }

    // Handle --defsym symbols.
    for (i, (name, value)) in ctx.args.defsyms.clone().iter().enumerate() {
        let sym1 = ctx.get_symbol(name.as_bytes());
        match value {
            DefsymValue::Symbol(target) => {
                let sym2 = ctx.get_symbol(target.as_bytes());
                if ctx.symbols[sym2].file().is_none() {
                    error!(ctx, "--defsym: undefined symbol: {}", ctx.symbols[sym2]);
                    continue;
                }
                let sym2_esym = ctx.symbols[sym2].esym(ctx);
                let obj = &mut ctx.objs[obj_id.index()];
                let esym = &mut obj.base.elf_syms.to_mut()[i + 1];
                esym.set_type(sym2_esym.st_type());
                if E::FAMILY == Family::Ppc64V2 {
                    esym.set_ppc64_local_entry(sym2_esym.ppc64_local_entry());
                }
                ctx.internal_esyms[i + 1] = *esym;
                if ctx.symbols[sym1].file() == Some(FileId::Obj(obj_id)) {
                    ctx.symbols[sym1].set_esym(esym);
                }
                if ctx.symbols[sym2].is_absolute() {
                    ctx.symbols[sym1].clear_origin();
                }
            }
            DefsymValue::Addr(_) => ctx.symbols[sym1].clear_origin(),
        }
    }
}

pub fn apply_section_align<E: Arch>(ctx: &mut Context<E>) {
    if ctx.args.section_align.is_empty() {
        return;
    }
    for osec in &mut ctx.output_sections {
        if let Some(&align) = ctx
            .args
            .section_align
            .get(&*String::from_utf8_lossy(osec.hdr.name))
        {
            osec.hdr.shdr.sh_addralign.set(align);
        }
    }
}

pub fn check_cet_errors<E: Arch>(ctx: &Context<E>) {
    let warning = ctx.args.z_cet_report == CetReportKind::Warning;
    let has_feature = |file: &ObjectFile<E>, feature: u32| {
        file.gnu_properties
            .get(&GNU_PROPERTY_X86_FEATURE_1_AND)
            .is_some_and(|v| v & feature != 0)
    };
    for file in &ctx.objs {
        if ctx.is_internal(file.id()) {
            continue;
        }
        for (feature, name) in [
            (GNU_PROPERTY_X86_FEATURE_1_IBT, "IBT"),
            (GNU_PROPERTY_X86_FEATURE_1_SHSTK, "SHSTK"),
        ] {
            if !has_feature(file, feature) {
                if warning {
                    warn!(
                        ctx,
                        "{file}: -cet-report=warning: missing GNU_PROPERTY_X86_FEATURE_1_{name}"
                    );
                } else {
                    error!(
                        ctx,
                        "{file}: -cet-report=error: missing GNU_PROPERTY_X86_FEATURE_1_{name}"
                    );
                }
            }
        }
    }
}

pub fn print_dependencies<E: Arch>(ctx: &Context<E>) {
    out!(
        ctx,
        "# This is an output of the mold linker's --print-dependencies option.\n\
         #\n\
         # Each line consists of 4 fields, <section1>, <section2>, <symbol-type> and\n\
         # <symbol>, separated by tab characters. It indicates that <section1> depends\n\
         # on <section2> to use <symbol>. <symbol-type> is either \"u\" or \"w\" for\n\
         # regular undefined or weak undefined, respectively.\n\
         #\n\
         # If you want to obtain dependency information per function granularity,\n\
         # compile source files with the -ffunction-sections compiler flag."
    );

    let println = |src: &dyn std::fmt::Display, sym: &Symbol, is_weak: bool| {
        let kind = if is_weak { 'w' } else { 'u' };
        match sym.input_section() {
            Some(sec) => out!(ctx, "{src}\t{}\t{kind}\t{sym}", ctx.section_display(sec)),
            None => out!(
                ctx,
                "{src}\t{}\t{kind}\t{sym}",
                ctx.file_display(sym.file().unwrap())
            ),
        }
    };

    for file in &ctx.objs {
        for isec in file.input_sections() {
            let mut visited: HashSet<SymbolId> = HashSet::new();
            for r in isec.rels::<E>(file) {
                if r.r_type() == R_NONE || file.base.elf_syms.len() <= r.r_sym() as usize {
                    continue;
                }
                let esym = &file.base.elf_syms[r.r_sym() as usize];
                let id = file.base.symbols[r.r_sym() as usize];
                let sym = &ctx.symbols[id];
                if esym.is_undef()
                    && sym.file().is_some()
                    && sym.file() != Some(FileId::Obj(file.id()))
                    && visited.insert(id)
                {
                    println(&isec.display(file), sym, esym.is_weak());
                }
            }
        }
    }
    for file in &ctx.dsos {
        for i in 0..file.base.elf_syms.len() {
            let esym = &file.base.elf_syms[i];
            let sym = &ctx.symbols[file.base.symbols[i]];
            if esym.is_undef() && sym.file().is_some() && sym.file() != Some(FileId::Dso(file.id()))
            {
                println(file, sym, esym.is_weak());
            }
        }
    }
}

fn create_response_file<E: Arch>(ctx: &Context<E>) -> String {
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut out = format!("-C {}\n", cwd.strip_prefix('/').unwrap_or(&cwd));
    if cwd != "/" {
        out.push_str("--chroot ..");
        let depth = cwd.matches('/').count();
        for _ in 1..depth {
            out.push_str("/..");
        }
        out.push('\n');
    }
    for arg in &ctx.cmdline_args[1..] {
        if arg != "-repro" && arg != "--repro" {
            out.push_str(arg);
            out.push('\n');
        }
    }
    out
}

pub fn write_repro_file<E: Arch>(ctx: &Context<E>) {
    let _t = ctx.timer("write_repro_file");
    let path = format!("{}.repro.tar", ctx.args.output);
    let basedir = format!("{}.repro", path_filename(&ctx.args.output));
    let mut tar = crate::util::tar::TarWriter::open(&path, &basedir)
        .unwrap_or_else(|e| fatal!(ctx, "cannot open {path}: {e}"));

    let write = |tar: &mut crate::util::tar::TarWriter, name: &str, data: &[u8]| {
        tar.append(name, data)
            .unwrap_or_else(|e| fatal!(ctx, "{path}: write failed: {e}"));
    };
    write(
        &mut tar,
        "response.txt",
        create_response_file(ctx).as_bytes(),
    );
    write(
        &mut tar,
        "version.txt",
        format!("{}\n", crate::cmdline::VERSION).as_bytes(),
    );

    let mut seen: HashSet<String> = HashSet::new();
    let files: Vec<&crate::mapped_file::MappedFile> = ctx
        .objs
        .iter()
        .filter_map(|f| f.base.mf)
        .chain(ctx.dsos.iter().filter_map(|f| f.base.mf))
        .collect();
    for mf in files {
        let top = mf.parent.unwrap_or(mf);
        if seen.insert(top.name.clone()) {
            // We reopen a file because we may have modified the contents of mf
            // in memory, which is mapped with PROT_WRITE and MAP_PRIVATE.
            let reopened =
                crate::mapped_file::must_open_file(&ctx.diag, &ctx.args.chroot, &top.name);
            let abs = std::fs::canonicalize(&top.name)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or(top.name.clone());
            write(&mut tar, &abs, reopened.data());
        }
    }
}

pub fn check_duplicate_symbols<E: Arch>(ctx: &Context<E>) {
    let _t = ctx.timer("check_duplicate_symbols");
    ctx.objs.par_iter().for_each(|file| {
        if !file.base.is_reachable() {
            return;
        }
        let file_id = FileId::Obj(file.id());
        for i in file.base.first_global..file.base.elf_syms.len() {
            let esym = &file.base.elf_syms[i];
            let sym = &ctx.symbols[file.base.symbols[i]];

            // Skip if our symbol is undef or weak
            let Some(owner) = sym.file() else { continue };
            if owner == file_id
                || ctx.internal_obj.map(FileId::Obj) == Some(owner)
                || esym.is_undef()
                || esym.is_common()
                || esym.st_bind() == STB_WEAK
            {
                continue;
            }
            // Skip if our symbol is in a dead section. In most cases, the
            // section has been eliminated due to comdat deduplication.
            if !esym.is_abs() {
                match file.symbol_section(i) {
                    Some(isec) if isec.is_alive() => {}
                    _ => continue,
                }
            }
            // Skip if the symbol is a deduplicated comdat symbol that is in
            // an IR file.
            if file.is_lto_input && file.lto_comdat_discarded[i] {
                continue;
            }
            // Skip if one side is an LTO IR object and the other is not.
            // The LTO backend resolves conflicts between IR and regular objects
            // on its own; only IR-vs-IR duplicates need to be caught here.
            if let FileId::Obj(o) = owner {
                if ctx.objs[o.index()].is_lto_input != file.is_lto_input {
                    continue;
                }
            }
            error!(
                ctx,
                "duplicate symbol: {file}: {}: {sym}",
                ctx.file_display(owner)
            );
        }
    });
    ctx.checkpoint();
}

// A default-versioned symbol `foo@@VER` can also be referred to as
// `foo@VER`, so exporting both `foo@@VER` and `foo@VER` would produce a
// dynamic symbol table with two definitions of the same versioned name,
// and which one a versioned reference binds to would be up to the
// dynamic loader. GNU ld and lld reject this; so do we.
pub fn check_symbol_version_conflicts<E: Arch>(ctx: &Context<E>) {
    if ctx.dynamic.is_none() || ctx.args.allow_multiple_definition {
        return;
    }
    let _t = ctx.timer("check_symbol_version_conflicts");
    for &id in ctx.dynsym.symbols.iter().skip(1).flatten() {
        let sym = &ctx.symbols[id];
        let Some(FileId::Obj(obj)) = sym.file() else {
            continue;
        };
        if sym.is_weak()
            || sym.ver_idx as u32 == VER_NDX_UNSPECIFIED
            || sym.ver_idx as u32 & VERSYM_HIDDEN == 0
        {
            continue;
        }
        let Some(id2) = ctx.symbols.lookup(sym.name()) else {
            continue;
        };
        if id2 == id {
            continue;
        }
        let sym2 = &ctx.symbols[id2];
        if let Some(FileId::Obj(_)) = sym2.file() {
            if !sym2.is_weak() && sym2.ver_idx as u32 == (sym.ver_idx as u32 & !VERSYM_HIDDEN) {
                let file = &ctx.objs[obj.index()];
                error!(
                    ctx,
                    "duplicate symbol: {file}: {}: {}",
                    ctx.file_display(sym2.file().unwrap()),
                    crate::util::display(file.base.symbol_name_in(sym.sym_idx as usize))
                );
            }
        }
    }
    ctx.checkpoint();
}

// GCC and Clang set the SHT_NOBITS flag for an output section only if the
// section name is .bss or similar. Sections with nonstandard names, such
// as those defined with __attribute__((section(".sectname"))), are always
// emitted as non-BSS sections even if they contain only uninitialized
// variables.
//
// This function finds such allocated but all-zero sections and converts
// them into BSS, reducing the output file size.
pub fn convert_zero_to_bss<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("convert_zero_to_bss");
    ctx.objs.par_iter_mut().for_each(|file| {
        if !file.base.is_reachable() {
            return;
        }
        let sections: Vec<u32> = file
            .input_sections()
            .filter(|isec| {
                let flags = isec.sh_flags as u32;
                isec.is_alive()
                    && isec.sh_type(file) == SHT_PROGBITS
                    && flags & SHF_ALLOC != 0
                    && flags & SHF_WRITE != 0
                    && flags & SHF_EXECINSTR == 0
                    && isec.rels::<E>(file).is_empty()
                    && !isec.contents().is_empty()
                    && isec.contents().iter().all(|&b| b == 0)
            })
            .map(|isec| isec.shndx)
            .collect();
        for shndx in sections {
            let isec = file.section_mut(shndx as usize).unwrap();
            isec.set_nobits();
            isec.clear_contents();
        }
    });
}

fn has_dso_definition<E: Arch>(ctx: &Context<E>, id: SymbolId) -> bool {
    ctx.dsos.pool_iter().any(|dso| {
        dso.base
            .symbols
            .iter()
            .zip(dso.base.elf_syms.iter())
            .any(|(&s, esym)| s == id && !esym.is_undef())
    })
}

// If --no-allow-shlib-undefined is specified, we report errors on
// unresolved symbols in shared libraries. This is useful when you are
// creating a final executable and want to make sure that all symbols
// including ones in shared libraries have been resolved.
//
// If you do not pass --no-allow-shlib-undefined, undefined symbols in
// shared libraries will be reported as run-time error by the dynamic
// linker.
pub fn check_shlib_undefined<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("check_shlib_undefined");

    // Skip test if we don't have a complete set of shared object files
    // for the program, because if there's a missing .so, an undefined
    // symbol might be defined by that library.
    let complete = ctx.dsos.iter().all(|dso| {
        dso.dt_needed(&ctx.diag)
            .iter()
            .all(|needed| ctx.dso_sonames.contains(&*String::from_utf8_lossy(needed)))
    });

    if complete {
        ctx.dsos.par_iter().for_each(|file| {
            // Check if all undefined symbols have been resolved.
            for i in 0..file.base.elf_syms.len() {
                let esym = &file.base.elf_syms[i];
                let id = file.base.symbols[i];
                let sym = &ctx.symbols[id];
                // Dynamic symbol table for SPARC contains bogus entries which
                // we need to ignore
                let is_sparc_register = E::IS_SPARC && esym.st_type() == STT_SPARC_REGISTER;
                let defined = sym.file().is_some() && sym.visibility() != STV_HIDDEN;
                if esym.is_undef()
                    && !esym.is_weak()
                    && !is_sparc_register
                    && !defined
                    && !has_dso_definition(ctx, id)
                {
                    error!(
                        ctx,
                        "{file}: --no-allow-shlib-undefined: undefined symbol: {sym}"
                    );
                }
            }
        });
    }

    // Beyond this point, DSOs that are not referenced directly by any
    // object file are not needed. They were kept by
    // SharedFile<E>::mark_live_objects just for this pass. Therefore,
    // remove unneeded DSOs from the list now.
    for file in &ctx.dsos {
        file.base.set_reachable(!file.base.as_needed);
    }
    let Context {
        objs,
        dsos,
        symbols,
        ..
    } = ctx;
    objs.par_iter().for_each(|file| {
        for &id in file.base.global_symbols() {
            if let Some(FileId::Dso(dso)) = symbols[id].file() {
                dsos[dso.index()].base.set_reachable(true);
            }
        }
    });
    remove_unreachable_dsos(ctx);
}

/// Drops DSOs that are no longer needed, renumbering the rest.
pub fn remove_unreachable_dsos<E: Arch>(ctx: &mut Context<E>) {
    ctx.dsos.retain(|file| file.base.is_reachable());
}

pub fn check_symbol_types<E: Arch>(ctx: &Context<E>) {
    let _t = ctx.timer("check_symbol_types");
    let canonicalize = |ty: u32| match ty {
        STT_GNU_IFUNC => STT_FUNC,
        STT_COMMON => STT_OBJECT,
        ty => ty,
    };
    let check = |file: &dyn std::fmt::Display, file_id: FileId, sym: &Symbol, st_type2: u32| {
        let esym1 = &sym.esym(ctx);
        if let Some(owner) = sym.file() {
            if owner != file_id
                && esym1.st_type() != STT_NOTYPE
                && st_type2 != STT_NOTYPE
                && canonicalize(esym1.st_type()) != canonicalize(st_type2)
            {
                warn!(
                    ctx,
                    "symbol type mismatch: {sym}\n>>> defined in {} as {}\n>>> defined in {file} as {}",
                    ctx.file_display(owner),
                    stt_to_string(esym1.st_type()),
                    stt_to_string(st_type2)
                );
            }
        }
    };

    ctx.objs.par_iter().for_each(|file| {
        let id = FileId::Obj(file.id());
        for i in file.base.first_global..file.base.elf_syms.len() {
            let sym = &ctx.symbols[file.base.symbols[i]];
            if sym.file().is_some() && sym.file() != Some(id) {
                check(file, id, sym, file.base.elf_syms[i].st_type());
            }
        }
    });
    ctx.dsos.par_iter().for_each(|file| {
        let id = FileId::Dso(file.id());
        for i in 0..file.base.elf_syms.len() {
            let sym = &ctx.symbols[file.base.symbols[i]];
            if sym.file().is_some() && sym.file() != Some(id) {
                check(file, id, sym, file.base.elf_syms[i].st_type());
            }
            let id2 = file.symbols2[i];
            if id2 != SymbolId::NONE {
                let sym = &ctx.symbols[id2];
                if sym.file().is_some() && sym.file() != Some(id) {
                    check(file, id, sym, file.base.elf_syms[i].st_type());
                }
            }
        }
    });
}

/// The `.N` suffix of an init/fini section name, or 65536.
fn init_fini_priority(name: &[u8]) -> i64 {
    numeric_suffix(name).unwrap_or(65536)
}

fn numeric_suffix(name: &[u8]) -> Option<i64> {
    let pos = name.iter().rposition(|&b| b == b'.')?;
    let digits = &name[pos + 1..];
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(digits).ok()?.parse().ok()
}

fn ctor_dtor_priority<E: Arch>(ctx: &Context<E>, id: InputSectionId) -> i64 {
    // crtbegin.o and crtend.o contain marker symbols such as
    // __CTOR_LIST__ or __DTOR_LIST__. So they have to be at the
    // beginning or end of the section.
    let isec = ctx.input_section(id);
    let file = &ctx.objs[isec.file.index()];
    let filename = &file.base.filename;
    if filename.contains("crtbegin") {
        return -2;
    }
    if filename.contains("crtend") {
        return 65536;
    }
    numeric_suffix(isec.name(file)).unwrap_or(-1)
}

pub fn sort_init_fini<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("sort_init_fini");
    for i in 0..ctx.output_sections.len() {
        let name = ctx.output_sections[i].hdr.name;
        if name != b".init_array" && name != b".preinit_array" && name != b".fini_array" {
            continue;
        }
        let mut members = std::mem::take(&mut ctx.output_sections[i].members);
        if ctx.args.shuffle_sections == ShuffleSectionsKind::Reverse {
            members.reverse();
        }
        let mut entries: Vec<(InputSectionId, i64)> = members
            .iter()
            .map(|&id| {
                let isec = ctx.input_section(id);
                let name = isec.name(&ctx.objs[isec.file.index()]);
                let prio = if name.starts_with(b".ctors") || name.starts_with(b".dtors") {
                    65535 - ctor_dtor_priority(ctx, id)
                } else {
                    init_fini_priority(name)
                };
                (id, prio)
            })
            .collect();
        entries.sort_by_key(|e| e.1);
        ctx.output_sections[i].members = entries.into_iter().map(|e| e.0).collect();
    }
}

pub fn sort_ctor_dtor<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("sort_ctor_dtor");
    for i in 0..ctx.output_sections.len() {
        let name = ctx.output_sections[i].hdr.name;
        if name != b".ctors" && name != b".dtors" {
            continue;
        }
        let mut members = std::mem::take(&mut ctx.output_sections[i].members);
        if ctx.args.shuffle_sections != ShuffleSectionsKind::Reverse {
            members.reverse();
        }
        let mut entries: Vec<(InputSectionId, i64)> = members
            .iter()
            .map(|&id| (id, ctor_dtor_priority(ctx, id)))
            .collect();
        entries.sort_by_key(|e| e.1);
        ctx.output_sections[i].members = entries.into_iter().map(|e| e.0).collect();
    }
}

// Debug sections in an input object file refer to other debug sections in
// the same file using section offsets. The offsets are 64 bits in DWARF64
// and 32 bits in DWARF32.
//
// GCC and Clang emit DWARF32 debug info by default even for 64-bit code.
// That is, 64-bit values are used for addresses, and 32-bit values for
// references between debug sections. This makes sense for ordinary programs
// because it reduces the size of the debug info sections.
//
// You can change the format to DWARF64 by passing `-gdwarf64`. Therefore,
// the "right" approach to build an extremely large program in debug mode is
// to recompile everything with `-gdwarf64`. However, that’s often not
// feasiable for various reasons.
//
// If we don't do anything about it, a relocation overflow could occur if
// any output debug section exceeds 4 GiB in size, making it almost
// impossible for users to link an object file compiled without `-gdwarf64`
// to an extremely large program.
//
// This function works around the issue by sorting output debug section
// contents so that DWARF32 input sections are at the start of the output
// section followed by DWARF64 input sections. By doing this, we can avoid
// relocation overflow until the total size of DWARF32 input sections alone
// exceeds 4 GiB.
pub fn sort_debug_info_sections<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("sort_debug_info_sections");

    // True if mold is running under ctest
    let is_in_test = std::env::var("MOLD_DEBUG").is_ok_and(|v| !v.is_empty());

    // Get lists of output debug sections that need sorting
    let vec1: Vec<OutputSectionId> = (0..ctx.output_sections.len())
        .map(|i| OutputSectionId::new(i as u32))
        .filter(|&id| {
            let osec = &ctx.output_sections[id.index()];
            !osec.hdr.is_alloc()
                && osec.hdr.name.starts_with(b".debug_")
                && (osec.hdr.shdr.sh_size.get() >= u32::MAX as u64 || is_in_test)
        })
        .collect();
    let vec2: Vec<crate::output_chunks::merged::MergedSectionId> = (0..ctx.merged_sections.len())
        .map(|i| crate::output_chunks::merged::MergedSectionId(i as u32))
        .filter(|&id| {
            let msec = &ctx.merged_sections[id.index()];
            !msec.is_alloc()
                && msec.hdr.name.starts_with(b".debug_")
                && (msec.hdr.shdr.sh_size.get() >= u32::MAX as u64 || is_in_test)
        })
        .collect();
    if vec1.is_empty() && vec2.is_empty() {
        return;
    }

    // Record whether each input file contains DWARF32 debug info.
    {
        let Context { objs, diag, .. } = ctx;
        objs.par_iter_mut().for_each(|file| {
            file.is_dwarf32 = !file.debug_info_sections.is_empty() && file.is_dwarf32(diag);
        });
    }

    // Unless DWARF32 and DWARF64 debug info come from different files, it
    // doesn't make sense to sort sections.
    let has_dwarf32 = ctx.objs.iter().any(|f| f.is_dwarf32);
    let has_dwarf64 = ctx
        .objs
        .iter()
        .any(|f| !f.is_dwarf32 && !f.debug_info_sections.is_empty());
    if !has_dwarf32 || !has_dwarf64 {
        return;
    }

    // Reorder input sections in the output section so that DWARF32
    // precededs DWARF64.
    for id in vec1 {
        let section_arena = &ctx.section_arena;
        let objs = &ctx.objs;
        let osec = &mut ctx.output_sections[id.index()];
        // We can't partition osec->members in place because stable_partition
        // may move elements to a heap-allocated temporary buffer, and an
        // ArenaPtr cannot live more than 8 GiB away from its target.
        let (a, b): (Vec<InputSectionId>, Vec<InputSectionId>) = osec
            .members
            .iter()
            .partition(|&&m| objs[section_arena.section(m).file.index()].is_dwarf32);
        osec.members = a.into_iter().chain(b).collect();
        output_chunks::compute_section_size(ctx, ChunkId::Output(id));
    }

    // Reorder strings in .debug_str and the like
    for id in &vec2 {
        let msec = &ctx.merged_sections[id.index()];
        for file in ctx.objs.iter().filter(|f| f.is_dwarf32) {
            for m in file.mergeable_sections().filter(|m| m.parent == *id) {
                for &frag in &m.fragments {
                    msec.fragments.get(frag).set_32bit();
                }
            }
        }
    }
    for id in vec2 {
        output_chunks::compute_section_size(ctx, ChunkId::Merged(id));
    }
}

// .ctors/.dtors serves the same purpose as .init_array/.fini_array,
// albeit with very subtly differences. Both contain pointers to
// initializer/finalizer functions. The runtime executes them one by one
// but in the exact opposite order to one another. Therefore, if we are to
// place the contents of .ctors/.dtors into .init_array/.fini_array, we
// need to reverse them.
//
// It's unfortunate that we have both .ctors/.dtors and
// .init_array/.fini_array in ELF for historical reasons, but that's
// the reality we need to deal with.
pub fn fixup_ctors_in_init_array<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("fixup_ctors_in_init_array");
    let word = E::WORD_SIZE;

    for (osec_name, prefix) in [
        (&b".init_array"[..], &b".ctors"[..]),
        (&b".fini_array"[..], &b".dtors"[..]),
    ] {
        let Some(ChunkId::Output(id)) = ctx.find_chunk_by_name(osec_name) else {
            continue;
        };
        let members = ctx.output_sections[id.index()].members.clone();
        for m in members {
            let section_ref = ctx.input_section(m).section_ref();
            if !ctx
                .input_section(m)
                .name(&ctx.objs[section_ref.file.index()])
                .starts_with(prefix)
            {
                continue;
            }
            let diag = &ctx.diag;
            let file = &ctx.objs[section_ref.file.index()];
            let isec = ctx.input_section(m);
            if !isec.sh_size.is_multiple_of(word as u64) {
                fatal!(diag, "{}: section corrupted", isec.display(file));
            }
            let mut contents = isec.contents().to_vec();
            let n = contents.len() / word;
            for i in 0..n / 2 {
                let (a, b) = (i * word, (n - 1 - i) * word);
                for k in 0..word {
                    contents.swap(a + k, b + k);
                }
            }
            let size = isec.sh_size;
            let mut rels = isec.rels::<E>(file).to_vec();
            for r in &mut rels {
                r.set_r_offset(size - r.r_offset() - word as u64);
            }
            rels.sort_by_key(|r| r.r_offset());
            let file = &mut ctx.objs[section_ref.file.index()];
            file.section_mut(section_ref.shndx as usize)
                .unwrap()
                .set_contents(leak_bytes(contents));
            file.rels_mut(section_ref.shndx).copy_from_slice(&rels);
        }
    }
}

fn shuffle(vec: &mut [InputSectionId], mut seed: u64) {
    if vec.is_empty() {
        return;
    }
    // Xorshift random number generator. We use this RNG because it is
    // measurably faster than MT19937.
    let mut rand = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    // The Fisher-Yates shuffling algorithm.
    //
    // We don't want to use std::shuffle for build reproducibility. That is,
    // std::shuffle's implementation is not guaranteed to be the same across
    // platform, so even though the result is guaranteed to be randomly
    // shuffled, the exact order may be different across implementations.
    //
    // We are not using std::uniform_int_distribution for the same reason.
    for i in 0..vec.len() - 1 {
        let j = i + (rand() % (vec.len() - i) as u64) as usize;
        vec.swap(i, j);
    }
}

pub fn shuffle_sections<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("shuffle_sections");
    let is_eligible = |osec: &OutputSection<E>| {
        let name = osec.hdr.name;
        osec.hdr.is_alloc()
            && name != b".init"
            && name != b".fini"
            && name != b".ctors"
            && name != b".dtors"
            && name != b".init_array"
            && name != b".preinit_array"
            && name != b".fini_array"
    };
    let kind = ctx.args.shuffle_sections;
    let seed = ctx.args.shuffle_sections_seed;
    ctx.output_sections
        .par_iter_mut()
        .filter(|o| is_eligible(o))
        .for_each(|osec| match kind {
            ShuffleSectionsKind::Shuffle => {
                let s = seed.wrapping_add(xxhash_rust::xxh3::xxh3_64(osec.hdr.name));
                shuffle(&mut osec.members, s);
            }
            ShuffleSectionsKind::Reverse => osec.members.reverse(),
            ShuffleSectionsKind::None => {}
        });
}

pub fn add_dynamic_strings<E: Arch>(ctx: &mut Context<E>) {
    let dso_ids: Vec<_> = ctx
        .dsos
        .iter()
        .map(crate::input_files::SharedFile::id)
        .collect();
    for id in dso_ids {
        let audit = ctx.dsos[id.index()].dt_audit(&ctx.diag);
        if !audit.is_empty() {
            if !ctx.args.depaudit.is_empty() {
                ctx.args.depaudit.push(':');
            }
            ctx.args.depaudit.push_str(&String::from_utf8_lossy(audit));
        }
    }
    let mut strings: Vec<Vec<u8>> = ctx
        .dsos
        .iter()
        .map(|d| d.soname.clone().into_bytes())
        .collect();
    strings.extend(ctx.args.auxiliary.iter().map(|s| s.clone().into_bytes()));
    strings.extend(ctx.args.filter.iter().map(|s| s.clone().into_bytes()));
    strings.push(ctx.args.audit.clone().into_bytes());
    strings.push(ctx.args.depaudit.clone().into_bytes());
    strings.push(ctx.args.rpaths.clone().into_bytes());
    strings.push(ctx.args.soname.clone().into_bytes());
    for s in strings {
        if !s.is_empty() {
            ctx.dynstr.add_string(&s);
        }
    }
}

pub fn compute_section_sizes<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("compute_section_sizes");

    // On a target with range extension thunks, an executable section gets
    // its thunks and its layout at the same time.
    let needs_thunks = |ctx: &Context<E>, id: ChunkId| match id {
        ChunkId::Output(osec) => {
            E::NEEDS_THUNK
                && !ctx.args.relocatable
                && ctx.output_sections[osec.index()].hdr.shdr.sh_flags.get() & SHF_EXECINSTR as u64
                    != 0
        }
        _ => false,
    };

    // create_range_extension_thunks is not thread-safe
    for id in ctx.chunks.clone() {
        if let ChunkId::Output(osec) = id {
            if needs_thunks(ctx, id) {
                crate::thunks::create_range_extension_thunks(ctx, osec);
            }
        }
    }

    // Merged sections without SHF_ALLOC are resolved lazily here. C++ mold
    // processes all chunks in one parallel loop, so resolve their independent
    // parent sections concurrently too.
    {
        let mut members = merged_resolve_members(&mut ctx.objs, ctx.merged_sections.len());
        crate::output_chunks::merged::resolve_sections::<E>(
            &mut ctx.merged_sections,
            &mut members,
            crate::output_chunks::merged::ResolveOptions {
                allocated_only: false,
                gc_sections: ctx.args.gc_sections,
                diag: &ctx.diag,
                comment: ctx.comment,
                cmdline_args: &ctx.cmdline_args,
                timers: &ctx.timers,
            },
        );
    }

    // Output sections are laid out in parallel with each other, and each
    // in parallel over its members; the synthesized chunks follow.
    let sizes: Vec<(OutputSectionId, u64)> = {
        let ctx: &Context<E> = ctx;
        ctx.chunks
            .par_iter()
            .filter_map(|&id| match id {
                ChunkId::Output(osec) if !needs_thunks(ctx, id) => {
                    Some((osec, output_chunks::output_section::layout(ctx, osec)))
                }
                _ => None,
            })
            .collect()
    };
    for (osec, size) in sizes {
        ctx.output_sections[osec.index()].hdr.shdr.sh_size.set(size);
    }

    // Every merged section now owns all the mutable state needed for its
    // layout, just as each C++ Chunk does in the parallel chunk loop.
    ctx.merged_sections
        .par_iter_mut()
        .for_each(crate::output_chunks::merged::layout);

    for id in ctx.chunks.clone() {
        match id {
            ChunkId::Output(_) => {}
            ChunkId::Merged(_) => {}
            _ => output_chunks::compute_section_size(ctx, id),
        }
    }
}

// Find all unresolved symbols and attach them to the most appropriate files.
//
// Note that even a symbol that will be reported as an undefined symbol
// will get an owner file in this function. Such symbol will be reported
// by ObjectFile<E>::scan_relocations(). This is because we want to report
// errors only on symbols that are actually referenced.
pub fn claim_unresolved_symbols<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("claim_unresolved_symbols");

    // Find the references to symbols that no file defines. Nearly all
    // references are to defined symbols, which this pass leaves alone, so
    // they are filtered out without taking the symbols' locks, which
    // would otherwise be contended for every popular symbol.
    let candidates: Vec<(ObjId, usize)> = {
        let ctx_ref: &Context<E> = ctx;
        ctx_ref
            .objs
            .par_iter()
            .flat_map_iter(|file| {
                let file_id = file.id();
                let internal = ctx_ref.is_internal(file_id);
                (file.base.first_global..file.base.elf_syms.len())
                    .filter(move |&i| {
                        !internal
                            && file.base.elf_syms[i].is_undef()
                            && ctx_ref.symbols[file.base.symbols[i]].file().is_none()
                    })
                    .map(move |i| (file_id, i))
            })
            .collect()
    };

    for (obj_id, i) in candidates {
        let file = &ctx.objs[obj_id.index()];
        let esym = file.base.elf_syms[i];
        let id = file.base.symbols[i];
        let priority = file.base.priority;
        let file_id = FileId::Obj(obj_id);

        {
            let sym = &ctx.symbols[id];
            if let Some(owner) = sym.file() {
                if !sym.is_undef() || ctx.file(owner).priority <= priority {
                    continue;
                }
            }
        }

        let claim = |ctx: &mut Context<E>, is_imported: bool| {
            let sym = &ctx.symbols[id];
            if sym.is_traced() {
                out!(
                    ctx,
                    "trace-symbol: {}: unresolved{} symbol {sym}",
                    ctx.objs[obj_id.index()],
                    if esym.is_weak() { " weak" } else { "" }
                );
            }
            let default_version = ctx.default_version;
            let is_rust = ctx.objs[obj_id.index()].is_rust_obj;
            let sym = &mut ctx.symbols[id];
            sym.set_file(file_id);
            sym.clear_origin();
            sym.value = 0;
            sym.sym_idx = i as u32;
            sym.set_esym(&esym);
            sym.set_rust(is_rust);
            sym.set_weak(false);
            sym.set_imported(is_imported);
            sym.set_exported(false);
            sym.ver_idx = if is_imported {
                VER_NDX_UNSPECIFIED as u16
            } else {
                default_version
            };
        };

        let visibility = ctx.symbols[id].visibility();
        if esym.is_undef_weak() {
            if ctx.args.z_dynamic_undefined_weak && visibility != STV_HIDDEN {
                // Global weak undefined symbols are promoted to dynamic symbols
                // by default only when linking a DSO. We generally cannot do that
                // for executables because we may need to create a copy relocation
                // for a data symbol, but the symbol size is not available for an
                // unclaimed weak symbol.
                //
                // In contrast, GNU ld promotes weak symbols to dynamic ones even
                // for an executable as long as they don't need copy relocations
                // (i.e. they need only PLT entries.) That may result in an
                // inconsistent behavior of a linked program depending on whether
                // whether its object files were compiled with -fPIC or not. I think
                // that's bad semantics, so we don't do that.
                claim(ctx, true);
            } else {
                // Otherwise, weak undefs are converted to absolute symbols with value 0.
                claim(ctx, false);
            }
            continue;
        }

        // Traditionally, remaining undefined symbols cause a link failure
        // only when we are creating an executable. Undefined symbols in
        // shared objects are promoted to dynamic symbols, so that they'll
        // get another chance to be resolved at run-time. You can change the
        // behavior by passing `-z defs` to the linker.
        //
        // Even if `-z defs` is given, weak undefined symbols are still
        // promoted to dynamic symbols for compatibility with other linkers.
        // Some major programs, notably Firefox, depend on the behavior
        // (they use this loophole to export symbols from libxul.so).
        if ctx.args.shared
            && visibility != STV_HIDDEN
            && ctx.args.unresolved_symbols != UnresolvedKind::Error
        {
            claim(ctx, true);
            continue;
        }

        // Convert remaining undefined symbols to absolute symbols with value 0.
        claim(ctx, false);
    }
}

pub fn scan_relocations<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("scan_relocations");

    // Scan relocations to find dynamic symbols.
    {
        let ctx_ref: &Context<E> = ctx;
        ctx_ref
            .objs
            .par_iter()
            .for_each(|file| file.scan_relocations(ctx_ref));
    }
    // Exit if there was a relocation that refers an undefined symbol.
    ctx.checkpoint();

    // Word-size absolute relocations (e.g. R_X86_64_64) are handled
    // separately because they can be promoted to dynamic relocations.
    let results: Vec<(
        OutputSectionId,
        Vec<crate::output_chunks::output_section::AbsRel>,
        Vec<u64>,
    )> = {
        let ctx_ref: &Context<E> = ctx;
        (0..ctx_ref.output_sections.len())
            .into_par_iter()
            .map(|i| OutputSectionId::new(i as u32))
            .filter(|&id| ctx_ref.output_sections[id.index()].hdr.is_alloc())
            .map(|id| {
                let (abs_rels, offsets) =
                    crate::output_chunks::output_section::scan_abs_relocations(ctx_ref, id);
                (id, abs_rels, offsets)
            })
            .collect()
    };
    for (id, abs_rels, offsets) in results {
        let osec = &mut ctx.output_sections[id.index()];
        osec.abs_rels = abs_rels;
        osec.dynrel_offsets = offsets;
    }
    // Exit if the absolute-relocation pass reported an error.
    ctx.checkpoint();

    // Aggregate dynamic symbols to a single vector.
    let syms: Vec<SymbolId> = {
        let ctx_ref: &Context<E> = ctx;
        let objs: Vec<Vec<SymbolId>> = ctx_ref
            .objs
            .par_iter()
            .map(|file| {
                let id = FileId::Obj(file.id());
                file.base
                    .symbols
                    .iter()
                    .copied()
                    .filter(|&s| {
                        let sym = &ctx_ref.symbols[s];
                        sym.file() == Some(id)
                            && (sym.flags() != 0 || sym.is_imported() || sym.is_exported())
                    })
                    .collect()
            })
            .collect();
        let dsos: Vec<Vec<SymbolId>> = ctx_ref
            .dsos
            .par_iter()
            .map(|file| {
                let id = FileId::Dso(file.id());
                file.base
                    .symbols
                    .iter()
                    .copied()
                    .filter(|&s| {
                        let sym = &ctx_ref.symbols[s];
                        sym.file() == Some(id)
                            && (sym.flags() != 0 || sym.is_imported() || sym.is_exported())
                    })
                    .collect()
            })
            .collect();
        let mut syms = Vec::with_capacity(objs.iter().chain(&dsos).map(Vec::len).sum());
        syms.extend(objs.into_iter().chain(dsos).flatten());
        syms
    };

    if ctx.needs_tlsld.load(Ordering::Relaxed) {
        crate::output_chunks::got::got::add_tlsld(ctx);
    }

    // Every dynamic symbol gets its auxiliary record. The loop below
    // assigns table entries in order and so runs on one thread; the
    // records are allocated beforehand in the side arena.
    {
        let mut ids = syms.clone();
        ids.par_sort_unstable();
        ids.dedup();
        ctx.symbols.allocate_aux(&ids);
    }

    // Assign offsets in additional tables for each dynamic symbol.
    for id in syms {
        let flags = ctx.symbols[id].flags();
        let (is_imported, is_exported, ty) = {
            let sym = &ctx.symbols[id];
            (sym.is_imported(), sym.is_exported(), sym.ty())
        };

        if is_imported || is_exported {
            symtab::dynsym::add_symbol(ctx, id);
        }
        if flags & NEEDS_GOT != 0 {
            crate::output_chunks::got::got::add_got_symbol(ctx, id);
        }
        if flags & NEEDS_CANONICAL != 0 && ty == STT_FUNC {
            let sym = &mut ctx.symbols[id];
            sym.set_canonical(true);

            // A canonical PLT needs to be visible from DSOs.
            sym.set_exported(true);

            // We can't use .plt.got for a canonical PLT because otherwise
            // .plt.got and .got would refer to each other, resulting in an
            // infinite loop at runtime.
            crate::output_chunks::got::plt::add_symbol(ctx, id);
        } else if flags & NEEDS_PLT != 0 {
            if flags & NEEDS_GOT != 0 {
                crate::output_chunks::got::pltgot::add_symbol(ctx, id);
            } else {
                crate::output_chunks::got::plt::add_symbol(ctx, id);
            }
        }
        if flags & NEEDS_GOTTP != 0 {
            crate::output_chunks::got::got::add_gottp_symbol(ctx, id);
        }
        if flags & NEEDS_TLSGD != 0 {
            crate::output_chunks::got::got::add_tlsgd_symbol(ctx, id);
        }
        if flags & NEEDS_TLSDESC != 0 {
            crate::output_chunks::got::got::add_tlsdesc_symbol(ctx, id);
        }
        if flags & NEEDS_CANONICAL != 0 && ty != STT_FUNC {
            let relro = ctx.args.z_relro
                && match ctx.symbols[id].file() {
                    Some(FileId::Dso(dso)) => ctx.dsos[dso.index()].is_readonly(&ctx.symbols[id]),
                    _ => false,
                };
            misc::copyrel::add_symbol(ctx, relro, id);
        }
        if E::FAMILY == Family::Ppc64V1 && flags & NEEDS_PPC_OPD != 0 {
            crate::output_chunks::opd::add_symbol(ctx, id);
        }
        ctx.symbols[id].clear_flags();
    }

    if ctx.has_textrel.load(Ordering::Relaxed) && ctx.args.warn_textrel {
        warn!(ctx, "creating a DT_TEXTREL in an output file");
    }
}

// Compute the is_weak bit for each imported symbol.
//
// If all references to a shared symbol is weak, the symbol is marked
// as weak in .dynsym.
pub fn compute_imported_symbol_weakness<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("compute_imported_symbol_weakness");
    let strong: Vec<SymbolId> = ctx
        .objs
        .par_iter()
        .flat_map_iter(|file| {
            (file.base.first_global..file.base.elf_syms.len()).filter_map(|i| {
                let esym = &file.base.elf_syms[i];
                let id = file.base.symbols[i];
                (esym.is_undef()
                    && !esym.is_weak()
                    && matches!(ctx.symbols[id].file(), Some(FileId::Dso(_))))
                .then_some(id)
            })
        })
        .collect();
    for id in strong {
        ctx.symbols[id].set_weak(false);
    }
}

// Report all undefined symbols, grouped by symbol.
pub fn report_undef_errors<E: Arch>(ctx: &Context<E>) {
    const MAX_ERRORS: usize = 3;
    if ctx.args.unresolved_symbols == UnresolvedKind::Ignore {
        return;
    }
    let mut errors: Vec<(SymbolId, Vec<String>)> =
        std::mem::take(&mut *ctx.undef_errors.lock().unwrap())
            .into_iter()
            .collect();
    errors.sort_by_key(|e| e.0);

    for (id, messages) in errors {
        let sym = &ctx.symbols[id];
        let mut msg = format!("undefined symbol: {sym}\n");
        for m in messages.iter().take(MAX_ERRORS) {
            msg.push_str(m);
        }
        if messages.len() > MAX_ERRORS {
            msg.push_str(&format!(
                ">>> referenced {} more times\n",
                messages.len() - MAX_ERRORS
            ));
        }
        // Remove the trailing '\n' because Error/Warn adds it automatically
        msg.pop();
        if ctx.args.unresolved_symbols == UnresolvedKind::Error {
            error!(ctx, "{msg}");
        } else {
            warn!(ctx, "{msg}");
        }
    }
    ctx.checkpoint();
}

pub fn create_reloc_sections<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("create_reloc_sections");

    // Create .rela.* sections
    let ids: Vec<OutputSectionId> = ctx
        .chunks
        .iter()
        .filter_map(|c| c.as_output_section())
        .collect();
    let secs: Vec<misc::RelocSection<E>> = {
        let ctx_ref: &Context<E> = ctx;
        ids.par_iter()
            .map(|&id| misc::reloc::new(ctx_ref, id))
            .collect()
    };
    for (id, sec) in ids.into_iter().zip(secs) {
        ctx.reloc_sections.push(sec);
        let idx = ctx.reloc_sections.len() as u32 - 1;
        ctx.output_sections[id.index()].reloc_sec = Some(idx);
        ctx.chunks.push(ChunkId::Reloc(idx));
    }
}

pub fn sort_dynsyms<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("sort_dynsyms");
    if ctx.dynsym.symbols.is_empty() {
        return;
    }
    let mut syms: Vec<SymbolId> = ctx.dynsym.symbols[1..].iter().flatten().copied().collect();

    // In any symtab, local symbols must precede global symbols.
    let (locals, mut globals): (Vec<SymbolId>, Vec<SymbolId>) =
        syms.iter().partition(|&&id| ctx.symbols[id].is_local(ctx));
    let num_locals = locals.len();

    // .gnu.hash imposes more restrictions on the order of the symbols in
    // .dynsym.
    if let Some(gnu_hash) = &mut ctx.gnu_hash {
        let (unexported, mut exported): (Vec<SymbolId>, Vec<SymbolId>) = globals
            .iter()
            .partition(|&&id| !ctx.symbols[id].is_exported());

        // Count the number of exported symbols to compute the size of .gnu.hash.
        let num_exported = exported.len() as u32;
        let num_buckets = num_exported / GnuHashSection::<E>::LOAD_FACTOR + 1;
        let symbols = &mut ctx.symbols;
        // SAFETY: .dynsym contains each symbol at most once, and exported is a
        // subset of it. Every dynamic symbol already has an auxiliary record.
        unsafe {
            symbols.par_for_each_aux_mut(&exported, |_, sym, aux| {
                aux.djb_hash = symtab::djb_hash(sym.name());
            });
        }
        exported.par_sort_unstable_by(|&a, &b| {
            let a = &symbols[a];
            let b = &symbols[b];
            (a.aux(symbols).unwrap().djb_hash % num_buckets, a.name())
                .cmp(&(b.aux(symbols).unwrap().djb_hash % num_buckets, b.name()))
        });
        gnu_hash.num_buckets = num_buckets;
        gnu_hash.num_exported = num_exported;
        globals = unexported.into_iter().chain(exported).collect();
    }

    syms = locals.into_iter().chain(globals).collect();

    // Compute .dynstr size
    ctx.dynsym.dynstr_offset = ctx.dynstr.hdr.shdr.sh_size.get();
    // SAFETY: .dynsym contains each symbol at most once. Every dynamic symbol
    // already has an auxiliary record.
    let size = unsafe {
        ctx.symbols.par_sum_aux_mut(&syms, |i, sym, aux| {
            aux.dynsym_idx = Some(i as u32 + 1);
            sym.name().len() as u64 + 1
        })
    };
    ctx.dynstr
        .hdr
        .shdr
        .sh_size
        .set(ctx.dynstr.hdr.shdr.sh_size.get() + size);
    ctx.dynsym.symbols = std::iter::once(None)
        .chain(syms.into_iter().map(Some))
        .collect();

    // ELF's symbol table sh_info holds the offset of the first global symbol.
    ctx.dynsym.hdr.shdr.sh_info.set(num_locals as u32 + 1);
}

pub fn create_output_symtab<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("compute_symtab_size");
    if E::NEEDS_THUNK {
        let mut n = 0;
        for osec in &mut ctx.output_sections {
            for thunk in &mut osec.thunks {
                thunk.name = format!("thunk{n}");
                n += 1;
            }
        }
    }
    for id in ctx.chunks.clone() {
        output_chunks::compute_symtab_size(ctx, id);
    }

    let obj_plans: Vec<crate::input_files::SymtabPlan> = {
        let ctx_ref: &Context<E> = ctx;
        ctx_ref
            .objs
            .par_iter()
            .map(|f| f.plan_symtab(ctx_ref, f.id()))
            .collect()
    };
    let dso_plans: Vec<crate::input_files::SymtabPlan> = {
        let ctx_ref: &Context<E> = ctx;
        ctx_ref
            .dsos
            .par_iter()
            .map(|f| f.plan_symtab(ctx_ref, f.id()))
            .collect()
    };
    for (file, plan) in ctx.objs.iter_mut().zip(obj_plans) {
        file.base.apply_symtab_plan(plan);
    }
    for (file, plan) in ctx.dsos.iter_mut().zip(dso_plans) {
        file.base.apply_symtab_plan(plan);
    }
}

pub fn apply_version_script<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("apply_version_script");

    // Assign versions to symbols specified with `extern "C++"` or
    // wildcard patterns first.
    let mut matcher = Glob::new();
    let mut cpp_matcher = Glob::new();

    // The "local:" label has a special meaning in the version script.
    // It can appear in any VERSION clause, and it hides matched symbols
    // unless other non-local patterns match to them. In other words,
    // "local:" has lower precedence than other version definitions.
    //
    // If two or more non-local patterns match to the same symbol, the
    // last one takes precedence.
    let mut patterns: Vec<VersionPattern> = ctx.version_patterns.clone();
    let (local, other): (Vec<VersionPattern>, Vec<VersionPattern>) = patterns
        .drain(..)
        .partition(|p| p.ver_idx as u32 == VER_NDX_LOCAL);
    patterns = local.into_iter().chain(other).collect();

    let has_wildcard = |s: &[u8]| s.iter().any(|&c| matches!(c, b'*' | b'?' | b'['));
    for (i, v) in patterns.iter().enumerate() {
        if v.is_cpp {
            if !cpp_matcher.add(v.pattern, i as i64) {
                fatal!(
                    ctx,
                    "invalid version pattern: {}",
                    crate::util::display(v.pattern)
                );
            }
        } else if has_wildcard(v.pattern) && !matcher.add(v.pattern, i as i64) {
            fatal!(
                ctx,
                "invalid version pattern: {}",
                crate::util::display(v.pattern)
            );
        }
    }

    if !matcher.is_empty() || !cpp_matcher.is_empty() {
        ctx.symbols.par_for_each_global_mut(|sym| {
            if !matches!(sym.file(), Some(FileId::Obj(_))) {
                return;
            }

            let mut m = matcher.find(sym.name());

            // Match non-mangled symbols against the C++ pattern as well.
            // Weird, but required to match other linkers' behavior.
            if !cpp_matcher.is_empty() {
                let demangled = crate::util::demangle::demangle_cpp(sym.name());
                let name: &[u8] = demangled.as_deref().map_or(sym.name(), str::as_bytes);
                m = m.max(cpp_matcher.find(name));
            }

            if m != -1 {
                sym.ver_idx = patterns[m as usize].ver_idx;
            }
        });
    }

    // Next, assign versions to symbols specified by exact name.
    // In other words, exact matches have higher precedence over
    // wildcard or `extern "C++"` patterns.
    for v in &patterns {
        if !v.is_cpp && !has_wildcard(v.pattern) {
            let id = ctx.get_symbol(v.pattern);
            let sym = &ctx.symbols[id];
            if sym.file().is_none() && !ctx.args.undefined_version {
                warn!(
                    ctx,
                    "{}: cannot assign version `{}` to symbol `{sym}`: symbol not found",
                    v.source,
                    crate::util::display(v.ver_str)
                );
            }
            if matches!(sym.file(), Some(FileId::Obj(_))) {
                ctx.symbols[id].ver_idx = v.ver_idx;
            }
        }
    }
}

pub fn parse_symbol_version<E: Arch>(ctx: &mut Context<E>) {
    if !ctx.args.shared {
        return;
    }
    let _t = ctx.timer("parse_symbol_version");

    let verdefs: HashMap<Vec<u8>, u16> = ctx
        .args
        .version_definitions
        .iter()
        .enumerate()
        .map(|(i, v)| {
            (
                v.clone().into_bytes(),
                i as u16 + VER_NDX_LAST_RESERVED as u16 + 1,
            )
        })
        .collect();

    let obj_ids: Vec<ObjId> = ctx.objs.iter().map(ObjectFile::id).collect();
    for obj_id in obj_ids {
        if ctx.is_internal(obj_id) {
            continue;
        }
        let file_id = FileId::Obj(obj_id);
        for i in
            ctx.objs[obj_id.index()].base.first_global..ctx.objs[obj_id.index()].base.elf_syms.len()
        {
            let file = &ctx.objs[obj_id.index()];

            // Match VERSION part of symbol foo@VERSION with version definitions.
            if !file.has_symver[i - file.base.first_global] {
                continue;
            }
            let id = file.base.symbols[i];
            if ctx.symbols[id].file() != Some(file_id) {
                continue;
            }
            let name = file.base.symbol_name_in(i);
            let at = crate::util::find_byte(b'@', name).unwrap();
            let mut ver = &name[at + 1..];
            let mut is_default = false;
            if let Some(rest) = ver.strip_prefix(b"@") {
                is_default = true;
                ver = rest;
            }

            // Empty version (`foo@@`) is the unversioned default; export it
            // globally, overriding any `local: *` from apply_version_script().
            if ver.is_empty() {
                ctx.symbols[id].ver_idx = VER_NDX_GLOBAL as u16;
                continue;
            }
            let Some(&ver_idx) = verdefs.get(ver) else {
                error!(
                    ctx,
                    "{}: symbol {} has undefined version {}",
                    ctx.objs[obj_id.index()],
                    ctx.symbols[id],
                    crate::util::display(ver)
                );
                continue;
            };
            let ver_idx = if is_default {
                ver_idx
            } else {
                ver_idx | VERSYM_HIDDEN as u16
            };
            ctx.symbols[id].ver_idx = ver_idx;

            // If both symbol `foo` and `foo@VERSION` are defined, `foo@VERSION`
            // hides `foo` so that all references to `foo` are resolved to a
            // versioned symbol. Likewise, if `foo@VERSION` and `foo@@VERSION` are
            // defined, the default one takes precedence.
            let sym_name = ctx.symbols[id].name();
            if let Some(id2) = ctx.symbols.lookup(sym_name) {
                if id2 != id && ctx.symbols[id2].file() == Some(file_id) {
                    let sym2_idx = ctx.symbols[id2].sym_idx as usize;
                    let file = &ctx.objs[obj_id.index()];
                    if !file.has_symver[sym2_idx - file.base.first_global] {
                        let v2 = ctx.symbols[id2].ver_idx as u32;
                        if v2 == ctx.default_version as u32
                            || (v2 & !VERSYM_HIDDEN) == (ver_idx as u32 & !VERSYM_HIDDEN)
                        {
                            ctx.symbols[id2].ver_idx = VER_NDX_LOCAL as u16;
                        }
                    }
                }
            }
        }
    }
}

fn should_export<E: Arch>(ctx: &Context<E>, sym: &Symbol) -> bool {
    if sym.visibility() == STV_HIDDEN {
        return false;
    }
    match sym.ver_idx as u32 {
        VER_NDX_UNSPECIFIED => {
            if ctx.args.dynamic_list_data {
                let ty = sym.ty();
                if ty != STT_FUNC && ty != STT_GNU_IFUNC {
                    return true;
                }
            }
            if ctx.args.shared {
                return match sym.file() {
                    Some(FileId::Obj(id)) => !ctx.objs[id.index()].exclude_libs,
                    _ => true,
                };
            }
            ctx.args.export_dynamic
        }
        VER_NDX_LOCAL => false,
        _ => true,
    }
}

fn is_protected<E: Arch>(ctx: &Context<E>, sym: &Symbol) -> bool {
    if sym.visibility() == STV_PROTECTED {
        return true;
    }
    match ctx.args.bsymbolic {
        BsymbolicKind::All => true,
        BsymbolicKind::None => false,
        BsymbolicKind::Functions => sym.ty() == STT_FUNC,
        BsymbolicKind::NonWeak => !sym.is_weak(),
        BsymbolicKind::NonWeakFunctions => !sym.is_weak() && sym.ty() == STT_FUNC,
    }
}

pub fn compute_import_export<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("compute_import_export");

    // If we are creating an executable, we want to export symbols referenced
    // by DSOs unless they are explicitly marked as local by a version script.
    let mut exports: Vec<SymbolId> = Vec::new();
    if !ctx.args.shared {
        let symbols = &ctx.symbols;
        exports = ctx
            .dsos
            .par_iter()
            .flat_map_iter(|file| {
                file.base.symbols.iter().copied().filter(|&id| {
                    let sym = &symbols[id];
                    matches!(sym.file(), Some(FileId::Obj(_)))
                        && sym.visibility() != STV_HIDDEN
                        && sym.ver_idx as u32 != VER_NDX_LOCAL
                })
            })
            .collect();
    }
    for id in exports {
        ctx.symbols[id].set_exported(true);
    }

    // Export symbols that are not hidden or marked as local.
    // We also want to mark imported symbols as such.
    let updates: Vec<(SymbolId, bool, bool)> = {
        let ctx_ref: &Context<E> = ctx;
        ctx_ref
            .objs
            .par_iter()
            .flat_map_iter(|file| {
                let file_id = FileId::Obj(file.id());
                file.base
                    .global_symbols()
                    .iter()
                    .copied()
                    .filter_map(move |id| {
                        let sym = &ctx_ref.symbols[id];

                        // If we are using a symbol in a DSO, we need to import it.
                        if let Some(FileId::Dso(_)) = sym.file() {
                            return Some((id, true, false));
                        }

                        // If we have a definition of a symbol, we may want to export it.
                        if sym.file() == Some(file_id) && should_export(ctx_ref, sym) {
                            // Exported symbols are marked as imported as well by default
                            // for DSOs.
                            let imported = ctx_ref.args.shared && !is_protected(ctx_ref, sym);
                            return Some((id, imported, true));
                        }
                        None
                    })
            })
            .collect()
    };
    for (id, imported, exported) in updates {
        let sym = &mut ctx.symbols[id];
        if imported {
            sym.set_imported(true);
        }
        if exported {
            sym.set_exported(true);
        }
    }

    // Apply --dynamic-list, --export-dynamic-symbol and
    // --export-dynamic-symbol-list options.
    //
    // The semantics of these options vary depending on whether we are
    // creating an executalbe or a shared object.
    //
    // For executable, matched symbols are exported.
    //
    // For shared objects, matched symbols are imported if it is already
    // exported so that they are interposable. In other words, symbols
    // that did not match will be bound locally within the output file,
    // effectively turning them into protected symbols.
    let handle_match = |ctx: &mut Context<E>, id: SymbolId| {
        let shared = ctx.args.shared;
        let sym = &mut ctx.symbols[id];
        if shared {
            if sym.is_exported() {
                sym.set_imported(true);
            }
        } else if matches!(sym.file(), Some(FileId::Obj(_))) && sym.visibility() != STV_HIDDEN {
            sym.set_exported(true);
        }
    };

    let mut matcher = Glob::new();
    let mut cpp_matcher = Glob::new();
    for p in ctx.dynamic_list_patterns.clone() {
        if p.is_cpp {
            if !cpp_matcher.add(p.pattern, 1) {
                fatal!(
                    ctx,
                    "{}: invalid dynamic list entry: {}",
                    p.source,
                    crate::util::display(p.pattern)
                );
            }
            continue;
        }
        if p.pattern.iter().any(|&c| matches!(c, b'*' | b'?' | b'[')) {
            if !matcher.add(p.pattern, 1) {
                fatal!(
                    ctx,
                    "{}: invalid dynamic list entry: {}",
                    p.source,
                    crate::util::display(p.pattern)
                );
            }
            continue;
        }
        let id = ctx.get_symbol(p.pattern);
        handle_match(ctx, id);
    }

    if !matcher.is_empty() || !cpp_matcher.is_empty() {
        let shared = ctx.args.shared;
        let symbols = &ctx.symbols;
        let matched: Vec<SymbolId> = symbols
            .global_ids()
            .collect::<Vec<_>>()
            .par_iter()
            .copied()
            .filter(|&id| {
                let sym = &symbols[id];
                if !matches!(sym.file(), Some(FileId::Obj(_))) || (shared && !sym.is_exported()) {
                    return false;
                }
                if matcher.find(sym.name()) != -1 {
                    return true;
                }
                if !cpp_matcher.is_empty() {
                    let demangled = crate::util::demangle::demangle_cpp(sym.name());
                    let name: &[u8] = demangled.as_deref().map_or(sym.name(), str::as_bytes);
                    return cpp_matcher.find(name) != -1;
                }
                false
            })
            .collect();
        for id in matched {
            handle_match(ctx, id);
        }
    }
}

// Compute the "address-taken" bit for each input section.
//
// As a space-saving optimization, we want to merge two read-only objects
// into a single object if their contents are equivalent. That
// optimization is called the Identical Code Folding or ICF.
//
// A catch is that comparing object contents is not enough to determine if
// two objects can be merged safely; we need to take care of pointer
// equivalence.
//
// In C/C++, two pointers are equivalent if and only if they are taken for
// the same object. Merging two objects into a single object can break
// this assumption because two distinctive pointers would become
// equivalent as a result of merging. We can still merge one object with
// another if no pointer to the object was taken in code, because without
// a pointer, comparing its address becomes moot.
//
// In mold, each input section has an "address-taken" bit. If there is a
// pointer-taking reference to the object, it's set to true. At the ICF
// stage, we merge only objects whose addresses were not taken.
//
// For functions, address-taking relocations are separated from
// non-address-taking ones. For example, x86-64 uses R_X86_64_PLT32 for
// direct function calls (e.g., "call foo" to call the function foo) while
// R_X86_64_PC32 or R_X86_64_GOT32 are used for pointer-taking operations.
//
// Unfortunately, for data, we can't distinguish between address-taking
// relocations and non-address-taking ones. LLVM generates an "address
// significance" table in the ".llvm_addrsig" section to mark symbols
// whose addresses are taken in code. If that table is available, we use
// that information in this function. Otherwise, we conservatively assume
// that all data items are address-taken.
pub fn compute_address_significance<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("compute_address_significance");
    let ctx_ref: &Context<E> = ctx;

    ctx_ref.objs.par_iter().for_each(|file| {
        // If .llvm_addrsig is available, use it.
        if let Some(sec) = &file.llvm_addrsig {
            let mut p = sec.contents();
            while !p.is_empty() {
                let idx = crate::util::read_uleb(&mut p) as usize;
                let sym = &ctx_ref.symbols[file.base.symbols[idx]];
                if let Some(r) = sym.input_section() {
                    ctx_ref.section(r).set_address_taken();
                }
            }
            return;
        }

        // Otherwise, infer address significance.
        for isec in file.input_sections() {
            if !isec.is_alive() || !isec.is_alloc() {
                continue;
            }
            if isec.sh_flags & SHF_EXECINSTR as u64 == 0 {
                isec.set_address_taken();
            }
            for r in isec.rels::<E>(file) {
                if !r.is_func_call::<E>() {
                    let sym = &ctx_ref.symbols[file.base.symbols[r.r_sym() as usize]];
                    if let Some(dst) = sym.input_section_ref() {
                        if dst.sh_flags & SHF_EXECINSTR as u64 != 0 {
                            dst.set_address_taken();
                        }
                    }
                }
            }
        }
    });

    let mark = |id: SymbolId| {
        if let Some(r) = ctx_ref.symbols[id].input_section() {
            ctx_ref.section(r).set_address_taken();
        }
    };
    // Some symbols' pointer values are leaked to the dynamic section.
    mark(ctx_ref.syms.entry);
    mark(ctx_ref.syms.init);
    mark(ctx_ref.syms.fini);
    // Exported symbols are conservatively considered address-taken.
    for &id in ctx_ref.dynsym.symbols.iter().flatten() {
        if ctx_ref.symbols[id].is_exported() {
            mark(id);
        }
    }
}

// We want to sort output chunks in the following order.
//
//   <ELF header>
//   <program header>
//   .interp
//   .note
//   .hash
//   .gnu.hash
//   .dynsym
//   .dynstr
//   .gnu.version
//   .gnu.version_r
//   .rela.dyn
//   .rela.plt
//   <readonly data>
//   <code>
//   <tdata>
//   <tbss>
//   <writable relro data>
//   .got
//   .toc
//   <writable relro bss>
//   .relro_padding
//   <writable non-relro data>
//   <writable non-relro bss>
//   <non-memory-allocated sections>
//   <section header>
//   .gdb_index
//
// .interp and some other linker-synthesized sections are placed at the
// beginning of a file because they are needed by loader. Especially on
// a hard drive with spinning disks, it is important to read these
// sections in a single seek.
//
// .note sections are also placed at the beginning so that they are
// included in a core crash dump even if it's truncated by ulimit. In
// particular, if .note.gnu.build-id is in a truncated core file, you
// can at least identify which executable has crashed.
//
// .gdb_index cannot be constructed before applying relocations to
// other debug sections, so we create it after completing other part
// of the output file and append it to the very end of the file.
//
// A PT_NOTE segment will contain multiple .note sections if exist,
// but there's no way to represent a gap between .note sections.
// Therefore, we sort .note sections by decreasing alignment
// requirement. I believe each .note section size is a multiple of its
// alignment, so by sorting them by alignment, we should be able to
// avoid a gap between .note sections.
//
// .toc is placed right after .got for PPC64. PPC-specific .toc section
// contains data that may be accessed with a 16-bit offset relative to
// %r2. %r2 is set to .got + 32 KiB. Therefore, .toc needs to be within
// [.got, .got + 64 KiB).
//
// Other file layouts are possible, but this layout is chosen to keep
// the number of segments as few as possible.
fn sort_output_sections_regular<E: Arch>(ctx: &mut Context<E>) {
    let rank1 = |ctx: &Context<E>, id: ChunkId| -> i64 {
        let hdr = ctx.chunk_header(id);
        let ty = hdr.shdr.sh_type.get();
        let flags = hdr.shdr.sh_flags.get();
        match id {
            ChunkId::Ehdr => return 0,
            ChunkId::Phdr => return 1,
            ChunkId::Interp => return 2,
            _ if ty == SHT_NOTE && flags & SHF_ALLOC as u64 != 0 => return 3,
            ChunkId::Hash => return 4,
            ChunkId::GnuHash => return 5,
            ChunkId::Dynsym => return 6,
            ChunkId::Dynstr => return 7,
            ChunkId::Versym => return 8,
            ChunkId::Verneed => return 9,
            ChunkId::RelDyn => return 10,
            ChunkId::RelPlt => return 11,
            ChunkId::Shdr => return i32::MAX as i64 - 1,
            ChunkId::GdbIndex => return i32::MAX as i64,
            _ => {}
        }
        let alloc = flags & SHF_ALLOC as u64 != 0;
        let writable = flags & SHF_WRITE as u64 != 0;
        let exec = flags & SHF_EXECINSTR as u64 != 0;
        let tls = flags & SHF_TLS as u64 != 0;
        let relro = hdr.is_relro;
        let is_bss = ty == SHT_NOBITS;
        (1 << 10)
            | ((!alloc as i64) << 9)
            | ((writable as i64) << 8)
            | ((exec as i64) << 7)
            | ((!tls as i64) << 6)
            | ((!relro as i64) << 5)
            | ((is_bss as i64) << 4)
    };
    // Ties are broken by additional rules
    let rank2 = |ctx: &Context<E>, id: ChunkId| -> i64 {
        let hdr = ctx.chunk_header(id);
        if hdr.shdr.sh_type.get() == SHT_NOTE {
            return -(hdr.shdr.sh_addralign.get() as i64);
        }
        match id {
            ChunkId::Got => 2,
            _ if hdr.name == b".toc" => 3,
            ChunkId::RelroPadding => i64::MAX,
            _ => 0,
        }
    };
    let mut chunks = std::mem::take(&mut ctx.chunks);
    chunks.sort_by_cached_key(|&id| {
        (
            rank1(ctx, id),
            rank2(ctx, id),
            ctx.chunk_header(id).name.to_vec(),
        )
    });
    ctx.chunks = chunks;
}

fn section_order_group<E: Arch>(ctx: &Context<E>, id: ChunkId) -> &'static str {
    let hdr = ctx.chunk_header(id);
    if hdr.shdr.sh_type.get() == SHT_NOBITS {
        "BSS"
    } else if hdr.shdr.sh_flags.get() & SHF_EXECINSTR as u64 != 0 {
        "TEXT"
    } else if hdr.shdr.sh_flags.get() & SHF_WRITE as u64 != 0 {
        "DATA"
    } else {
        "RODATA"
    }
}

// Sort sections according to a --section-order argument.
fn sort_output_sections_by_order<E: Arch>(ctx: &mut Context<E>) {
    let rank =
        |ctx: &Context<E>, id: ChunkId| -> i64 {
            let hdr = ctx.chunk_header(id);
            let flags = hdr.shdr.sh_flags.get();
            match id {
                ChunkId::Ehdr if flags & SHF_ALLOC as u64 == 0 => return -2,
                ChunkId::Phdr if flags & SHF_ALLOC as u64 == 0 => return -1,
                ChunkId::Shdr => return i32::MAX as i64,
                _ if flags & SHF_ALLOC as u64 == 0 => return i32::MAX as i64 - 1,
                _ => {}
            }
            let name = hdr.name;
            if let Some(i) = ctx
                .args
                .section_order
                .iter()
                .position(|o| o.kind == SectionOrderKind::Section && o.name.as_bytes() == name)
            {
                return i as i64;
            }
            let group = section_order_group(ctx, id);
            if let Some(i) = ctx.args.section_order.iter().position(|o| {
                o.kind == SectionOrderKind::Group && o.name.eq_ignore_ascii_case(group)
            }) {
                return i as i64;
            }
            error!(
                ctx,
                "--section-order: missing section specification for {}", hdr.name
            );
            0
        };
    // It is an error if a section order cannot be determined by a given
    // section order list.
    for id in ctx.chunks.clone() {
        let r = rank(ctx, id);
        ctx.chunk_header_mut(id).sect_order = r;
    }
    // Sort output sections by --section-order
    let mut chunks = std::mem::take(&mut ctx.chunks);
    chunks.sort_by_key(|&id| ctx.chunk_header(id).sect_order);
    ctx.chunks = chunks;
}

pub fn sort_output_sections<E: Arch>(ctx: &mut Context<E>) {
    if ctx.args.section_order.is_empty() {
        sort_output_sections_regular(ctx);
    } else {
        sort_output_sections_by_order(ctx);
    }
}

fn tls_segment_alignment<E: Arch>(ctx: &Context<E>) -> u64 {
    ctx.chunks
        .iter()
        .map(|&id| ctx.chunk_header(id))
        .filter(|h| h.shdr.sh_flags.get() & SHF_TLS as u64 != 0)
        .map(|h| h.shdr.sh_addralign.get())
        .max()
        .unwrap_or(1)
        .max(1)
}

// This function assigns virtual addresses to output sections. Assigning
// addresses is a bit tricky because we want to pack sections as tightly
// as possible while not violating the constraints imposed by the hardware
// and the OS kernel. Specifically, we need to satisfy the following
// constraints:
//
// - Memory protection (readable, writable and executable) works at page
//   granularity. Therefore, if we want to set different memory attributes
//   to two sections, we need to place them into separate pages.
//
// - The ELF spec requires that a section's file offset is congruent to
//   its virtual address modulo the page size. For example, a section at
//   virtual address 0x401234 on x86-64 (4 KiB, or 0x1000 byte page
//   system) can be at file offset 0x3234 or 0x50234 but not at 0x1000.
//
// We need to insert paddings between sections if we can't satisfy the
// above constraints without them.
//
// We don't want to waste too much memory and disk space for paddings.
// There are a few tricks we can use to minimize paddings as below:
//
// - We want to place sections with the same memory attributes
//   contiguous as possible.
//
// - We can map the same file region to memory more than once. For
//   example, we can write code (with R and X bits) and read-only data
//   (with R bit) adjacent on file and map it twice as the last page of
//   the executable segment and the first page of the read-only data
//   segment. This doesn't save memory but saves disk space.
fn set_virtual_addresses_regular<E: Arch>(ctx: &mut Context<E>) {
    const RELRO: u64 = 1 << 32;
    let flags_of = |ctx: &Context<E>, id: ChunkId| -> u64 {
        let flags = output_chunks::to_phdr_flags(ctx, id) as u64;
        if ctx.args.z_relro && ctx.chunk_header(id).is_relro {
            flags | RELRO
        } else {
            flags
        }
    };
    let is_tls = |ctx: &Context<E>, id: ChunkId| {
        ctx.chunk_header(id).shdr.sh_flags.get() & SHF_TLS as u64 != 0
    };
    let is_tbss = |ctx: &Context<E>, id: ChunkId| {
        let shdr = &ctx.chunk_header(id).shdr;
        shdr.sh_flags.get() & SHF_TLS as u64 != 0 && shdr.sh_type.get() == SHT_NOBITS
    };

    // Assign virtual addresses
    let chunks = ctx.chunks.clone();
    let mut addr = ctx.args.image_base;
    let page_size = ctx.page_size;
    let mut i = 0;
    while i < chunks.len() {
        let id = chunks[i];
        if !ctx.chunk_header(id).is_alloc() {
            i += 1;
            continue;
        }

        // .relro_padding is a padding section to extend a PT_GNU_RELRO
        // segment to cover an entire page. Technically, we don't need a
        // .relro_padding section because we can leave a trailing part of a
        // segment an unused space. However, the `strip` command would delete
        // such an unused trailing part and make an executable invalid.
        // So we add a dummy section.
        if id == ChunkId::RelroPadding {
            let hdr = ctx.chunk_header_mut(id);
            hdr.shdr.sh_addr.set(addr);
            hdr.shdr.sh_size.set(align_to(addr, page_size) - addr);
            addr += page_size;
            i += 1;
            continue;
        }

        // Handle --section-start first
        let name = String::from_utf8_lossy(ctx.chunk_header(id).name).into_owned();
        if let Some(&start) = ctx.args.section_start.get(&name) {
            addr = start;
            let hdr = ctx.chunk_header_mut(id);
            hdr.shdr.sh_addr.set(addr);
            addr += hdr.shdr.sh_size.get();
            i += 1;
            continue;
        }

        // Memory protection works at page size granularity. We need to
        // put sections with different memory attributes into different
        // pages. We do it by inserting paddings here.
        if i > 0 && chunks[i - 1] != ChunkId::RelroPadding {
            let flags1 = flags_of(ctx, chunks[i - 1]);
            let flags2 = flags_of(ctx, id);
            if !ctx.args.nmagic && flags1 != flags2 {
                match ctx.args.z_separate_code {
                    SeparateCodeKind::SeparateLoadableSegments => addr = align_to(addr, page_size),
                    SeparateCodeKind::SeparateCode
                        if (flags1 & PF_X as u64) != (flags2 & PF_X as u64) =>
                    {
                        addr = align_to(addr, page_size)
                    }
                    _ => {
                        if !addr.is_multiple_of(page_size) {
                            addr += page_size;
                        }
                    }
                }
            }
        }

        // TLS sections are included only in PT_LOAD but also in PT_TLS.
        // We align the first TLS section so that the PT_TLS segment starts
        // at an address that meets the segment's alignment requirement.
        if is_tls(ctx, id) && (i == 0 || !is_tls(ctx, chunks[i - 1])) {
            addr = align_to(addr, tls_segment_alignment(ctx));
        }

        // TLS BSS sections are laid out so that they overlap with the
        // subsequent non-tbss sections. Overlapping is fine because a STT_TLS
        // segment contains an initialization image for newly-created threads,
        // and no one except the runtime reads its contents. Even the runtime
        // doesn't need a BSS part of a TLS initialization image; it just
        // leaves zero-initialized bytes as-is instead of copying zeros.
        // So no one really read tbss at runtime.
        //
        // We can instead allocate a dedicated virtual address space to tbss,
        // but that would be just a waste of the address and disk space.
        if is_tbss(ctx, id) {
            let mut addr2 = addr;
            loop {
                let hdr = ctx.chunk_header_mut(chunks[i]);
                addr2 = align_to(addr2, hdr.shdr.sh_addralign.get());
                hdr.shdr.sh_addr.set(addr2);
                addr2 += hdr.shdr.sh_size.get();
                if i + 2 == chunks.len() || !is_tbss(ctx, chunks[i + 1]) {
                    break;
                }
                i += 1;
            }
            i += 1;
            continue;
        }

        let hdr = ctx.chunk_header_mut(id);
        addr = align_to(addr, hdr.shdr.sh_addralign.get());
        hdr.shdr.sh_addr.set(addr);
        addr += hdr.shdr.sh_size.get();
        i += 1;
    }
}

fn set_virtual_addresses_by_order<E: Arch>(ctx: &mut Context<E>) {
    let vec: Vec<ChunkId> = ctx
        .chunks
        .iter()
        .copied()
        .filter(|&c| ctx.chunk_header(c).is_alloc())
        .collect();
    let mut addr = ctx.args.image_base;
    let page_size = ctx.page_size;
    let mut i = 0;

    for (j, ord) in ctx.args.section_order.clone().iter().enumerate() {
        match ord.kind {
            SectionOrderKind::Section | SectionOrderKind::Group => {
                while i < vec.len() && ctx.chunk_header(vec[i]).sect_order == j as i64 {
                    // Memory protection works on page size granularity. We need to
                    // put sections with different memory attributes into different
                    // pages. We do it by inserting a padding.
                    if i != 0 {
                        let flags1 = output_chunks::to_phdr_flags(ctx, vec[i - 1]);
                        let flags2 = output_chunks::to_phdr_flags(ctx, vec[i]);
                        if flags1 != flags2 {
                            match ctx.args.z_separate_code {
                                SeparateCodeKind::SeparateLoadableSegments => {
                                    addr = align_to(addr, page_size)
                                }
                                SeparateCodeKind::SeparateCode
                                    if (flags1 & PF_X) != (flags2 & PF_X) =>
                                {
                                    addr = align_to(addr, page_size)
                                }
                                _ => {}
                            }
                        }
                    }
                    let hdr = ctx.chunk_header_mut(vec[i]);
                    addr = align_to(addr, hdr.shdr.sh_addralign.get());
                    hdr.shdr.sh_addr.set(addr);
                    addr += hdr.shdr.sh_size.get();
                    i += 1;
                }
            }
            SectionOrderKind::Addr => {
                if addr != ctx.args.image_base && ord.value < addr {
                    error!(
                        ctx,
                        "--section-order: address goes backward: requested {:#x} < current {addr:#x} (at token '{}')",
                        ord.value,
                        ord.token
                    );
                }
                addr = ord.value;
            }
            SectionOrderKind::Align => addr = align_to(addr, ord.value),
            SectionOrderKind::Symbol => {
                let id = ctx.get_symbol(ord.name.as_bytes());
                ctx.symbols[id].value = addr;
            }
        }
    }
}

// Returns the smallest integer N that satisfies N >= val and
// N % align == skew % align.
//
// Section's file offset must be congruent to its virtual address modulo
// the page size. We use this function to satisfy that requirement.
fn align_with_skew(val: u64, align: u64, skew: u64) -> u64 {
    val + (skew.wrapping_sub(val) & (align - 1))
}

// Assign file offsets to output sections.
fn set_file_offsets<E: Arch>(ctx: &mut Context<E>) -> u64 {
    let chunks = ctx.chunks.clone();
    let page_size = ctx.page_size;
    let mut fileoff = 0u64;
    let mut i = 0;

    while i < chunks.len() {
        let first = ctx.chunk_header(chunks[i]).shdr;
        if first.sh_flags.get() & SHF_ALLOC as u64 == 0 {
            fileoff = align_to(fileoff, first.sh_addralign.get());
            ctx.chunk_header_mut(chunks[i]).shdr.sh_offset.set(fileoff);
            fileoff += first.sh_size.get();
            i += 1;
            continue;
        }
        if first.sh_type.get() == SHT_NOBITS {
            ctx.chunk_header_mut(chunks[i]).shdr.sh_offset.set(fileoff);
            i += 1;
            continue;
        }

        if first.sh_addralign.get() > page_size {
            fileoff = align_to(fileoff, first.sh_addralign.get());
        } else {
            fileoff = align_with_skew(fileoff, page_size, first.sh_addr.get());
        }

        // Assign ALLOC sections contiguous file offsets as long as they
        // are contiguous in memory.
        loop {
            let shdr = ctx.chunk_header(chunks[i]).shdr;
            ctx.chunk_header_mut(chunks[i])
                .shdr
                .sh_offset
                .set(fileoff + shdr.sh_addr.get() - first.sh_addr.get());
            i += 1;
            if i >= chunks.len() {
                break;
            }
            let next = ctx.chunk_header(chunks[i]).shdr;
            if next.sh_flags.get() & SHF_ALLOC as u64 == 0 || next.sh_type.get() == SHT_NOBITS {
                break;
            }
            // If --start-section is given, addresses may not increase
            // monotonically.
            if next.sh_addr.get() < first.sh_addr.get() {
                break;
            }
            let prev = ctx.chunk_header(chunks[i - 1]).shdr;
            // This section requires larger alignment, we need to adjust the
            // offset to ensure offset % align == vaddr % align.
            if next.sh_addralign.get() > page_size
                && next.sh_addralign.get() > prev.sh_addralign.get()
            {
                break;
            }
            // If --start-section is given, there may be a large gap between
            // sections. We don't want to allocate a disk space for a gap if
            // exists.
            let gap = next.sh_addr.get() - prev.sh_addr.get() - prev.sh_size.get();
            if gap >= page_size {
                break;
            }
        }

        let last = ctx.chunk_header(chunks[i - 1]).shdr;
        fileoff = last.sh_offset.get() + last.sh_size.get();

        while i < chunks.len() {
            let shdr = ctx.chunk_header(chunks[i]).shdr;
            if shdr.sh_flags.get() & SHF_ALLOC as u64 == 0 || shdr.sh_type.get() != SHT_NOBITS {
                break;
            }
            ctx.chunk_header_mut(chunks[i]).shdr.sh_offset.set(fileoff);
            i += 1;
        }
    }
    fileoff
}

// Remove debug sections from ctx.chunks and save them to ctx.debug_chunks.
// This is for --separate-debug-file.
pub fn separate_debug_sections<E: Arch>(ctx: &mut Context<E>) {
    let is_debug = |ctx: &Context<E>, id: ChunkId| {
        let hdr = ctx.chunk_header(id);
        !hdr.is_alloc()
            && (matches!(id, ChunkId::GdbIndex | ChunkId::Symtab | ChunkId::Strtab)
                || hdr.name.starts_with(b".debug_"))
    };
    let (debug, rest): (Vec<ChunkId>, Vec<ChunkId>) =
        ctx.chunks.iter().partition(|&&id| is_debug(ctx, id));
    ctx.chunks = rest;
    ctx.debug_chunks = debug;
}

pub fn compute_section_headers<E: Arch>(ctx: &mut Context<E>) {
    // Update sh_size for each chunk.
    for id in ctx.chunks.clone() {
        output_chunks::update_shdr(ctx, id);
    }

    // Remove empty chunks.
    let chunks = std::mem::take(&mut ctx.chunks);
    ctx.chunks = chunks
        .into_iter()
        .filter(|&id| {
            matches!(
                id,
                ChunkId::Output(_) | ChunkId::GdbIndex | ChunkId::Placeholder(_)
            ) || ctx.chunk_header(id).shdr.sh_size.get() != 0
        })
        .collect();

    // Set section indices.
    let mut shndx = 1u32;
    for id in ctx.chunks.clone() {
        if !id.is_header() {
            ctx.chunk_header_mut(id).shndx = shndx;
            shndx += 1;
        }
    }

    if shndx >= SHN_LORESERVE && ctx.chunks.contains(&ChunkId::Symtab) && ctx.symtab_shndx.is_none()
    {
        let mut sec = SymtabShndxSection::new();
        sec.hdr.shndx = shndx;
        shndx += 1;
        sec.hdr.shdr.sh_link.set(ctx.symtab.hdr.shndx);
        ctx.symtab_shndx = Some(sec);
        ctx.chunks.push(ChunkId::SymtabShndx);
    }

    if let Some(shdr) = &mut ctx.shdr {
        shdr.hdr
            .shdr
            .sh_size
            .set(shndx as u64 * ElfShdr::<E>::size() as u64);
    }

    // Some types of section header refer to other section by index.
    // Recompute all section headers to fill such fields with correct values.
    for id in ctx.chunks.clone() {
        output_chunks::update_shdr(ctx, id);
    }

    if let Some(symtab_shndx) = &mut ctx.symtab_shndx {
        let n = ctx.symtab.hdr.shdr.sh_size.get() / std::mem::size_of::<ElfSym<E>>() as u64;
        symtab_shndx.hdr.shdr.sh_size.set(n * 4);
    }
}

// Assign virtual addresses and file offsets to output sections.
pub fn set_osec_offsets<E: Arch>(ctx: &mut Context<E>) -> u64 {
    let _t = ctx.timer("set_osec_offsets");
    loop {
        if ctx.args.section_order.is_empty() {
            set_virtual_addresses_regular(ctx);
        } else {
            set_virtual_addresses_by_order(ctx);
        }

        if ctx.args.pack_dyn_relocs_android {
            let before = ctx.reldyn.hdr.shdr.sh_size.get();
            crate::output_chunks::dynamic::reldyn::update_shdr(ctx);
            if before != ctx.reldyn.hdr.shdr.sh_size.get() {
                continue;
            }
        }
        ctx.checkpoint();

        // Assigning new offsets may change the contents and the length
        // of the program header, so repeat it until converge.
        let fileoff = set_file_offsets(ctx);
        if ctx.phdr.is_some() {
            let before = ctx.phdr.as_ref().unwrap().hdr.shdr.sh_size.get();
            output_chunks::update_phdr(ctx);
            if before < ctx.phdr.as_ref().unwrap().hdr.shdr.sh_size.get() {
                continue;
            }
        }
        return fileoff;
    }
}

fn num_irelative_relocs<E: Arch>(ctx: &Context<E>) -> u64 {
    let mut n = ctx.num_ifunc_dynrels.load(Ordering::Relaxed) as u64;
    n += ctx
        .got
        .got_syms
        .iter()
        .filter(|&&id| ctx.symbols[id].is_ifunc())
        .count() as u64;
    n
}

fn to_paddr<E: Arch>(ctx: &Context<E>, vaddr: u64) -> u64 {
    if let Some(phdr) = &ctx.phdr {
        for p in &phdr.phdrs {
            if p.p_type().get() == PT_LOAD
                && p.p_vaddr().get() <= vaddr
                && vaddr < p.p_vaddr().get() + p.p_memsz().get()
            {
                return p.p_paddr().get() + (vaddr - p.p_vaddr().get());
            }
        }
    }
    0
}

pub fn fix_synthetic_symbols<E: Arch>(ctx: &mut Context<E>) {
    fn start<E: Arch>(
        ctx: &mut Context<E>,
        sym: Option<SymbolId>,
        chunk: Option<ChunkId>,
        bias: i64,
    ) {
        if let (Some(sym), Some(chunk)) = (sym, chunk) {
            let addr = ctx.chunk_header(chunk).shdr.sh_addr.get();
            let s = &mut ctx.symbols[sym];
            s.set_output_chunk(chunk);
            s.value = addr.wrapping_add(bias as u64);
        }
    }
    fn stop<E: Arch>(
        ctx: &mut Context<E>,
        sym: Option<SymbolId>,
        chunk: Option<ChunkId>,
        bias: i64,
    ) {
        if let (Some(sym), Some(chunk)) = (sym, chunk) {
            let shdr = ctx.chunk_header(chunk).shdr;
            let s = &mut ctx.symbols[sym];
            s.set_output_chunk(chunk);
            s.value = (shdr.sh_addr.get() + shdr.sh_size.get()).wrapping_add(bias as u64);
        }
    }

    let sections: Vec<ChunkId> = ctx
        .chunks
        .iter()
        .copied()
        .filter(|&c| !c.is_header() && ctx.chunk_header(c).is_alloc())
        .collect();
    let find = |ctx: &Context<E>, name: &[u8]| {
        sections
            .iter()
            .copied()
            .find(|&c| ctx.chunk_header(c).name == name)
    };
    let first = sections.first().copied();

    // __bss_start
    if let Some(bss) = find(ctx, b".bss") {
        start(ctx, ctx.syms.bss_start, Some(bss), 0);
    }

    if let Some(ehdr) = &ctx.ehdr {
        if ehdr.hdr.is_alloc() {
            let addr = ehdr.hdr.shdr.sh_addr.get();
            for sym in [ctx.syms.ehdr_start, ctx.syms.executable_start] {
                if let (Some(sym), Some(first)) = (sym, first) {
                    let s = &mut ctx.symbols[sym];
                    s.set_output_chunk(first);
                    s.value = addr;
                }
            }
        }
    }

    if let (Some(sym), Some(first)) = (ctx.syms.dso_handle, first) {
        let addr = ctx.chunk_header(first).shdr.sh_addr.get();
        let s = &mut ctx.symbols[sym];
        s.set_output_chunk(first);
        s.value = addr;
    }

    // __rel_iplt_start and __rel_iplt_end. These symbols need to be
    // defined in a statically-linked non-relocatable executable because
    // such executable lacks the .dynamic section and thus there's no way
    // to find ifunc relocations other than these symbols.
    if ctx.chunks.contains(&ChunkId::RelDyn) && ctx.args.is_static && !ctx.args.pie {
        let n = num_irelative_relocs(ctx) as i64 * std::mem::size_of::<ElfRel<E>>() as i64;
        stop(ctx, ctx.syms.rel_iplt_start, Some(ChunkId::RelDyn), -n);
        stop(ctx, ctx.syms.rel_iplt_end, Some(ChunkId::RelDyn), 0);
    } else {
        // If the symbols are not ncessary, we turn them to absolute
        // symbols at address 0.
        for sym in [ctx.syms.rel_iplt_start, ctx.syms.rel_iplt_end]
            .into_iter()
            .flatten()
        {
            ctx.symbols[sym].clear_origin();
        }
    }

    // __{init,fini}_array_{start,end}
    for &chunk in &sections {
        match ctx.chunk_header(chunk).shdr.sh_type.get() {
            SHT_INIT_ARRAY => {
                start(ctx, ctx.syms.init_array_start, Some(chunk), 0);
                stop(ctx, ctx.syms.init_array_end, Some(chunk), 0);
            }
            SHT_PREINIT_ARRAY => {
                start(ctx, ctx.syms.preinit_array_start, Some(chunk), 0);
                stop(ctx, ctx.syms.preinit_array_end, Some(chunk), 0);
            }
            SHT_FINI_ARRAY => {
                start(ctx, ctx.syms.fini_array_start, Some(chunk), 0);
                stop(ctx, ctx.syms.fini_array_end, Some(chunk), 0);
            }
            _ => {}
        }
    }

    // _end, _etext, _edata and the like
    for &chunk in &sections {
        let shdr = ctx.chunk_header(chunk).shdr;
        if shdr.sh_flags.get() & SHF_ALLOC as u64 != 0 {
            stop(ctx, ctx.syms.end_, Some(chunk), 0);
            stop(ctx, ctx.syms.end, Some(chunk), 0);
        }
        if shdr.sh_flags.get() & SHF_EXECINSTR as u64 != 0 {
            stop(ctx, ctx.syms.etext_, Some(chunk), 0);
            stop(ctx, ctx.syms.etext, Some(chunk), 0);
        }
        if shdr.sh_type.get() != SHT_NOBITS && shdr.sh_flags.get() & SHF_ALLOC as u64 != 0 {
            stop(ctx, ctx.syms.edata_, Some(chunk), 0);
            stop(ctx, ctx.syms.edata, Some(chunk), 0);
        }
    }

    // _DYNAMIC
    let dynamic = ctx.dynamic.as_ref().map(|_| ChunkId::Dynamic);
    start(ctx, ctx.syms.dynamic, dynamic, 0);

    // _GLOBAL_OFFSET_TABLE_. I don't know why, but for the sake of
    // compatibility with existing code, it must be set to the beginning of
    // .got.plt instead of .got only on i386 and x86-64.
    let got = if E::IS_X86 {
        ChunkId::GotPlt
    } else {
        ChunkId::Got
    };
    start(ctx, ctx.syms.global_offset_table, Some(got), 0);

    // _PROCEDURE_LINKAGE_TABLE_. We need this on SPARC.
    start(ctx, ctx.syms.procedure_linkage_table, Some(ChunkId::Plt), 0);

    // _TLS_MODULE_BASE_. This symbol is used to obtain the address of
    // the TLS block in the TLSDESC model. I believe GCC and Clang don't
    // create a reference to it, but Intel compiler seems to be using
    // this symbol.
    if let (Some(sym), Some(first)) = (ctx.syms.tls_module_base, first) {
        let dtp = ctx.dtp_addr;
        let s = &mut ctx.symbols[sym];
        s.set_output_chunk(first);
        s.value = dtp;
    }

    // __GNU_EH_FRAME_HDR
    let eh_frame_hdr = ctx.eh_frame_hdr.as_ref().map(|_| ChunkId::EhFrameHdr);
    start(ctx, ctx.syms.gnu_eh_frame_hdr, eh_frame_hdr, 0);

    // RISC-V's __global_pointer$
    if let Some(sym) = ctx.syms.global_pointer {
        match find(ctx, b".sdata") {
            Some(c) => start(ctx, Some(sym), Some(c), 0x800),
            None => start(ctx, Some(sym), first, 0),
        }
    }
    // ARM32's __exidx_{start,end}
    if ctx.syms.exidx_start.is_some() {
        if let Some(c) = find(ctx, b".ARM.exidx") {
            start(ctx, ctx.syms.exidx_start, Some(c), 0);
            stop(ctx, ctx.syms.exidx_end, Some(c), 0);
        }
    }
    // PPC64's ".TOC." symbol.
    if E::IS_PPC64 {
        if let Some(c) = find(ctx, b".got").or_else(|| find(ctx, b".toc")) {
            start(ctx, ctx.syms.toc, Some(c), 0x8000);
        } else if let (Some(sym), Some(first)) = (ctx.syms.toc, first) {
            let s = &mut ctx.symbols[sym];
            s.set_output_chunk(first);
            s.value = 0;
        }
    }

    // PPC64's _{save,rest}gpr{0,1}_{14,15,16,...,31} symbols
    if E::FAMILY == Family::Ppc64V2 {
        for (i, &(label, _)) in crate::arch::ppc64v2::SAVE_RESTORE_INSNS.iter().enumerate() {
            if label.is_empty() {
                continue;
            }
            let sym = ctx.get_symbol(label.as_bytes());
            if ctx.symbols[sym].file() == ctx.internal_obj.map(FileId::Obj) {
                start(
                    ctx,
                    Some(sym),
                    Some(ChunkId::Ppc64SaveRestore),
                    (i * 4) as i64,
                );
            }
        }
    }

    // __start_ and __stop_ symbols
    for &chunk in &sections {
        if let Some(name) = start_stop_name(ctx, chunk) {
            let s = ctx.get_symbol(format!("__start_{name}").as_bytes());
            start(ctx, Some(s), Some(chunk), 0);
            let e = ctx.get_symbol(format!("__stop_{name}").as_bytes());
            stop(ctx, Some(e), Some(chunk), 0);

            if ctx.args.physical_image_base.is_some() {
                let shdr = ctx.chunk_header(chunk).shdr;
                let paddr = to_paddr(ctx, shdr.sh_addr.get());
                let x = ctx.get_symbol(format!("__phys_start_{name}").as_bytes());
                ctx.symbols[x].set_output_chunk(chunk);
                ctx.symbols[x].value = paddr;
                let y = ctx.get_symbol(format!("__phys_stop_{name}").as_bytes());
                ctx.symbols[y].set_output_chunk(chunk);
                ctx.symbols[y].value = paddr + shdr.sh_size.get();
            }
        }
    }

    // --defsym=sym=value symbols
    for (name, value) in ctx.args.defsyms.clone() {
        let sym = ctx.get_symbol(name.as_bytes());
        match value {
            DefsymValue::Addr(addr) => {
                let s = &mut ctx.symbols[sym];
                s.clear_origin();
                s.value = addr;
            }
            DefsymValue::Symbol(target) => {
                let sym2 = ctx.get_symbol(target.as_bytes());
                let (value, origin, vis) = {
                    let s2 = &ctx.symbols[sym2];
                    (s2.value, s2.origin_state(), s2.visibility())
                };
                let s = &mut ctx.symbols[sym];
                s.value = value;
                s.set_origin_state(origin);
                s.set_visibility(vis);
            }
        }
    }

    // --section-order symbols
    for ord in ctx.args.section_order.clone() {
        if ord.kind == SectionOrderKind::Symbol {
            let sym = ctx.get_symbol(ord.name.as_bytes());
            if let Some(first) = first {
                ctx.symbols[sym].set_output_chunk(first);
            }
        }
    }
}

pub fn compress_debug_sections<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("compress_debug_sections");

    // Since this pass is embarassingly parallel, we want to use all
    // available cores by default.
    let targets: Vec<(usize, ChunkId)> = ctx
        .chunks
        .iter()
        .enumerate()
        .filter(|&(_, &id)| {
            let hdr = ctx.chunk_header(id);
            !hdr.is_alloc() && hdr.shdr.sh_size.get() != 0 && hdr.name.starts_with(b".debug_")
        })
        .map(|(i, &id)| (i, id))
        .collect();
    let compressed: Vec<misc::CompressedSection<E>> = {
        let ctx_ref: &Context<E> = ctx;
        targets
            .par_iter()
            .map(|&(_, id)| misc::compressed::new(ctx_ref, id))
            .collect()
    };
    for ((i, _), sec) in targets.into_iter().zip(compressed) {
        ctx.compressed_sections.push(sec);
        ctx.chunks[i] = ChunkId::Compressed(ctx.compressed_sections.len() as u32 - 1);
    }
}

// BLAKE3 is a cryptographic hash function just like SHA256.
// We use it instead of SHA256 because it's faster.
pub fn write_build_id<E: Arch>(ctx: &mut Context<E>, buf: &mut [u8], is_mmapped: bool) {
    let _t = ctx.timer("write_build_id");
    let contents: Vec<u8> = match ctx.args.build_id.kind {
        BuildIdKind::Hex => ctx.args.build_id.value.clone(),
        BuildIdKind::Hash => {
            const SHARD: usize = 4 * 1024 * 1024; // 4 MiB
            let hashes: Vec<[u8; 32]> = buf
                .par_chunks_mut(SHARD)
                .enumerate()
                .map(|(i, shard)| {
                    let hash = *blake3::hash(shard).as_bytes();
                    // Make the kernel page out the file contents we've just written
                    // so that subsequent close(2) call will become quicker.
                    if i > 0 && is_mmapped {
                        // SAFETY: the shard is part of the output mapping; the
                        // advice only drops the process's mapping of pages
                        // that are backed by the file.
                        unsafe {
                            libc::madvise(
                                shard.as_mut_ptr() as *mut libc::c_void,
                                shard.len(),
                                libc::MADV_DONTNEED,
                            )
                        };
                    }
                    hash
                })
                .collect();
            let digest = *blake3::hash(hashes.as_flattened()).as_bytes();
            digest[..ctx.args.build_id.size()].to_vec()
        }
        BuildIdKind::Uuid => {
            let mut bytes = [0u8; 16];
            crate::util::random_bytes(&mut bytes);
            // Indicate that this is UUIDv4 as defined by RFC4122
            bytes[6] = (bytes[6] & 0x0f) | 0x40;
            bytes[8] = (bytes[8] & 0x3f) | 0x80;
            bytes.to_vec()
        }
        BuildIdKind::None => unreachable!(),
    };
    ctx.buildid.as_mut().unwrap().contents = contents;
    let hdr = ctx.buildid.as_ref().unwrap().hdr.shdr;
    let start = hdr.sh_offset.get() as usize;
    let end = (hdr.sh_offset.get() + hdr.sh_size.get()) as usize;
    misc::build_id::copy_buf(ctx, &mut buf[start..end]);
}

// A .gnu_debuglink section contains a filename and a CRC32 checksum of a
// debug info file. When we are writing a .gnu_debuglink, we don't know
// its CRC32 checksum because we haven't created a debug info file. So we
// write a dummy value instead.
//
// We can't choose a random value as a dummy value for build
// reproducibility. We also don't want to write a fixed value for all
// files because the CRC checksum is in this section to prevent using
// wrong file on debugging. gdb rejects a debug info file if its CRC
// doesn't match with the one in .gdb_debuglink.
//
// Therefore, we'll try to make our CRC checksum as unique as possible.
// We'll remember that checksum, and after creating a debug info file, add
// a few bytes of garbage at the end of it so that the debug info file's
// CRC checksum becomes the one that we have precomputed.
pub fn write_gnu_debuglink<E: Arch>(ctx: &mut Context<E>, buf: &mut [u8]) {
    let _t = ctx.timer("write_gnu_debuglink");
    let crc = match &ctx.buildid {
        Some(buildid) => crc32fast::hash(&buildid.contents),
        None => {
            const SHARD: usize = 4 * 1024 * 1024;
            let hashes: Vec<u8> = buf
                .par_chunks(SHARD)
                .flat_map_iter(|s| xxhash_rust::xxh3::xxh3_64(s).to_le_bytes())
                .collect();
            crc32fast::hash(&hashes)
        }
    };
    ctx.gnu_debuglink.as_mut().unwrap().crc32 = crc;
    let hdr = ctx.gnu_debuglink.as_ref().unwrap().hdr.shdr;
    let start = hdr.sh_offset.get() as usize;
    let end = (hdr.sh_offset.get() + hdr.sh_size.get()) as usize;
    misc::gnu_debuglink::copy_buf(ctx, &mut buf[start..end]);
}

// Compute a CRC for given data in parallel
/// The CRC32 of a large buffer, computed in parallel.
fn crc32_parallel(buf: &[u8]) -> u32 {
    const SHARD: usize = 1024 * 1024; // 1 MiB
    buf.par_chunks(SHARD)
        .map(|shard| {
            let mut hasher = crc32fast::Hasher::new();
            hasher.update(shard);
            hasher
        })
        .reduce(crc32fast::Hasher::new, |mut a, b| {
            a.combine(&b);
            a
        })
        .finalize()
}

// This function "forges" a CRC. That is, given the current and a desired
// CRC32 value, crc32_solve() returns a binary blob to add to the end of
// the original data to yield the desired CRC. Trailing garbage is ignored
// by many bianry file formats, so you can create a file with a desired
// CRC using crc32_solve(). We need it for --separate-debug-file.
fn crc32_solve(current: u32, desired: u32) -> [u8; 4] {
    const POLY: u32 = 0xedb8_8320;
    let mut x = !desired;

    // Each iteration computes x = (x * x^-1) mod poly.
    for _ in 0..32 {
        x = x.rotate_left(1);
        x ^= (x & 1) * (POLY << 1);
    }
    (x ^ !current).to_le_bytes()
}

// Write a separate debug file. This function is called after we finish
// writing to the usual output file.
pub fn write_separate_debug_file<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("write_separate_debug_file");

    let path = ctx.args.separate_debug_file.clone();

    // Open an output file early
    let mut output = OutputFile::open_locked(&ctx.diag, &path, 0o666);

    // We want to write to the debug info file in background so that the
    // user doesn't have to wait for it to complete.
    if ctx.args.detach {
        crate::subprocess::notify_parent();
    }

    // Restore debug info sections that had been set aside while we were
    // creating the main file.
    let num_chunks = ctx.chunks.len();
    let debug_chunks = std::mem::take(&mut ctx.debug_chunks);
    ctx.chunks.extend(debug_chunks);

    // A debug info file contains all sections as the original file, though
    // most of them can be empty as if they were bss sections. We convert
    // real sections into dummy sections here.
    for i in 0..num_chunks {
        let id = ctx.chunks[i];
        if id.is_header()
            || id == ChunkId::Shstrtab
            || ctx.chunk_header(id).shdr.sh_type.get() == SHT_NOTE
        {
            continue;
        }
        let hdr = ctx.chunk_header(id);
        let mut placeholder =
            ChunkHeader::<E>::with_name(hdr.name, SHT_NOBITS, hdr.shdr.sh_flags.get());
        placeholder.shdr = hdr.shdr;
        placeholder.shdr.sh_type.set(SHT_NOBITS);
        placeholder.shndx = hdr.shndx;
        ctx.placeholders.push(placeholder);
        ctx.chunks[i] = ChunkId::Placeholder(ctx.placeholders.len() as u32 - 1);
    }

    let new_chunks = ctx.chunks[num_chunks..].to_vec();
    for id in new_chunks {
        output_chunks::compute_section_size(ctx, id);
    }
    sort_debug_info_sections(ctx);

    // Handle --compress-debug-info
    if ctx.args.compress_debug_sections != ELFCOMPRESS_NONE {
        compress_debug_sections(ctx);
    }

    // Recompute section header contents since we have added debug sections
    compute_section_headers(ctx);

    // Assign file offsets to sections
    let page_size = ctx.page_size;
    let mut fileoff = 0;
    for id in ctx.chunks.clone() {
        let shdr = &mut ctx.chunk_header_mut(id).shdr;
        if shdr.sh_type.get() == SHT_NOBITS {
            shdr.sh_offset.set(fileoff);
        } else if shdr.sh_flags.get() & SHF_ALLOC as u64 != 0 {
            fileoff = align_with_skew(fileoff, page_size, shdr.sh_addr.get());
            shdr.sh_offset.set(fileoff);
            fileoff += shdr.sh_size.get();
        } else {
            fileoff = align_to(fileoff, shdr.sh_addralign.get());
            shdr.sh_offset.set(fileoff);
            fileoff += shdr.sh_size.get();
        }
    }

    // The program header keeps its size, since the placeholders' addresses
    // were laid out around it.
    if let Some(n) = ctx.phdr.as_ref().map(|p| p.phdrs.len()) {
        output_chunks::update_phdr(ctx);
        let phdr = ctx.phdr.as_mut().unwrap();
        phdr.phdrs.resize(n, ElfPhdr::<E>::default());
        phdr.hdr
            .shdr
            .sh_size
            .set((n * std::mem::size_of::<ElfPhdr<E>>()) as u64);
    }

    // Write to a separate debug file
    output.resize(&ctx.diag, fileoff);
    crate::driver::copy_chunks(ctx, output.buf());

    if ctx.gdb_index.is_some() {
        crate::gdb_index::build_tables_now(ctx);
        crate::gdb_index::write(ctx, &mut output);
    }

    // Reverse-compute a CRC32 value so that the CRC32 checksum embedded to
    // the .gnu_debuglink section in the main executable matches with the
    // debug info file's CRC32 checksum.
    let trailer = crc32_solve(
        crc32_parallel(output.buf()),
        ctx.gnu_debuglink.as_ref().unwrap().crc32,
    );
    let len = output.len();
    output.extend(&ctx.diag, trailer.len());
    output.buf()[len..].copy_from_slice(&trailer);
    output.close(&ctx.diag);
}

// Write Makefile-style dependency rules to a file specified by
// --dependency-file. This is analogous to the compiler's -M flag.
pub fn write_dependency_file<E: Arch>(ctx: &Context<E>) {
    // Dependencies are listed in command line order, which is the order
    // of file priorities.
    let mut files: Vec<(u32, &MappedFile)> = ctx
        .objs
        .iter()
        .filter_map(|f| Some((f.base.priority, f.base.mf?)))
        .chain(
            ctx.dsos
                .iter()
                .filter_map(|f| Some((f.base.priority, f.base.mf?))),
        )
        .chain(ctx.lto_input_files.iter().copied())
        .collect();
    files.sort_by_key(|&(priority, _)| priority);

    let mut deps: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (_, mf) in files {
        let top = mf.parent.unwrap_or(mf);
        if top.is_dependency() {
            let path = crate::util::path_clean(&top.name);
            if seen.insert(path.clone()) {
                deps.push(path);
            }
        }
    }

    let mut out = format!("{}:", ctx.args.output);
    for d in &deps {
        out.push(' ');
        out.push_str(d);
    }
    out.push('\n');
    for d in &deps {
        out.push_str(&format!("\n{d}:\n"));
    }

    let path = &ctx.args.dependency_file;
    if path == "-" {
        let _ = std::io::stdout().write_all(out.as_bytes());
    } else {
        std::fs::write(path, out)
            .unwrap_or_else(|e| fatal!(ctx, "--dependency-file: cannot open {path}: {e}"));
    }
}

pub fn show_stats<E: Arch>(ctx: &Context<E>) {
    let print = |name: &str, value: usize| out!(ctx, "{name:>20}={value}");
    print("num_objs", ctx.objs.len());
    print("num_dsos", ctx.dsos.len());
    print("output_chunks", ctx.chunks.len());
    print(
        "input_sections",
        ctx.objs.iter().map(|f| f.sections.len()).sum(),
    );
    print(
        "comdats",
        ctx.objs.iter().map(|f| f.comdat_groups.len()).sum(),
    );
    print("num_cies", ctx.objs.iter().map(|f| f.cies.len()).sum());
    print("num_fdes", ctx.objs.iter().map(|f| f.fdes.len()).sum());
    print(
        "merged_strings",
        ctx.merged_sections
            .iter()
            .map(crate::output_chunks::merged::num_fragments)
            .sum(),
    );
}

/// Whether `--section-order` names a section that doesn't exist.
pub fn align_down_to_page<E: Arch>(ctx: &Context<E>, addr: u64) -> u64 {
    align_down(addr, ctx.page_size)
}
