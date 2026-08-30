//! The passes of a link, in roughly the order the driver runs them.

use std::cell::UnsafeCell;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use bstr::BStr;
use rayon::prelude::*;

use crate::arch::{Arch, Family};
use crate::args::{
    BsymbolicKind, BuildIdKind, CetReportKind, DefsymValue, SectionOrderKind, SeparateCodeKind,
    ShuffleSectionsKind, UnresolvedKind,
};
use crate::chunks::dynamic::{DynamicSection, RelrDynSection};
use crate::chunks::eh_frame::{EhFrameHdrSection, EhFrameRelocSection};
use crate::chunks::misc::{
    self, BuildIdSection, GnuDebuglinkSection, InterpSection, NotePropertySection,
    RelroPaddingSection,
};
use crate::chunks::output_section::OutputSection;
use crate::chunks::symtab::{
    self, GnuHashSection, HashSection, ShstrtabSection, SymtabShndxSection,
};
use crate::chunks::version::VerdefSection;
use crate::chunks::{
    self, ChunkHeader, ChunkId, GdbIndexSection, OutputEhdr, OutputPhdr, OutputSectionId,
    OutputShdr,
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
        ctx.ehdr = Some(OutputEhdr::new::<E>(ehdr_flags));
        chunks.push(ChunkId::Ehdr);
        ctx.phdr = Some(OutputPhdr::new::<E>(phdr_flags));
        chunks.push(ChunkId::Phdr);
        if ctx.args.z_sectionheader {
            ctx.shdr = Some(OutputShdr::new::<E>());
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
        ctx.relrdyn = Some(RelrDynSection::new::<E>(&ctx.args));
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
        ctx.hash = Some(HashSection::new::<E>());
        chunks.push(ChunkId::Hash);
    }
    if ctx.args.hash_style_gnu {
        ctx.gnu_hash = Some(GnuHashSection::new::<E>());
        chunks.push(ChunkId::GnuHash);
    }
    if !ctx.args.version_definitions.is_empty() {
        ctx.verdef = Some(VerdefSection::new());
        chunks.push(ChunkId::Verdef);
    }
    if ctx.args.emit_relocs {
        ctx.eh_frame_reloc = Some(EhFrameRelocSection::new::<E>());
        chunks.push(ChunkId::EhFrameReloc);
    }
    if !ctx.args.separate_debug_file.is_empty() {
        ctx.gnu_debuglink = Some(GnuDebuglinkSection::new());
        chunks.push(ChunkId::GnuDebuglink);
    }

    if ctx.args.shared || !ctx.dsos.is_empty() || ctx.args.pie {
        ctx.dynamic = Some(DynamicSection::new::<E>(&ctx.args));
        chunks.push(ChunkId::Dynamic);
        // .dynamic refers to .dynsym and .dynstr, so they must exist.
        ctx.dynstr.add_string(b"");
        if ctx.dynsym.symbols.is_empty() {
            ctx.dynsym.symbols.push(None);
        }
    }

    chunks.push(ChunkId::Versym);
    chunks.push(ChunkId::Verneed);
    chunks.push(ChunkId::NotePackage);

    if !ctx.args.oformat_binary {
        let shdr = ElfShdr {
            sh_type: SHT_PROGBITS,
            sh_flags: (SHF_MERGE | SHF_STRINGS) as u64,
            ..ElfShdr::default()
        };
        let merged = RwLock::new(std::mem::take(&mut ctx.merged_sections));
        ctx.comment = crate::chunks::merged::MergedSection::get_instance(
            &ctx.args,
            &merged,
            BStr::new(b".comment"),
            &shdr,
        );
        ctx.merged_sections = merged.into_inner().unwrap();
    }

    if E::IS_X86 {
        ctx.note_property = Some(NotePropertySection::new::<E>());
        chunks.push(ChunkId::NoteProperty);
    }
    if E::IS_RISCV {
        ctx.riscv_attributes = Some(crate::chunks::misc::RiscvAttributesSection::new());
        chunks.push(ChunkId::RiscvAttributes);
    }
    if E::FAMILY == Family::Ppc64V2 {
        ctx.ppc64_save_restore = Some(crate::chunks::misc::Ppc64SaveRestoreSection::new());
        chunks.push(ChunkId::Ppc64SaveRestore);
    }
    if E::FAMILY == Family::Ppc64V1 {
        ctx.ppc64_opd = Some(crate::chunks::opd::Ppc64OpdSection::new());
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
                let esym = &file.base.elf_syms.at_in::<E>(i);
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
                let esym = &file.base.elf_syms.at_in::<E>(i);
                let sym = &ctx.symbols[file.base.symbols[i]];
                if sym.is_traced() {
                    crate::input_files::print_trace_symbol(&ctx.diag, file, esym, sym);
                }
                // Undefined symbols in a DSO are followed only for
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

/// A default-versioned symbol `foo@@VER` can be referred to as `foo` or
/// `foo@VER`. References to the latter were resolved to a forwarding
/// alias; redirect them to the real symbol.
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

fn clear_symbol(sym: &mut Symbol) {
    sym.clear_file();
    sym.clear_origin();
    sym.value = 0;
    sym.sym_idx = u32::MAX;
    sym.set_esym(&ElfSym::default());
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
            clear_symbol(sym);
        }
    });
}

/// Creates a symbol for each global symbol name recorded while reading
/// files and fills in the files' symbol references.
/// Interns the global symbols every input file refers to.
///
/// The keys are recorded by owner and index, then interned all at once:
/// objects own their global symbol slots, and a shared library owns two
/// sets, its symbols and their default-version aliases.
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
                file.resolve_symbols::<E>(&resolver, id);
            }
        }
        FileId::Dso(id) => {
            let file = &dsos[id.index()];
            if !only_reachable || file.base.is_reachable() {
                file.resolve_symbols::<E>(&resolver, id);
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
        file.resolve_skip_dso_symbols::<E>(&resolver, file.id());
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

/// Selects COMDAT groups and constructs input sections.
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
                        file.read_section_metadata::<E>(diag);
                    }
                    let priority = file.base.priority;
                    let is_lto_output = file.is_lto_output;
                    for group in &mut file.comdat_groups {
                        if group.signature() != SymbolId::DISCARDED_COMDAT {
                            let sym = &symbols[group.signature()];
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

    // An IR file may claim a group only if no reachable regular object
    // already has. Its claim is permanent: the LTO result provides the
    // group's definitions, so a regular object extracted after LTO must
    // not win the group and resurrect a copy of them.
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
                    file.parse_sections::<E>(
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

    // Apply the selection to all group members.
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
            clear_symbol(sym);
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
fn remove_objects<E: Arch>(ctx: &mut Context<E>, remove: impl Fn(&ObjectFile) -> bool) {
    ctx.objs.retain(|file| !remove(file));
}

/// Whether the link involves the LTO plugin.
pub fn has_lto_obj<E: Arch>(ctx: &Context<E>) -> bool {
    ctx.objs
        .iter()
        .any(|file| file.base.is_reachable() && (file.is_lto_input || file.is_gcc_offload_obj))
}

/// Runs link-time optimization: the plugin compiles the IR objects into
/// ELF objects, which then take their place.
pub fn do_lto<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("do_lto");

    // The compiler backend needs to know how symbols are resolved, so
    // visibility and import/export bits are computed early.
    apply_version_script(ctx);
    parse_symbol_version(ctx);
    compute_import_export(ctx);

    // If several IR objects define the same symbol, the backend would
    // pick one at random rather than complain.
    if !ctx.args.allow_multiple_definition {
        check_duplicate_symbols(ctx);
    }

    crate::lto::run_plugin(ctx);

    // Redo name resolution without the IR objects. Archive members that
    // were extracted for them may no longer be needed, so their
    // reachability is decided afresh too.
    clear_symbols(ctx);
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
        .for_each(|file| file.parse_ehframe::<E>(diag));
}

pub fn parse_sframe_sections<E: Arch>(ctx: &mut Context<E>) {
    if !E::SUPPORTS_SFRAME {
        return;
    }
    let _t = ctx.timer("parse_sframe_sections");
    let Context { objs, diag, .. } = ctx;
    objs.par_iter_mut()
        .for_each(|file| file.parse_sframe::<E>(diag));
}

/// Registers direct, stable member borrows with their merged sections for a
/// parallel resolution phase.
fn merged_resolve_members(
    objs: &mut FileList<ObjectFile>,
    count: usize,
) -> Vec<Vec<crate::chunks::merged::ResolveMember<'_>>> {
    let mut members: Vec<Vec<crate::chunks::merged::ResolveMember<'_>>> =
        (0..count).map(|_| Vec::new()).collect();
    for file in objs {
        let filename = file.base.filename.as_str();
        let archive_name = file.archive_name.as_str();
        let shstrtab = file.base.shstrtab;
        let num_elf_sections = file.num_elf_sections;
        for (mergeable, input) in file.sections.mergeable_sections_with_inputs_mut() {
            let parent = mergeable.parent.index();
            members[parent].push(crate::chunks::merged::ResolveMember {
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
            .for_each(|file| file.convert_mergeable_sections::<E>(args, &merged, diag));
        *merged_sections = merged.into_inner().unwrap();
    }
    drop(t);

    // Register each mergeable section with its merged section.
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
    crate::chunks::merged::resolve_sections::<E>(
        &mut ctx.merged_sections,
        &mut members,
        crate::chunks::merged::ResolveOptions {
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
                    file.reattach_section_symbols::<E>(diag, id, &editor, merged_sections);
                    file.reattach_fragment_relocations::<E>(
                        diag,
                        id,
                        merged_sections,
                        base_id,
                        slots,
                    );
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
        file.convert_common_symbols::<E>(
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
    // The x86-64 psABI defines SHT_X86_64_UNWIND for .eh_frame, but the
    // section may come as either type. Use SHT_PROGBITS consistently.
    if E::FAMILY == Family::X86_64 && ty == SHT_X86_64_UNWIND {
        return SHT_PROGBITS;
    }
    ty
}

fn output_name<E: Arch>(
    args: &crate::args::Args,
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
    args: &crate::args::Args,
    isec: &crate::input_sections::InputSection,
    name: &'static BStr,
    sh_type: u32,
    ctors_in_init_array: bool,
) -> (&'static BStr, u32) {
    // .ctors/.dtors are merged into .init_array/.fini_array if those
    // exist, except for the relocation-free sentinel sections in CRT
    // files, whose values 0 and -1 would crash the program.
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

/// Stable scratch storage corresponding to C++ `OutputSection::members_vec`.
struct OutputSectionBuilder {
    section: OutputSectionId,
    files: Box<[UnsafeCell<OutputSectionFileMembers>]>,
}

type OutputSectionShared = (
    HashMap<OutputSectionKey, Arc<OutputSectionBuilder>>,
    Vec<OutputSection>,
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

/// PT_GNU_RELRO makes pages that need dynamic relocations at load time
/// read-only afterwards. Sections such as `.init_array`, `.got` and
/// `.dynamic` need relocations but not writability at runtime.
fn is_relro(osec: &OutputSection) -> bool {
    let name = osec.hdr.name;
    let ty = osec.hdr.shdr.sh_type;
    let flags = osec.hdr.shdr.sh_flags;
    name == b".toc"
        || name.ends_with(b".rel.ro")
        || name.ends_with(b".rel.ro.hot")
        || name.ends_with(b".rel.ro.unlikely")
        || matches!(ty, SHT_INIT_ARRAY | SHT_FINI_ARRAY | SHT_PREINIT_ARRAY)
        || flags & SHF_TLS as u64 != 0
}

/// Creates output sections for input sections.
pub fn create_output_sections<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("create_output_sections");
    let ctors_in_init_array = has_ctors_and_init_array(ctx);
    let first_new = ctx.output_sections.len();

    // Instantiate output sections and assign input sections to them. The
    // output sections and the map from keys to them are shared under a
    // lock; each worker keeps a cache of the map to avoid lock contention.
    // It makes a noticeable difference if we have millions of input
    // sections.
    let num_files = ctx.objs.len();
    let shared: Mutex<OutputSectionShared> =
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
                for (member, isec) in file
                    .sections
                    .regular_ids_mut()
                    .filter(|(_, isec)| isec.is_alive())
                {
                    let name = isec.name_in(shstrtab, num_elf_sections);
                    let sh_type = isec.sh_type_from(shdrs);
                    let sh_flags = isec.sh_flags
                        & !(SHF_MERGE | SHF_STRINGS | SHF_COMPRESSED | SHF_GNU_RETAIN) as u64;

                    if args.relocatable && sh_flags & SHF_GROUP as u64 != 0 {
                        // COMDAT group members keep their own output sections
                        // in a relocatable output.
                        let mut osec = OutputSection::new(name, sh_type);
                        osec.hdr.shdr.sh_flags = sh_flags;
                        osec.hdr.shdr.sh_addralign = 1 << isec.p2align();
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
                                    sections.push(OutputSection::new(key.0, key.1));
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

    // Flatten members_vec into members and compute the section alignment.
    // Both are done in parallel over the files; an output section such as
    // .text has a million members.
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
        osec.hdr.shdr.sh_flags = sh_flags;
        osec.hdr.shdr.sh_addralign = 1 << p2align;
        osec.hdr.is_relro = is_relro(osec);
    }

    // Output sections are created in an arbitrary order; sort them to
    // make the output deterministic.
    let mut chunks: Vec<ChunkId> = (first_new..ctx.output_sections.len())
        .map(|i| ChunkId::Output(OutputSectionId::new(i as u32)))
        .chain(
            (0..ctx.merged_sections.len())
                .map(|i| ChunkId::Merged(crate::chunks::merged::MergedSectionId(i as u32))),
        )
        .collect();
    chunks.sort_by_cached_key(|&id| {
        let hdr = ctx.chunk_header(id);
        (hdr.name.to_vec(), hdr.shdr.sh_type, hdr.shdr.sh_flags)
    });
    ctx.chunks.extend(chunks);
}

/// Creates the object file that holds linker-synthesized symbols.
pub fn create_internal_file<E: Arch>(ctx: &mut Context<E>) {
    let mut obj = ObjectFile::internal();
    obj.base.priority = 0;

    ctx.internal_esyms = vec![ElfSym::default()];
    let dummy = ctx.symbols.add(Symbol::new(BStr::new(b"")));
    obj.base.symbols.push(dummy);
    obj.base.first_global = 1;

    let add = |ctx: &mut Context<E>, obj: &mut ObjectFile, name: &str| {
        let id = ctx.get_symbol(name.as_bytes());
        obj.base.symbols.push(id);
        // The real value is set by fix_synthetic_symbols; a distinctive
        // dummy makes accidental early uses easier to spot.
        ctx.symbols[id].value = 0xdeadbeef;
        let mut esym = ElfSym {
            st_shndx: SHN_ABS as u16,
            ..ElfSym::default()
        };
        esym.set_type(STT_NOTYPE);
        esym.set_bind(STB_GLOBAL);
        esym.set_visibility(STV_DEFAULT);
        ctx.internal_esyms.push(esym);
    };

    for (name, _) in ctx.args.defsyms.clone() {
        add(ctx, &mut obj, &name);
    }
    for order in ctx.args.section_order.clone() {
        if order.kind == SectionOrderKind::Symbol {
            add(ctx, &mut obj, &order.name);
        }
    }

    obj.base.elf_syms = SymTable::from_records(RecordLayout::of::<E>(), &ctx.internal_esyms);
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
        let esym = obj.base.elf_syms.at_in::<E>(i);
        let sym_id = obj.base.symbols[i];
        let rank = symbol_resolution_rank(&esym, false, false, 0);
        if rank < current_rank(ctx, &ctx.symbols[sym_id]) {
            let sym = &mut ctx.symbols[sym_id];
            sym.set_file(FileId::Obj(id));
            sym.clear_origin();
            sym.value = esym.st_value;
            sym.sym_idx = i as u32;
            sym.set_esym(&esym);
            sym.ver_idx = ctx.default_version;
            sym.set_weak(esym.is_weak());
            sym.set_versioned_default(false);
        }
    }
}

pub fn add_synthetic_symbols<E: Arch>(ctx: &mut Context<E>) {
    let obj_id = ctx.internal_obj.unwrap();

    fn add<E: Arch>(ctx: &mut Context<E>, name: &str, ty: u32) -> SymbolId {
        let mut esym = ElfSym {
            st_shndx: SHN_ABS as u16,
            ..ElfSym::default()
        };
        esym.set_type(ty);
        esym.set_bind(STB_GLOBAL);
        esym.set_visibility(STV_HIDDEN);
        ctx.internal_esyms.push(esym);
        let id = ctx.get_symbol(name.as_bytes());
        ctx.symbols[id].value = 0xdeadbeef;
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

    ctx.objs[obj_id.index()].base.elf_syms =
        SymTable::from_records(RecordLayout::of::<E>(), &ctx.internal_esyms);
    resolve_internal_symbols(ctx);

    // Make all synthetic symbols relative by associating them with a
    // dummy output section.
    let syms = ctx.objs[obj_id.index()].base.symbols.clone();
    for id in &syms {
        let sym = &mut ctx.symbols[*id];
        if sym.file() == Some(FileId::Obj(obj_id)) {
            sym.set_output_chunk(ChunkId::Symtab);
            sym.set_imported(false);
        }
    }

    // --defsym
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
                let mut esym = obj.base.elf_syms.at_in::<E>(i + 1);
                esym.set_type(sym2_esym.st_type());
                if E::FAMILY == Family::Ppc64V2 {
                    esym.st_other = (esym.st_other & 0x1f) | (sym2_esym.st_other & 0xe0);
                }
                obj.base.elf_syms.set_in::<E>(i + 1, esym);
                ctx.internal_esyms[i + 1] = esym;
                if ctx.symbols[sym1].file() == Some(FileId::Obj(obj_id)) {
                    ctx.symbols[sym1].set_esym(&esym);
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
            osec.hdr.shdr.sh_addralign = align;
        }
    }
}

pub fn check_cet_errors<E: Arch>(ctx: &Context<E>) {
    let warning = ctx.args.z_cet_report == CetReportKind::Warning;
    let has_feature = |file: &ObjectFile, feature: u32| {
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

    let println = |src: &dyn std::fmt::Display, sym: &Symbol, esym: &ElfSym| {
        let kind = if esym.is_weak() { 'w' } else { 'u' };
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
                if r.r_type == R_NONE || file.base.elf_syms.len() <= r.r_sym as usize {
                    continue;
                }
                let esym = &file.base.elf_syms.at_in::<E>(r.r_sym as usize);
                let id = file.base.symbols[r.r_sym as usize];
                let sym = &ctx.symbols[id];
                if esym.is_undef()
                    && sym.file().is_some()
                    && sym.file() != Some(FileId::Obj(file.id()))
                    && visited.insert(id)
                {
                    println(&isec.display(file), sym, esym);
                }
            }
        }
    }
    for file in &ctx.dsos {
        for i in 0..file.base.elf_syms.len() {
            let esym = &file.base.elf_syms.at_in::<E>(i);
            let sym = &ctx.symbols[file.base.symbols[i]];
            if esym.is_undef() && sym.file().is_some() && sym.file() != Some(FileId::Dso(file.id()))
            {
                println(file, sym, esym);
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
        format!("{}\n", crate::args::VERSION).as_bytes(),
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
            let abs = std::fs::canonicalize(&top.name)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or(top.name.clone());
            write(&mut tar, &abs, top.data());
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
            let esym = &file.base.elf_syms.at_in::<E>(i);
            let sym = &ctx.symbols[file.base.symbols[i]];

            // Skip if our symbol is undefined or weak.
            let Some(owner) = sym.file() else { continue };
            if owner == file_id
                || ctx.internal_obj.map(FileId::Obj) == Some(owner)
                || esym.is_undef()
                || esym.is_common()
                || esym.st_bind() == STB_WEAK
            {
                continue;
            }
            // Skip if our symbol is in a dead section, most likely due to
            // COMDAT deduplication.
            if !esym.is_abs() {
                match file.symbol_section(i) {
                    Some(isec) if isec.is_alive() => {}
                    _ => continue,
                }
            }
            if file.is_lto_input && file.lto_comdat_discarded[i] {
                continue;
            }
            // The LTO backend sorts out conflicts between IR and regular
            // objects itself; only IR-vs-IR duplicates are caught here.
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

/// Exporting both `foo@@VER` and `foo@VER` would leave the loader to pick
/// between two definitions of the same versioned name.
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
                    crate::util::display(file.base.symbol_name_in::<E>(sym.sym_idx as usize))
                );
            }
        }
    }
    ctx.checkpoint();
}

/// Converts allocated data sections containing only zeros into BSS.
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

/// With --no-allow-shlib-undefined, reports unresolved symbols in shared
/// libraries, which the dynamic linker would otherwise report at runtime.
pub fn check_shlib_undefined<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("check_shlib_undefined");

    // Skip the test unless we have the complete set of shared libraries:
    // a missing one might define the symbol.
    let complete = ctx.dsos.iter().all(|dso| {
        dso.dt_needed::<E>(&ctx.diag)
            .iter()
            .all(|needed| ctx.dso_sonames.contains(&*String::from_utf8_lossy(needed)))
    });

    if complete {
        ctx.dsos.par_iter().for_each(|file| {
            for i in 0..file.base.elf_syms.len() {
                let esym = &file.base.elf_syms.at_in::<E>(i);
                let id = file.base.symbols[i];
                let sym = &ctx.symbols[id];
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

    // DSOs not referenced by any object were kept only for this pass.
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
    let check = |file: &dyn std::fmt::Display, file_id: FileId, sym: &Symbol, esym2: &ElfSym| {
        let esym1 = &sym.esym(ctx);
        if let Some(owner) = sym.file() {
            if owner != file_id
                && esym1.st_type() != STT_NOTYPE
                && esym2.st_type() != STT_NOTYPE
                && canonicalize(esym1.st_type()) != canonicalize(esym2.st_type())
            {
                warn!(
                    ctx,
                    "symbol type mismatch: {sym}\n>>> defined in {} as {}\n>>> defined in {file} as {}",
                    ctx.file_display(owner),
                    stt_to_string(esym1.st_type()),
                    stt_to_string(esym2.st_type())
                );
            }
        }
    };

    ctx.objs.par_iter().for_each(|file| {
        let id = FileId::Obj(file.id());
        for i in file.base.first_global..file.base.elf_syms.len() {
            let sym = &ctx.symbols[file.base.symbols[i]];
            if sym.file().is_some() && sym.file() != Some(id) {
                check(file, id, sym, &file.base.elf_syms.at_in::<E>(i));
            }
        }
    });
    ctx.dsos.par_iter().for_each(|file| {
        let id = FileId::Dso(file.id());
        for i in 0..file.base.elf_syms.len() {
            let sym = &ctx.symbols[file.base.symbols[i]];
            if sym.file().is_some() && sym.file() != Some(id) {
                check(file, id, sym, &file.base.elf_syms.at_in::<E>(i));
            }
            let id2 = file.symbols2[i];
            if id2 != SymbolId::NONE {
                let sym = &ctx.symbols[id2];
                if sym.file().is_some() && sym.file() != Some(id) {
                    check(file, id, sym, &file.base.elf_syms.at_in::<E>(i));
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
    // crtbegin.o and crtend.o contain marker symbols such as __CTOR_LIST__
    // and must be at the beginning or end of the section.
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

/// Places DWARF32 input sections before DWARF64 ones in large debug
/// sections, so that 32-bit references between debug sections don't
/// overflow until the DWARF32 part alone exceeds 4 GiB.
pub fn sort_debug_info_sections<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("sort_debug_info_sections");
    let is_in_test = std::env::var("MOLD_DEBUG").is_ok_and(|v| !v.is_empty());

    let vec1: Vec<OutputSectionId> = (0..ctx.output_sections.len())
        .map(|i| OutputSectionId::new(i as u32))
        .filter(|&id| {
            let osec = &ctx.output_sections[id.index()];
            !osec.hdr.is_alloc()
                && osec.hdr.name.starts_with(b".debug_")
                && (osec.hdr.shdr.sh_size >= u32::MAX as u64 || is_in_test)
        })
        .collect();
    let vec2: Vec<crate::chunks::merged::MergedSectionId> = (0..ctx.merged_sections.len())
        .map(|i| crate::chunks::merged::MergedSectionId(i as u32))
        .filter(|&id| {
            let msec = &ctx.merged_sections[id.index()];
            !msec.is_alloc()
                && msec.hdr.name.starts_with(b".debug_")
                && (msec.hdr.shdr.sh_size >= u32::MAX as u64 || is_in_test)
        })
        .collect();
    if vec1.is_empty() && vec2.is_empty() {
        return;
    }

    {
        let Context { objs, diag, .. } = ctx;
        objs.par_iter_mut().for_each(|file| {
            file.is_dwarf32 = !file.debug_info_sections.is_empty() && file.is_dwarf32::<E>(diag);
        });
    }

    let has_dwarf32 = ctx.objs.iter().any(|f| f.is_dwarf32);
    let has_dwarf64 = ctx
        .objs
        .iter()
        .any(|f| !f.is_dwarf32 && !f.debug_info_sections.is_empty());
    if !has_dwarf32 || !has_dwarf64 {
        return;
    }

    for id in vec1 {
        let section_arena = &ctx.section_arena;
        let objs = &ctx.objs;
        let osec = &mut ctx.output_sections[id.index()];
        let (a, b): (Vec<InputSectionId>, Vec<InputSectionId>) = osec
            .members
            .iter()
            .partition(|&&m| objs[section_arena.section(m).file.index()].is_dwarf32);
        osec.members = a.into_iter().chain(b).collect();
        chunks::compute_section_size(ctx, ChunkId::Output(id));
    }

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
        chunks::compute_section_size(ctx, ChunkId::Merged(id));
    }
}

/// `.ctors`/`.dtors` are executed in the opposite order to
/// `.init_array`/`.fini_array`, so their contents are reversed when they
/// are placed there.
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
            let mut rels: Vec<ElfRel> = isec.rels::<E>(file).iter().collect();
            for r in &mut rels {
                r.r_offset = size - r.r_offset - word as u64;
            }
            rels.sort_by_key(|r| r.r_offset);
            let file = &mut ctx.objs[section_ref.file.index()];
            file.section_mut(section_ref.shndx as usize)
                .unwrap()
                .set_contents(leak_bytes(contents));
            file.rels_mut::<E>(section_ref.shndx).set_all(&rels);
        }
    }
}

/// Shuffles members with a xorshift generator and Fisher-Yates, which
/// are stable across platforms unlike the standard library's.
fn shuffle(vec: &mut [InputSectionId], mut seed: u64) {
    if vec.is_empty() {
        return;
    }
    let mut rand = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for i in 0..vec.len() - 1 {
        let j = i + (rand() % (vec.len() - i) as u64) as usize;
        vec.swap(i, j);
    }
}

pub fn shuffle_sections<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("shuffle_sections");
    let is_eligible = |osec: &OutputSection| {
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
        let audit = ctx.dsos[id.index()].dt_audit::<E>(&ctx.diag);
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
                && ctx.output_sections[osec.index()].hdr.shdr.sh_flags & SHF_EXECINSTR as u64 != 0
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
        crate::chunks::merged::resolve_sections::<E>(
            &mut ctx.merged_sections,
            &mut members,
            crate::chunks::merged::ResolveOptions {
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
                    Some((osec, chunks::output_section::layout(ctx, osec)))
                }
                _ => None,
            })
            .collect()
    };
    for (osec, size) in sizes {
        ctx.output_sections[osec.index()].hdr.shdr.sh_size = size;
    }

    // Every merged section now owns all the mutable state needed for its
    // layout, just as each C++ Chunk does in the parallel chunk loop.
    ctx.merged_sections
        .par_iter_mut()
        .for_each(crate::chunks::merged::layout);

    for id in ctx.chunks.clone() {
        match id {
            ChunkId::Output(_) => {}
            ChunkId::Merged(_) => {}
            _ => chunks::compute_section_size(ctx, id),
        }
    }
}

/// Attaches every unresolved symbol to a file: a shared library import,
/// or an absolute symbol with value 0. Errors for referenced ones are
/// reported by relocation scanning, so that only actually used symbols
/// are reported.
pub fn claim_unresolved_symbols<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("claim_unresolved_symbols");

    // Nearly all references are to defined symbols, which are filtered
    // out first.
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
                            && file.base.elf_syms.at_in::<E>(i).is_undef()
                            && ctx_ref.symbols[file.base.symbols[i]].file().is_none()
                    })
                    .map(move |i| (file_id, i))
            })
            .collect()
    };

    for (obj_id, i) in candidates {
        let file = &ctx.objs[obj_id.index()];
        let esym = file.base.elf_syms.at_in::<E>(i);
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
            // Weak undefined symbols become dynamic symbols only in a DSO
            // by default: an executable might need a copy relocation for a
            // data symbol, whose size is unknown for an unclaimed symbol.
            // Otherwise they become absolute symbols with value 0.
            claim(
                ctx,
                ctx.args.z_dynamic_undefined_weak && visibility != STV_HIDDEN,
            );
            continue;
        }
        // Undefined symbols in a shared object are promoted to dynamic
        // symbols unless `-z defs` is given.
        if ctx.args.shared
            && visibility != STV_HIDDEN
            && ctx.args.unresolved_symbols != UnresolvedKind::Error
        {
            claim(ctx, true);
            continue;
        }
        claim(ctx, false);
    }
}

pub fn scan_relocations<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("scan_relocations");

    {
        let ctx_ref: &Context<E> = ctx;
        ctx_ref
            .objs
            .par_iter()
            .for_each(|file| file.scan_relocations(ctx_ref));
    }
    ctx.checkpoint();

    // Word-size absolute relocations are handled separately since they
    // can be promoted to dynamic relocations.
    let results: Vec<(
        OutputSectionId,
        Vec<crate::chunks::output_section::AbsRel>,
        Vec<u64>,
    )> = {
        let ctx_ref: &Context<E> = ctx;
        (0..ctx_ref.output_sections.len())
            .into_par_iter()
            .map(|i| OutputSectionId::new(i as u32))
            .filter(|&id| ctx_ref.output_sections[id.index()].hdr.is_alloc())
            .map(|id| {
                let (abs_rels, offsets) =
                    crate::chunks::output_section::scan_abs_relocations(ctx_ref, id);
                (id, abs_rels, offsets)
            })
            .collect()
    };
    for (id, abs_rels, offsets) in results {
        let osec = &mut ctx.output_sections[id.index()];
        osec.abs_rels = abs_rels;
        osec.dynrel_offsets = offsets;
    }
    ctx.checkpoint();

    // Gather the dynamic symbols.
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
        crate::chunks::got::got::add_tlsld(ctx);
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

    // Assign entries in the various tables to each dynamic symbol.
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
            crate::chunks::got::got::add_got_symbol(ctx, id);
        }
        if flags & NEEDS_CANONICAL != 0 && ty == STT_FUNC {
            // A canonical PLT must be visible from DSOs, and can't use
            // .plt.got because .plt.got and .got would then refer to each
            // other in an infinite loop.
            let sym = &mut ctx.symbols[id];
            sym.set_canonical(true);
            sym.set_exported(true);
            crate::chunks::got::plt::add_symbol(ctx, id);
        } else if flags & NEEDS_PLT != 0 {
            if flags & NEEDS_GOT != 0 {
                crate::chunks::got::pltgot::add_symbol(ctx, id);
            } else {
                crate::chunks::got::plt::add_symbol(ctx, id);
            }
        }
        if flags & NEEDS_GOTTP != 0 {
            crate::chunks::got::got::add_gottp_symbol(ctx, id);
        }
        if flags & NEEDS_TLSGD != 0 {
            crate::chunks::got::got::add_tlsgd_symbol(ctx, id);
        }
        if flags & NEEDS_TLSDESC != 0 {
            crate::chunks::got::got::add_tlsdesc_symbol(ctx, id);
        }
        if flags & NEEDS_CANONICAL != 0 && ty != STT_FUNC {
            let relro = ctx.args.z_relro
                && match ctx.symbols[id].file() {
                    Some(FileId::Dso(dso)) => {
                        ctx.dsos[dso.index()].is_readonly::<E>(&ctx.symbols[id])
                    }
                    _ => false,
                };
            misc::copyrel::add_symbol(ctx, relro, id);
        }
        if E::FAMILY == Family::Ppc64V1 && flags & NEEDS_PPC_OPD != 0 {
            crate::chunks::opd::add_symbol(ctx, id);
        }
        ctx.symbols[id].clear_flags();
    }

    if ctx.has_textrel.load(Ordering::Relaxed) && ctx.args.warn_textrel {
        warn!(ctx, "creating a DT_TEXTREL in an output file");
    }
}

/// An imported symbol is weak in `.dynsym` only if all references to it
/// are weak.
pub fn compute_imported_symbol_weakness<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("compute_imported_symbol_weakness");
    let strong: Vec<SymbolId> = ctx
        .objs
        .par_iter()
        .flat_map_iter(|file| {
            (file.base.first_global..file.base.elf_syms.len()).filter_map(|i| {
                let esym = &file.base.elf_syms.at_in::<E>(i);
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

/// Reports all undefined symbols, grouped by symbol.
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
    let ids: Vec<OutputSectionId> = ctx
        .chunks
        .iter()
        .filter_map(|c| c.as_output_section())
        .collect();
    let secs: Vec<misc::RelocSection> = {
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
        let num_buckets = num_exported / GnuHashSection::LOAD_FACTOR + 1;
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
    ctx.dynsym.dynstr_offset = ctx.dynstr.hdr.shdr.sh_size;
    // SAFETY: .dynsym contains each symbol at most once. Every dynamic symbol
    // already has an auxiliary record.
    let size = unsafe {
        ctx.symbols.par_sum_aux_mut(&syms, |i, sym, aux| {
            aux.dynsym_idx = Some(i as u32 + 1);
            sym.name().len() as u64 + 1
        })
    };
    ctx.dynstr.hdr.shdr.sh_size += size;
    ctx.dynsym.symbols = std::iter::once(None)
        .chain(syms.into_iter().map(Some))
        .collect();

    // ELF's symbol table sh_info holds the offset of the first global symbol.
    ctx.dynsym.hdr.shdr.sh_info = num_locals as u32 + 1;
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
        chunks::compute_symtab_size(ctx, id);
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
            if !file.has_symver[i - file.base.first_global] {
                continue;
            }
            let id = file.base.symbols[i];
            if ctx.symbols[id].file() != Some(file_id) {
                continue;
            }
            let name = file.base.symbol_name_in::<E>(i);
            let at = crate::util::find_byte(b'@', name).unwrap();
            let mut ver = &name[at + 1..];
            let mut is_default = false;
            if let Some(rest) = ver.strip_prefix(b"@") {
                is_default = true;
                ver = rest;
            }

            // `foo@@` is the unversioned default; export it globally.
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

            // If both `foo` and `foo@VERSION` are defined, the versioned
            // one hides `foo`; likewise the default one takes precedence.
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

    // An executable exports the symbols that DSOs reference, unless a
    // version script marks them local.
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

    // Export symbols that are neither hidden nor local, and mark imported
    // symbols as such.
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
                        if let Some(FileId::Dso(_)) = sym.file() {
                            return Some((id, true, false));
                        }
                        if sym.file() == Some(file_id) && should_export(ctx_ref, sym) {
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

    // --dynamic-list and friends. For an executable, matched symbols are
    // exported; for a shared object, matched symbols are imported if
    // exported, so that they are interposable, and the rest are bound
    // locally.
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

/// Computes the "address-taken" bit of each input section, for ICF.
///
/// Merging two identical objects changes pointer equality, so ICF folds
/// only sections whose addresses are never taken. For functions, direct
/// calls use dedicated relocation types, so pointer-taking references can
/// be told apart. For data, LLVM's `.llvm_addrsig` lists the symbols whose
/// addresses are taken; without it, all data is assumed address-taken.
pub fn compute_address_significance<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("compute_address_significance");
    let ctx_ref: &Context<E> = ctx;

    ctx_ref.objs.par_iter().for_each(|file| {
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
        for isec in file.input_sections() {
            if !isec.is_alive() || !isec.is_alloc() {
                continue;
            }
            if isec.sh_flags & SHF_EXECINSTR as u64 == 0 {
                isec.set_address_taken();
            }
            for r in isec.rels::<E>(file) {
                if !r.is_func_call::<E>() {
                    let sym = &ctx_ref.symbols[file.base.symbols[r.r_sym as usize]];
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
    // Some symbols' addresses leak into the dynamic section.
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

/// Sorts chunks into the standard layout: headers, then read-only data
/// needed by the loader, code, TLS, RELRO data, `.got`, writable data,
/// BSS, non-allocated sections, and the section header last.
fn sort_output_sections_regular<E: Arch>(ctx: &mut Context<E>) {
    let rank1 = |ctx: &Context<E>, id: ChunkId| -> i64 {
        let hdr = ctx.chunk_header(id);
        let ty = hdr.shdr.sh_type;
        let flags = hdr.shdr.sh_flags;
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
    let rank2 = |ctx: &Context<E>, id: ChunkId| -> i64 {
        let hdr = ctx.chunk_header(id);
        if hdr.shdr.sh_type == SHT_NOTE {
            return -(hdr.shdr.sh_addralign as i64);
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
    if hdr.shdr.sh_type == SHT_NOBITS {
        "BSS"
    } else if hdr.shdr.sh_flags & SHF_EXECINSTR as u64 != 0 {
        "TEXT"
    } else if hdr.shdr.sh_flags & SHF_WRITE as u64 != 0 {
        "DATA"
    } else {
        "RODATA"
    }
}

fn sort_output_sections_by_order<E: Arch>(ctx: &mut Context<E>) {
    let rank =
        |ctx: &Context<E>, id: ChunkId| -> i64 {
            let hdr = ctx.chunk_header(id);
            let flags = hdr.shdr.sh_flags;
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
    for id in ctx.chunks.clone() {
        let r = rank(ctx, id);
        ctx.chunk_header_mut(id).sect_order = r;
    }
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
        .filter(|h| h.shdr.sh_flags & SHF_TLS as u64 != 0)
        .map(|h| h.shdr.sh_addralign)
        .max()
        .unwrap_or(1)
        .max(1)
}

/// Assigns virtual addresses. Sections with different memory protection
/// must be in different pages, and a section's file offset must be
/// congruent to its address modulo the page size; sections are packed as
/// tightly as those constraints allow.
fn set_virtual_addresses_regular<E: Arch>(ctx: &mut Context<E>) {
    const RELRO: u64 = 1 << 32;
    let flags_of = |ctx: &Context<E>, id: ChunkId| -> u64 {
        let flags = chunks::to_phdr_flags(ctx, id) as u64;
        if ctx.args.z_relro && ctx.chunk_header(id).is_relro {
            flags | RELRO
        } else {
            flags
        }
    };
    let is_tls =
        |ctx: &Context<E>, id: ChunkId| ctx.chunk_header(id).shdr.sh_flags & SHF_TLS as u64 != 0;
    let is_tbss = |ctx: &Context<E>, id: ChunkId| {
        let shdr = &ctx.chunk_header(id).shdr;
        shdr.sh_flags & SHF_TLS as u64 != 0 && shdr.sh_type == SHT_NOBITS
    };

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

        if id == ChunkId::RelroPadding {
            let hdr = ctx.chunk_header_mut(id);
            hdr.shdr.sh_addr = addr;
            hdr.shdr.sh_size = align_to(addr, page_size) - addr;
            addr += page_size;
            i += 1;
            continue;
        }

        // --section-start
        let name = String::from_utf8_lossy(ctx.chunk_header(id).name).into_owned();
        if let Some(&start) = ctx.args.section_start.get(&name) {
            addr = start;
            let hdr = ctx.chunk_header_mut(id);
            hdr.shdr.sh_addr = addr;
            addr += hdr.shdr.sh_size;
            i += 1;
            continue;
        }

        // Sections with different memory attributes go to different pages.
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

        // The first TLS section is aligned to the PT_TLS segment's alignment.
        if is_tls(ctx, id) && (i == 0 || !is_tls(ctx, chunks[i - 1])) {
            addr = align_to(addr, tls_segment_alignment(ctx));
        }

        // TLS BSS sections overlap the following sections: a TLS segment is
        // an initialization image and its BSS part is never read.
        if is_tbss(ctx, id) {
            let mut addr2 = addr;
            loop {
                let hdr = ctx.chunk_header_mut(chunks[i]);
                addr2 = align_to(addr2, hdr.shdr.sh_addralign);
                hdr.shdr.sh_addr = addr2;
                addr2 += hdr.shdr.sh_size;
                if i + 2 == chunks.len() || !is_tbss(ctx, chunks[i + 1]) {
                    break;
                }
                i += 1;
            }
            i += 1;
            continue;
        }

        let hdr = ctx.chunk_header_mut(id);
        addr = align_to(addr, hdr.shdr.sh_addralign);
        hdr.shdr.sh_addr = addr;
        addr += hdr.shdr.sh_size;
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
                    if i != 0 {
                        let flags1 = chunks::to_phdr_flags(ctx, vec[i - 1]);
                        let flags2 = chunks::to_phdr_flags(ctx, vec[i]);
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
                    addr = align_to(addr, hdr.shdr.sh_addralign);
                    hdr.shdr.sh_addr = addr;
                    addr += hdr.shdr.sh_size;
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

/// The smallest N >= val with N % align == skew % align.
fn align_with_skew(val: u64, align: u64, skew: u64) -> u64 {
    val + (skew.wrapping_sub(val) & (align - 1))
}

fn set_file_offsets<E: Arch>(ctx: &mut Context<E>) -> u64 {
    let chunks = ctx.chunks.clone();
    let page_size = ctx.page_size;
    let mut fileoff = 0u64;
    let mut i = 0;

    while i < chunks.len() {
        let first = ctx.chunk_header(chunks[i]).shdr;
        if first.sh_flags & SHF_ALLOC as u64 == 0 {
            fileoff = align_to(fileoff, first.sh_addralign);
            ctx.chunk_header_mut(chunks[i]).shdr.sh_offset = fileoff;
            fileoff += first.sh_size;
            i += 1;
            continue;
        }
        if first.sh_type == SHT_NOBITS {
            ctx.chunk_header_mut(chunks[i]).shdr.sh_offset = fileoff;
            i += 1;
            continue;
        }

        if first.sh_addralign > page_size {
            fileoff = align_to(fileoff, first.sh_addralign);
        } else {
            fileoff = align_with_skew(fileoff, page_size, first.sh_addr);
        }

        // Allocated sections contiguous in memory get contiguous file
        // offsets.
        loop {
            let shdr = ctx.chunk_header(chunks[i]).shdr;
            ctx.chunk_header_mut(chunks[i]).shdr.sh_offset = fileoff + shdr.sh_addr - first.sh_addr;
            i += 1;
            if i >= chunks.len() {
                break;
            }
            let next = ctx.chunk_header(chunks[i]).shdr;
            if next.sh_flags & SHF_ALLOC as u64 == 0 || next.sh_type == SHT_NOBITS {
                break;
            }
            // With --section-start, addresses may not increase monotonically.
            if next.sh_addr < first.sh_addr {
                break;
            }
            let prev = ctx.chunk_header(chunks[i - 1]).shdr;
            // A section with a larger alignment needs offset % align == vaddr % align.
            if next.sh_addralign > page_size && next.sh_addralign > prev.sh_addralign {
                break;
            }
            // Don't allocate disk space for a large gap (--section-start).
            let gap = next.sh_addr - prev.sh_addr - prev.sh_size;
            if gap >= page_size {
                break;
            }
        }

        let last = ctx.chunk_header(chunks[i - 1]).shdr;
        fileoff = last.sh_offset + last.sh_size;

        while i < chunks.len() {
            let shdr = ctx.chunk_header(chunks[i]).shdr;
            if shdr.sh_flags & SHF_ALLOC as u64 == 0 || shdr.sh_type != SHT_NOBITS {
                break;
            }
            ctx.chunk_header_mut(chunks[i]).shdr.sh_offset = fileoff;
            i += 1;
        }
    }
    fileoff
}

/// Sets aside debug sections for `--separate-debug-file`.
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
    for id in ctx.chunks.clone() {
        chunks::update_shdr(ctx, id);
    }

    // Remove empty chunks.
    let chunks = std::mem::take(&mut ctx.chunks);
    ctx.chunks = chunks
        .into_iter()
        .filter(|&id| {
            matches!(
                id,
                ChunkId::Output(_) | ChunkId::GdbIndex | ChunkId::Placeholder(_)
            ) || ctx.chunk_header(id).shdr.sh_size != 0
        })
        .collect();

    // Assign section indices.
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
        sec.hdr.shdr.sh_link = ctx.symtab.hdr.shndx;
        ctx.symtab_shndx = Some(sec);
        ctx.chunks.push(ChunkId::SymtabShndx);
    }

    if let Some(shdr) = &mut ctx.shdr {
        shdr.hdr.shdr.sh_size = shndx as u64 * ElfShdr::size::<E>() as u64;
    }

    // Some section headers refer to other sections by index, so recompute
    // them now that indices are known.
    for id in ctx.chunks.clone() {
        chunks::update_shdr(ctx, id);
    }

    if let Some(symtab_shndx) = &mut ctx.symtab_shndx {
        let n = ctx.symtab.hdr.shdr.sh_size / ElfSym::size::<E>() as u64;
        symtab_shndx.hdr.shdr.sh_size = n * 4;
    }
}

/// Assigns addresses and file offsets, repeating until the program
/// header's size, which depends on the layout, converges.
pub fn set_osec_offsets<E: Arch>(ctx: &mut Context<E>) -> u64 {
    let _t = ctx.timer("set_osec_offsets");
    loop {
        if ctx.args.section_order.is_empty() {
            set_virtual_addresses_regular(ctx);
        } else {
            set_virtual_addresses_by_order(ctx);
        }

        if ctx.args.pack_dyn_relocs_android {
            let before = ctx.reldyn.hdr.shdr.sh_size;
            crate::chunks::dynamic::reldyn::update_shdr(ctx);
            if before != ctx.reldyn.hdr.shdr.sh_size {
                continue;
            }
        }
        ctx.checkpoint();

        let fileoff = set_file_offsets(ctx);
        if ctx.phdr.is_some() {
            let before = ctx.phdr.as_ref().unwrap().hdr.shdr.sh_size;
            chunks::update_phdr(ctx);
            if before < ctx.phdr.as_ref().unwrap().hdr.shdr.sh_size {
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
            if p.p_type == PT_LOAD && p.p_vaddr <= vaddr && vaddr < p.p_vaddr + p.p_memsz {
                return p.p_paddr + (vaddr - p.p_vaddr);
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
            let addr = ctx.chunk_header(chunk).shdr.sh_addr;
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
            s.value = (shdr.sh_addr + shdr.sh_size).wrapping_add(bias as u64);
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

    if let Some(bss) = find(ctx, b".bss") {
        start(ctx, ctx.syms.bss_start, Some(bss), 0);
    }

    if let Some(ehdr) = &ctx.ehdr {
        if ehdr.hdr.is_alloc() {
            let addr = ehdr.hdr.shdr.sh_addr;
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
        let addr = ctx.chunk_header(first).shdr.sh_addr;
        let s = &mut ctx.symbols[sym];
        s.set_output_chunk(first);
        s.value = addr;
    }

    // __rel_iplt_start/end are needed in a statically linked
    // non-relocatable executable, which has no .dynamic to find IFUNC
    // relocations by.
    if ctx.chunks.contains(&ChunkId::RelDyn) && ctx.args.is_static && !ctx.args.pie {
        let n = num_irelative_relocs(ctx) as i64 * ElfRel::size::<E>() as i64;
        stop(ctx, ctx.syms.rel_iplt_start, Some(ChunkId::RelDyn), -n);
        stop(ctx, ctx.syms.rel_iplt_end, Some(ChunkId::RelDyn), 0);
    } else {
        for sym in [ctx.syms.rel_iplt_start, ctx.syms.rel_iplt_end]
            .into_iter()
            .flatten()
        {
            ctx.symbols[sym].clear_origin();
        }
    }

    for &chunk in &sections {
        match ctx.chunk_header(chunk).shdr.sh_type {
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

    for &chunk in &sections {
        let shdr = ctx.chunk_header(chunk).shdr;
        if shdr.sh_flags & SHF_ALLOC as u64 != 0 {
            stop(ctx, ctx.syms.end_, Some(chunk), 0);
            stop(ctx, ctx.syms.end, Some(chunk), 0);
        }
        if shdr.sh_flags & SHF_EXECINSTR as u64 != 0 {
            stop(ctx, ctx.syms.etext_, Some(chunk), 0);
            stop(ctx, ctx.syms.etext, Some(chunk), 0);
        }
        if shdr.sh_type != SHT_NOBITS && shdr.sh_flags & SHF_ALLOC as u64 != 0 {
            stop(ctx, ctx.syms.edata_, Some(chunk), 0);
            stop(ctx, ctx.syms.edata, Some(chunk), 0);
        }
    }

    let dynamic = ctx.dynamic.as_ref().map(|_| ChunkId::Dynamic);
    start(ctx, ctx.syms.dynamic, dynamic, 0);

    // _GLOBAL_OFFSET_TABLE_ is the start of .got.plt on x86 for
    // compatibility, and of .got elsewhere.
    let got = if E::IS_X86 {
        ChunkId::GotPlt
    } else {
        ChunkId::Got
    };
    start(ctx, ctx.syms.global_offset_table, Some(got), 0);
    start(ctx, ctx.syms.procedure_linkage_table, Some(ChunkId::Plt), 0);

    if let (Some(sym), Some(first)) = (ctx.syms.tls_module_base, first) {
        let dtp = ctx.dtp_addr;
        let s = &mut ctx.symbols[sym];
        s.set_output_chunk(first);
        s.value = dtp;
    }

    let eh_frame_hdr = ctx.eh_frame_hdr.as_ref().map(|_| ChunkId::EhFrameHdr);
    start(ctx, ctx.syms.gnu_eh_frame_hdr, eh_frame_hdr, 0);

    if let Some(sym) = ctx.syms.global_pointer {
        match find(ctx, b".sdata") {
            Some(c) => start(ctx, Some(sym), Some(c), 0x800),
            None => start(ctx, Some(sym), first, 0),
        }
    }
    if ctx.syms.exidx_start.is_some() {
        if let Some(c) = find(ctx, b".ARM.exidx") {
            start(ctx, ctx.syms.exidx_start, Some(c), 0);
            stop(ctx, ctx.syms.exidx_end, Some(c), 0);
        }
    }
    if E::IS_PPC64 {
        if let Some(c) = find(ctx, b".got").or_else(|| find(ctx, b".toc")) {
            start(ctx, ctx.syms.toc, Some(c), 0x8000);
        } else if let (Some(sym), Some(first)) = (ctx.syms.toc, first) {
            let s = &mut ctx.symbols[sym];
            s.set_output_chunk(first);
            s.value = 0;
        }
    }

    // The register save and restore routines, unless an input defines them.
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

    for &chunk in &sections {
        if let Some(name) = start_stop_name(ctx, chunk) {
            let s = ctx.get_symbol(format!("__start_{name}").as_bytes());
            start(ctx, Some(s), Some(chunk), 0);
            let e = ctx.get_symbol(format!("__stop_{name}").as_bytes());
            stop(ctx, Some(e), Some(chunk), 0);

            if ctx.args.physical_image_base.is_some() {
                let shdr = ctx.chunk_header(chunk).shdr;
                let paddr = to_paddr(ctx, shdr.sh_addr);
                let x = ctx.get_symbol(format!("__phys_start_{name}").as_bytes());
                ctx.symbols[x].set_output_chunk(chunk);
                ctx.symbols[x].value = paddr;
                let y = ctx.get_symbol(format!("__phys_stop_{name}").as_bytes());
                ctx.symbols[y].set_output_chunk(chunk);
                ctx.symbols[y].value = paddr + shdr.sh_size;
            }
        }
    }

    // --defsym
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
    let targets: Vec<(usize, ChunkId)> = ctx
        .chunks
        .iter()
        .enumerate()
        .filter(|&(_, &id)| {
            let hdr = ctx.chunk_header(id);
            !hdr.is_alloc() && hdr.shdr.sh_size != 0 && hdr.name.starts_with(b".debug_")
        })
        .map(|(i, &id)| (i, id))
        .collect();
    let compressed: Vec<misc::CompressedSection> = {
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

/// Computes the build ID from the output file contents.
/// Writes the build ID. A hash-based ID is a hash of the output file:
/// BLAKE3 is a cryptographic hash function just like SHA256; we use it
/// instead of SHA256 because it's faster. The file is hashed in 4 MiB
/// shards in parallel, and the ID is the hash of the shards' hashes.
pub fn write_build_id<E: Arch>(ctx: &mut Context<E>, buf: &mut [u8], is_mmapped: bool) {
    let _t = ctx.timer("write_build_id");
    let contents: Vec<u8> = match ctx.args.build_id.kind {
        BuildIdKind::Hex => ctx.args.build_id.value.clone(),
        BuildIdKind::Hash => {
            const SHARD: usize = 4 * 1024 * 1024;
            let hashes: Vec<[u8; 32]> = buf
                .par_chunks_mut(SHARD)
                .enumerate()
                .map(|(i, shard)| {
                    let hash = *blake3::hash(shard).as_bytes();
                    // Make the kernel page out the file contents we've just
                    // written so that the subsequent close(2) call will
                    // become quicker.
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
            // UUIDv4 as defined by RFC 4122
            bytes[6] = (bytes[6] & 0x0f) | 0x40;
            bytes[8] = (bytes[8] & 0x3f) | 0x80;
            bytes.to_vec()
        }
        BuildIdKind::None => unreachable!(),
    };
    ctx.buildid.as_mut().unwrap().contents = contents;
    let hdr = ctx.buildid.as_ref().unwrap().hdr.shdr;
    misc::build_id::copy_buf(
        ctx,
        &mut buf[hdr.sh_offset as usize..(hdr.sh_offset + hdr.sh_size) as usize],
    );
}

/// Writes `.gnu_debuglink` with a CRC32 that the separate debug file will
/// be adjusted to match.
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
    misc::gnu_debuglink::copy_buf(
        ctx,
        &mut buf[hdr.sh_offset as usize..(hdr.sh_offset + hdr.sh_size) as usize],
    );
}

/// The CRC32 of a large buffer, computed in parallel.
fn crc32_parallel(buf: &[u8]) -> u32 {
    const SHARD: usize = 1024 * 1024;
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

/// Four bytes that, appended to data whose CRC32 is `current`, make its
/// CRC32 `desired`. ELF files ignore trailing bytes, so this is how a
/// debug file is given the checksum its executable recorded for it.
fn crc32_solve(current: u32, desired: u32) -> [u8; 4] {
    const POLY: u32 = 0xedb8_8320;
    let mut x = !desired;
    // Each iteration multiplies x by the inverse of x modulo the polynomial.
    for _ in 0..32 {
        x = x.rotate_left(1);
        x ^= (x & 1) * (POLY << 1);
    }
    (x ^ !current).to_le_bytes()
}

/// Writes the debug sections set aside by `separate_debug_sections` to a
/// separate file, whose name and checksum the main output recorded in
/// `.gnu_debuglink`.
pub fn write_separate_debug_file<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("write_separate_debug_file");

    let path = ctx.args.separate_debug_file.clone();
    let mut output = OutputFile::open_locked(&ctx.diag, &path, 0o666);

    // The main output is complete; writing the debug file can go on in
    // the background.
    if ctx.args.detach {
        crate::subprocess::notify_parent();
    }

    // A debug file has the same sections as the main file, but all except
    // the debug sections are empty, like bss. Replace them with
    // placeholders that keep their addresses and indices.
    let num_chunks = ctx.chunks.len();
    let debug_chunks = std::mem::take(&mut ctx.debug_chunks);
    ctx.chunks.extend(debug_chunks);
    for i in 0..num_chunks {
        let id = ctx.chunks[i];
        if id.is_header()
            || id == ChunkId::Shstrtab
            || ctx.chunk_header(id).shdr.sh_type == SHT_NOTE
        {
            continue;
        }
        let hdr = ctx.chunk_header(id);
        let mut placeholder = ChunkHeader::with_name(hdr.name, SHT_NOBITS, hdr.shdr.sh_flags);
        placeholder.shdr = ElfShdr {
            sh_type: SHT_NOBITS,
            ..hdr.shdr
        };
        placeholder.shndx = hdr.shndx;
        ctx.placeholders.push(placeholder);
        ctx.chunks[i] = ChunkId::Placeholder(ctx.placeholders.len() as u32 - 1);
    }

    let new_chunks = ctx.chunks[num_chunks..].to_vec();
    for id in new_chunks {
        chunks::compute_section_size(ctx, id);
    }
    sort_debug_info_sections(ctx);
    if ctx.args.compress_debug_sections != ELFCOMPRESS_NONE {
        compress_debug_sections(ctx);
    }
    compute_section_headers(ctx);

    let page_size = ctx.page_size;
    let mut fileoff = 0;
    for id in ctx.chunks.clone() {
        let shdr = &mut ctx.chunk_header_mut(id).shdr;
        if shdr.sh_type == SHT_NOBITS {
            shdr.sh_offset = fileoff;
        } else if shdr.sh_flags & SHF_ALLOC as u64 != 0 {
            fileoff = align_with_skew(fileoff, page_size, shdr.sh_addr);
            shdr.sh_offset = fileoff;
            fileoff += shdr.sh_size;
        } else {
            fileoff = align_to(fileoff, shdr.sh_addralign);
            shdr.sh_offset = fileoff;
            fileoff += shdr.sh_size;
        }
    }

    // The program header keeps its size, since the placeholders' addresses
    // were laid out around it.
    if let Some(n) = ctx.phdr.as_ref().map(|p| p.phdrs.len()) {
        chunks::update_phdr(ctx);
        let phdr = ctx.phdr.as_mut().unwrap();
        phdr.phdrs.resize(n, ElfPhdr::default());
        phdr.hdr.shdr.sh_size = (n * ElfPhdr::size::<E>()) as u64;
    }

    output.resize(&ctx.diag, fileoff);
    crate::driver::copy_chunks(ctx, output.buf());

    if ctx.gdb_index.is_some() {
        crate::gdb_index::build_tables_now(ctx);
        crate::gdb_index::write(ctx, &mut output);
    }

    let trailer = crc32_solve(
        crc32_parallel(output.buf()),
        ctx.gnu_debuglink.as_ref().unwrap().crc32,
    );
    let len = output.len();
    output.extend(&ctx.diag, trailer.len());
    output.buf()[len..].copy_from_slice(&trailer);
    output.close(&ctx.diag);
}

/// Writes Makefile-style dependency rules, like the compiler's -M.
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
            .map(crate::chunks::merged::num_fragments)
            .sum(),
    );
}

/// Whether `--section-order` names a section that doesn't exist.
pub fn align_down_to_page<E: Arch>(ctx: &Context<E>, addr: u64) -> u64 {
    align_down(addr, ctx.page_size)
}
