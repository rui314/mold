//! The linker context: everything about one link, from parsed arguments
//! to output chunks.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fmt;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use crate::arch::Arch;
use crate::chunks::build_id::BuildIdSection;
use crate::chunks::comdat_group::ComdatGroupSection;
use crate::chunks::compressed::CompressedSection;
use crate::chunks::copyrel::CopyrelSection;
use crate::chunks::dynstr::DynstrSection;
use crate::chunks::dynsym::DynsymSection;
use crate::chunks::eh_frame_hdr::EhFrameHdrSection;
use crate::chunks::gnu_debuglink::GnuDebuglinkSection;
use crate::chunks::gnu_hash::GnuHashSection;
use crate::chunks::got::GotSection;
use crate::chunks::merged::{MergedSection, MergedSectionId};
use crate::chunks::note_property::NotePropertySection;
use crate::chunks::output_section::OutputSection;
use crate::chunks::plt::PltSection;
use crate::chunks::pltgot::PltGotSection;
use crate::chunks::reldyn::RelDynSection;
use crate::chunks::reloc::RelocSection;
use crate::chunks::riscv_attributes::RiscvAttributesSection;
use crate::chunks::sframe::SFrameSection;
use crate::chunks::verdef::VerdefSection;
use crate::chunks::verneed::VerneedSection;
use crate::chunks::versym::VersymSection;
use crate::chunks::{self, ChunkHeader, ChunkId, OutputPhdr, OutputSectionId};
use crate::cmdline::{Args, ReaderContext};
use crate::elf::ElfWord;
use crate::input_files::{FileId, FileList, InputFile, ObjId, ObjectFile, SharedFile};
use crate::input_sections::{
    FragmentRef, InputSection, InputSectionId, SectionFragment, SectionRef,
};
use crate::linker_script::{DynamicPattern, VersionPattern};
use crate::mapped_file::MappedFile;
use crate::symbol::{Bins, Symbol, SymbolChunkId, SymbolId, SymbolSlot, SymbolTable};
use crate::util::perf::Timers;
use crate::util::worker_local::WorkerLocal;

// Keep immutable and mutable chunk lookup in the same static match.
macro_rules! chunk_header {
    ($ctx:ident, $id:ident, $borrow:ident $(, $mutable:tt)?) => {
        match $id {
            ChunkId::Ehdr => $ctx.ehdr.$borrow().expect("chunk does not exist"),
            ChunkId::Phdr => &$($mutable)? $ctx.phdr.$borrow().expect("chunk does not exist").hdr,
            ChunkId::Shdr => $ctx.shdr.$borrow().expect("chunk does not exist"),
            ChunkId::Interp => $ctx.interp.$borrow().expect("chunk does not exist"),
            ChunkId::Got => &$($mutable)? $ctx.got.hdr,
            ChunkId::GotPlt => &$($mutable)? $ctx.gotplt,
            ChunkId::RelPlt => &$($mutable)? $ctx.relplt,
            ChunkId::RelDyn => &$($mutable)? $ctx.reldyn.hdr,
            ChunkId::RelrDyn => $ctx.relrdyn.$borrow().expect("chunk does not exist"),
            ChunkId::Dynamic => $ctx.dynamic.$borrow().expect("chunk does not exist"),
            ChunkId::Strtab => &$($mutable)? $ctx.strtab,
            ChunkId::Dynstr => &$($mutable)? $ctx.dynstr.hdr,
            ChunkId::Hash => $ctx.hash.$borrow().expect("chunk does not exist"),
            ChunkId::GnuHash => &$($mutable)? $ctx.gnu_hash.$borrow().expect("chunk does not exist").hdr,
            ChunkId::GnuDebuglink => &$($mutable)? $ctx.gnu_debuglink.$borrow().expect("chunk does not exist").hdr,
            ChunkId::Shstrtab => $ctx.shstrtab.$borrow().expect("chunk does not exist"),
            ChunkId::Plt => &$($mutable)? $ctx.plt.hdr,
            ChunkId::PltGot => &$($mutable)? $ctx.pltgot.hdr,
            ChunkId::Symtab => &$($mutable)? $ctx.symtab,
            ChunkId::SymtabShndx => $ctx.symtab_shndx.$borrow().expect("chunk does not exist"),
            ChunkId::Dynsym => &$($mutable)? $ctx.dynsym.hdr,
            ChunkId::EhFrame => &$($mutable)? $ctx.eh_frame,
            ChunkId::EhFrameHdr => &$($mutable)? $ctx.eh_frame_hdr.$borrow().expect("chunk does not exist").hdr,
            ChunkId::EhFrameReloc => $ctx.eh_frame_reloc.$borrow().expect("chunk does not exist"),
            ChunkId::SFrame => &$($mutable)? $ctx.sframe.hdr,
            ChunkId::SFrameReloc => $ctx.sframe_reloc.$borrow().expect("chunk does not exist"),
            ChunkId::Copyrel => &$($mutable)? $ctx.copyrel.hdr,
            ChunkId::CopyrelRelro => &$($mutable)? $ctx.copyrel_relro.hdr,
            ChunkId::Versym => &$($mutable)? $ctx.versym.hdr,
            ChunkId::Verneed => &$($mutable)? $ctx.verneed.hdr,
            ChunkId::Verdef => &$($mutable)? $ctx.verdef.$borrow().expect("chunk does not exist").hdr,
            ChunkId::BuildId => &$($mutable)? $ctx.buildid.$borrow().expect("chunk does not exist").hdr,
            ChunkId::NotePackage => &$($mutable)? $ctx.note_package,
            ChunkId::NoteProperty => &$($mutable)? $ctx.note_property.$borrow().expect("chunk does not exist").hdr,
            ChunkId::RiscvAttributes => &$($mutable)? $ctx.riscv_attributes.$borrow().expect("chunk does not exist").hdr,
            ChunkId::ArmExidx => &$($mutable)? $ctx.arm_exidx.$borrow().expect("chunk does not exist").hdr,
            ChunkId::Ppc64SaveRestore => $ctx
                .ppc64_save_restore
                .$borrow()
                .expect("chunk does not exist"),
            ChunkId::Ppc64Opd => &$($mutable)? $ctx.ppc64_opd.$borrow().expect("chunk does not exist").hdr,
            ChunkId::GdbIndex => $ctx.gdb_index.$borrow().expect("chunk does not exist"),
            ChunkId::RelroPadding => $ctx.relro_padding.$borrow().expect("chunk does not exist"),
            ChunkId::Output(id) => &$($mutable)? $ctx.output_sections[id.index()].hdr,
            ChunkId::Merged(id) => &$($mutable)? $ctx.merged_sections[id.index()].hdr,
            ChunkId::Reloc(i) => &$($mutable)? $ctx.reloc_sections[i as usize].hdr,
            ChunkId::ComdatGroup(i) => &$($mutable)? $ctx.comdat_group_sections[i as usize].hdr,
            ChunkId::Compressed(i) => &$($mutable)? $ctx.compressed_sections[i as usize].hdr,
            ChunkId::Placeholder(i) => &$($mutable)? $ctx.placeholders[i as usize],
        }
    };
}

/// Linker-synthesized symbols with well-known names.
#[derive(Debug, Default)]
pub struct SyntheticSymbols {
    pub dynamic: Option<SymbolId>,
    pub global_offset_table: Option<SymbolId>,
    pub procedure_linkage_table: Option<SymbolId>,
    pub tls_module_base: Option<SymbolId>,
    pub gnu_eh_frame_hdr: Option<SymbolId>,
    pub bss_start: Option<SymbolId>,
    pub dso_handle: Option<SymbolId>,
    pub ehdr_start: Option<SymbolId>,
    pub executable_start: Option<SymbolId>,
    pub exidx_end: Option<SymbolId>,
    pub exidx_start: Option<SymbolId>,
    pub fini_array_end: Option<SymbolId>,
    pub fini_array_start: Option<SymbolId>,
    pub global_pointer: Option<SymbolId>,
    pub init_array_end: Option<SymbolId>,
    pub init_array_start: Option<SymbolId>,
    pub preinit_array_end: Option<SymbolId>,
    pub preinit_array_start: Option<SymbolId>,
    pub rel_iplt_end: Option<SymbolId>,
    pub rel_iplt_start: Option<SymbolId>,
    pub edata_: Option<SymbolId>,
    pub end_: Option<SymbolId>,
    pub etext_: Option<SymbolId>,
    pub edata: Option<SymbolId>,
    pub end: Option<SymbolId>,
    pub etext: Option<SymbolId>,
    pub toc: Option<SymbolId>,
    pub tls_get_addr: Option<SymbolId>,

    /// The entry point, `-init` and `-fini` symbols.
    pub entry: SymbolId,
    pub init: SymbolId,
    pub fini: SymbolId,
}

// Context contains the state for one linker invocation: command-line options,
// input and output files, symbols, sections and target-specific data.
pub struct Context<E: Arch> {
    // Command-line arguments
    pub args: Args,

    // Fully-expanded command line args
    pub cmdline_args: Arc<[Cow<'static, OsStr>]>,
    pub timers: Timers,

    // Symbol table. Input file parsing records global symbols and their
    // SymbolId slots in worker bins; gather_symbols() interns those names
    // and fills the slots. Linker-synthesized symbols are interned directly.
    pub symbols: SymbolTable,

    // Append-only registry for synthetic symbol origins. The output chunk
    // list can be reordered, and the context and chunk vectors can move.
    symbol_chunks: Vec<ChunkId>,

    /// Global symbol keys recorded while input files are parsed, one bin per
    /// Rayon worker plus one for callers outside the pool.
    symbol_bins: OnceLock<WorkerLocal<Bins<SymbolSlot>>>,

    pub objs: FileList<ObjectFile<E>>,
    pub dsos: FileList<SharedFile<E>>,

    // Reader context

    // Input files with their command line positions, in the
    // nondeterministic order in which the parallel file reader found
    // them. read_input_files() sorts them by position to construct
    // `objs` and `dsos`.
    pub pending_files: Vec<(Vec<u32>, FileId)>,

    // Deferred IR files with their reader contexts and archive names.
    // read_input_files() hands them to the LTO plugin in command line
    // order once all input files have been found.
    pub lto_jobs: Mutex<Vec<(ReaderContext, &'static MappedFile, &'static Path)>>,

    /// Sonames of all shared libraries given to the linker, including
    /// ones later dropped as unneeded; --no-allow-shlib-undefined can
    /// only be checked if the set of libraries is complete.
    pub dso_sonames: HashSet<&'static [u8]>,

    pub lto_file_priority: u32,

    pub internal_obj: Option<ObjId>,

    pub output_sections: Vec<OutputSection<E>>,
    pub merged_sections: Vec<MergedSection<E>>,
    pub reloc_sections: Vec<RelocSection<E>>,
    pub comdat_group_sections: Vec<ComdatGroupSection<E>>,
    pub compressed_sections: Vec<CompressedSection<E>>,
    pub placeholders: Vec<ChunkHeader<E>>,

    /// The chunks that make up the output, in file order.
    pub chunks: Vec<ChunkId>,

    // For --separate-debug-file
    pub debug_chunks: Vec<ChunkId>,

    pub ehdr: Option<ChunkHeader<E>>,
    pub phdr: Option<OutputPhdr<E>>,
    pub shdr: Option<ChunkHeader<E>>,
    pub interp: Option<ChunkHeader<E>>,
    pub got: GotSection<E>,
    pub gotplt: ChunkHeader<E>,
    pub relplt: ChunkHeader<E>,
    pub reldyn: RelDynSection<E>,
    pub relrdyn: Option<ChunkHeader<E>>,
    pub dynamic: Option<ChunkHeader<E>>,
    pub strtab: ChunkHeader<E>,
    pub dynstr: DynstrSection<E>,
    pub hash: Option<ChunkHeader<E>>,
    pub gnu_hash: Option<GnuHashSection<E>>,
    pub gnu_debuglink: Option<GnuDebuglinkSection<E>>,
    pub shstrtab: Option<ChunkHeader<E>>,
    pub plt: PltSection<E>,
    pub pltgot: PltGotSection<E>,
    pub symtab: ChunkHeader<E>,
    pub symtab_shndx: Option<ChunkHeader<E>>,
    pub dynsym: DynsymSection<E>,
    pub eh_frame: ChunkHeader<E>,
    pub eh_frame_hdr: Option<EhFrameHdrSection<E>>,
    pub eh_frame_reloc: Option<ChunkHeader<E>>,
    pub sframe: SFrameSection<E>,
    pub sframe_reloc: Option<ChunkHeader<E>>,
    pub copyrel: CopyrelSection<E>,
    pub copyrel_relro: CopyrelSection<E>,
    pub versym: VersymSection<E>,
    pub verneed: VerneedSection<E>,
    pub verdef: Option<VerdefSection<E>>,
    pub buildid: Option<BuildIdSection<E>>,
    pub note_package: ChunkHeader<E>,

    // Target-specific context members
    pub note_property: Option<NotePropertySection<E>>,
    pub riscv_attributes: Option<RiscvAttributesSection<E>>,
    pub arm_exidx: Option<crate::chunks::arm_exidx::ArmExidxSection<E>>,
    pub ppc64_save_restore: Option<ChunkHeader<E>>,
    pub ppc64_opd: Option<crate::chunks::opd::Ppc64OpdSection<E>>,
    /// Whether any input uses Power10 PC-relative calls, which decides
    /// how thunks address their targets.
    pub is_power10: AtomicBool,
    pub gdb_index: Option<ChunkHeader<E>>,

    // Partially built .gdb_index data passed between its background stages.
    pub gdb_index_data: Option<crate::gdb_index::GdbIndexData>,
    pub relro_padding: Option<ChunkHeader<E>>,
    pub comment: Option<MergedSectionId>,

    pub needs_tlsld: AtomicBool,
    pub has_textrel: AtomicBool,

    pub undef_errors: Mutex<HashMap<SymbolId, Vec<String>>>,

    pub version_patterns: Vec<VersionPattern>,
    pub dynamic_list_patterns: Vec<DynamicPattern>,
    pub default_version: u16,

    // For thread-local variables
    pub tls_begin: u64,
    pub tp_addr: u64,
    pub dtp_addr: u64,

    pub syms: SyntheticSymbols,
}

impl<E: Arch> Context<E> {
    pub fn new(mut args: Args, cmdline_args: impl Into<Arc<[Cow<'static, OsStr>]>>) -> Context<E> {
        let mut symbols = SymbolTable::new();
        let syms = SyntheticSymbols {
            entry: symbols.intern(crate::util::leak_bytes(std::mem::take(&mut args.entry))),
            init: symbols.intern(crate::util::leak_bytes(std::mem::take(&mut args.init))),
            fini: symbols.intern(crate::util::leak_bytes(std::mem::take(&mut args.fini))),
            ..SyntheticSymbols::default()
        };
        let timers = if args.perf {
            Timers::new()
        } else {
            Timers::disabled()
        };

        Context {
            reldyn: RelDynSection::<E>::new(&args),
            got: GotSection::<E>::new(),
            gotplt: chunks::gotplt::new_header::<E>(&args),
            relplt: chunks::relplt::new_header::<E>(),
            strtab: chunks::strtab::new_header(),
            dynstr: DynstrSection::new(),
            plt: PltSection::<E>::new(),
            pltgot: PltGotSection::new(),
            symtab: chunks::symtab::new_header::<E>(),
            dynsym: DynsymSection::<E>::new(),
            eh_frame: chunks::eh_frame::new_header::<E>(),
            sframe: SFrameSection::<E>::new(),
            copyrel: CopyrelSection::new(false),
            copyrel_relro: CopyrelSection::new(true),
            versym: VersymSection::new(),
            verneed: VerneedSection::new(),
            note_package: chunks::note_package::new_header(),
            args,
            cmdline_args: cmdline_args.into(),
            timers,
            symbols,
            symbol_chunks: Vec::new(),
            symbol_bins: OnceLock::new(),
            objs: FileList::default(),
            dsos: FileList::default(),
            pending_files: Vec::new(),
            lto_jobs: Mutex::new(Vec::new()),
            dso_sonames: HashSet::new(),
            lto_file_priority: 100,
            internal_obj: None,
            output_sections: Vec::new(),
            merged_sections: Vec::new(),
            reloc_sections: Vec::new(),
            comdat_group_sections: Vec::new(),
            compressed_sections: Vec::new(),
            placeholders: Vec::new(),
            chunks: Vec::new(),
            debug_chunks: Vec::new(),
            ehdr: None,
            phdr: None,
            shdr: None,
            interp: None,
            relrdyn: None,
            dynamic: None,
            hash: None,
            gnu_hash: None,
            gnu_debuglink: None,
            shstrtab: None,
            symtab_shndx: None,
            eh_frame_hdr: None,
            eh_frame_reloc: None,
            sframe_reloc: None,
            verdef: None,
            buildid: None,
            note_property: None,
            riscv_attributes: None,
            arm_exidx: None,
            ppc64_save_restore: None,
            ppc64_opd: None,
            is_power10: AtomicBool::new(false),
            gdb_index: None,
            gdb_index_data: None,
            relro_padding: None,
            comment: None,
            needs_tlsld: AtomicBool::new(false),
            has_textrel: AtomicBool::new(false),
            undef_errors: Mutex::new(HashMap::new()),
            version_patterns: Vec::new(),
            dynamic_list_patterns: Vec::new(),
            default_version: crate::elf::VER_NDX_UNSPECIFIED as u16,
            tls_begin: 0,
            tp_addr: 0,
            dtp_addr: 0,
            syms,
        }
    }

    /// Returns this worker's symbol bin. Looking it up once per file keeps the
    /// synchronization cost outside the per-symbol loop.
    pub(crate) fn symbol_bin(&self) -> MutexGuard<'_, Bins<SymbolSlot>> {
        self.symbol_bins
            .get_or_init(|| WorkerLocal::new(Bins::new))
            .get()
    }

    /// Takes all keys recorded since the previous gather, leaving empty bins
    /// in place for an LTO output file to use before the second gather.
    pub(crate) fn take_symbol_bins(&mut self) -> Vec<Bins<SymbolSlot>> {
        self.symbol_bins
            .get_mut()
            .map(|bins| bins.take().collect())
            .unwrap_or_default()
    }

    pub fn get_symbol(&mut self, name: &[u8]) -> SymbolId {
        self.symbols.get_or_intern(name)
    }

    /// The common part of a file.
    pub fn file(&self, id: FileId) -> &InputFile<E> {
        match id {
            FileId::Obj(id) => &self.objs[id.index()].base,
            FileId::Dso(id) => &self.dsos[id.index()].base,
        }
    }

    /// Formats a file for diagnostics.
    pub fn file_display(&self, id: FileId) -> &dyn fmt::Display {
        match id {
            FileId::Obj(id) => &self.objs[id.index()],
            FileId::Dso(id) => &self.dsos[id.index()],
        }
    }

    pub fn section(&self, r: SectionRef) -> &InputSection<E> {
        self.objs[r.file.index()].section_at(r.shndx)
    }

    #[inline]
    pub fn input_section(&self, id: InputSectionId) -> &InputSection<E> {
        // SAFETY: InputSectionIds are created only when a section is inserted
        // into the stable file pool and that file's dense section vector.
        let file = unsafe { self.objs.get_unchecked(id.file().index()) };
        unsafe { file.sections.input_unchecked(id.index()) }
    }

    /// Formats an input section for diagnostics.
    pub fn section_display(&self, r: SectionRef) -> impl fmt::Display + '_ {
        let file = &self.objs[r.file.index()];
        file.section_at(r.shndx).display(file)
    }

    /// Formats an input section for diagnostics.
    pub fn input_section_display(&self, id: InputSectionId) -> impl fmt::Display + '_ {
        let isec = self.input_section(id);
        isec.display(&self.objs[isec.file.index()])
    }

    pub fn fragment(&self, r: FragmentRef) -> &SectionFragment {
        let msec = &self.merged_sections[r.section.index()];
        msec.fragments.get(r.entry)
    }

    pub fn fragment_addr(&self, r: FragmentRef) -> u64 {
        let msec = &self.merged_sections[r.section.index()];
        msec.hdr.shdr.sh_addr.get() + msec.fragments.get(r.entry).offset()
    }

    pub fn output_section(&self, id: OutputSectionId) -> &OutputSection<E> {
        &self.output_sections[id.index()]
    }

    /// The address of a PPC64 ELFv1 function descriptor.
    pub fn opd_addr(&self, idx: u32) -> u64 {
        let opd = self
            .ppc64_opd
            .as_ref()
            .expect("PPC64 ELFv1 has an .opd section");
        opd.hdr.shdr.sh_addr.get() + idx as u64 * crate::chunks::opd::ENTRY_SIZE
    }

    /// Whether the file is the internal object holding synthesized symbols.
    pub fn is_internal(&self, id: ObjId) -> bool {
        self.internal_obj == Some(id)
    }

    /// Associates a synthetic symbol with an output chunk of this context.
    pub fn set_symbol_output_chunk(&mut self, sym: SymbolId, chunk: ChunkId) -> &mut Symbol {
        let id = SymbolChunkId(
            u32::try_from(self.symbol_chunks.len()).expect("too many symbol output chunks"),
        );
        self.symbol_chunks.push(chunk);
        let sym = &mut self.symbols[sym];
        sym.set_output_chunk(id);
        sym
    }

    pub(crate) fn symbol_chunk_header(&self, id: SymbolChunkId) -> &ChunkHeader<E> {
        self.chunk_header(self.symbol_chunks[id.0 as usize])
    }

    /// The header of any chunk. Panics if the chunk does not exist.
    pub fn chunk_header(&self, id: ChunkId) -> &ChunkHeader<E> {
        chunk_header!(self, id, as_ref)
    }

    pub fn chunk_header_mut(&mut self, id: ChunkId) -> &mut ChunkHeader<E> {
        chunk_header!(self, id, as_mut, mut)
    }

    /// Finds the first chunk of a section type.
    pub fn find_chunk_by_type(&self, sh_type: u32) -> Option<ChunkId> {
        self.chunks
            .iter()
            .copied()
            .find(|&c| self.chunk_header(c).shdr.sh_type.get() == sh_type)
    }

    /// Finds the first chunk with a name.
    pub fn find_chunk_by_name(&self, name: &[u8]) -> Option<ChunkId> {
        self.chunks
            .iter()
            .copied()
            .find(|&c| self.chunk_header(c).name == name)
    }

    /// Starts a `--perf` timer for a pass.
    pub fn timer(&self, name: &str) -> crate::util::perf::Timer {
        self.timers.start(name)
    }
}

impl<E: Arch> fmt::Debug for Context<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Context<{}>", E::NAME)
    }
}
