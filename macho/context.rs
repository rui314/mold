//! The linker context: everything about one link, from parsed arguments
//! to output chunks.

use std::marker::PhantomData;

use crate::arch::Target;
use crate::chunks::bind_info::BindInfoSection;
use crate::chunks::chain_starts::ChainStartsSection;
use crate::chunks::chained_fixups::ChainedFixupsSection;
use crate::chunks::code_signature::CodeSignatureSection;
use crate::chunks::data_in_code::DataInCodeSection;
use crate::chunks::delay_init::DelayInit;
use crate::chunks::eh_frame::EhFrameSection;
use crate::chunks::export_trie::ExportTrieSection;
use crate::chunks::extern_relocs::ExternRelocsSection;
use crate::chunks::function_starts::FunctionStartsSection;
use crate::chunks::got::GotSection;
use crate::chunks::indirect_symtab::IndirectSymtabSection;
use crate::chunks::init_offsets::InitOffsetsSection;
use crate::chunks::lazy_bind_info::LazyBindInfoSection;
use crate::chunks::lazy_helpers::LazyHelpersSection;
use crate::chunks::lazy_load_got::LazyLoadGotSection;
use crate::chunks::lazy_load_info::LazyLoadInfoSection;
use crate::chunks::lazy_ptrs::LazyPtrsSection;
use crate::chunks::local_relocs::LocalRelocsSection;
use crate::chunks::objc_imageinfo::ObjcImageInfoSection;
use crate::chunks::objc_methlist::ObjcMethlistSection;
use crate::chunks::objc_stubs::ObjcStubsSection;
use crate::chunks::rebase_info::RebaseInfoSection;
use crate::chunks::sectcreate::{SectCreateInput, SectCreateSection};
use crate::chunks::split_info::SplitInfoSection;
use crate::chunks::strtab::StrtabSection;
use crate::chunks::stub_helper::StubHelperSection;
use crate::chunks::stubs::StubsSection;
use crate::chunks::symtab::SymtabSection;
use crate::chunks::unwind_info::UnwindInfoSection;
use crate::chunks::weak_bind_info::WeakBindInfoSection;
use crate::chunks::{
    ChunkHeader, ChunkId, OutputMachHeader, OutputSection, OutputSectionId, OutputSegment,
};
use crate::cmdline::Args;
use crate::input_files::{DylibFile, FileId, ObjectFile};
use crate::symbol::{SymbolId, SymbolTable};
use crate::util::perf::Timers;

// Keep immutable and mutable chunk lookup in the same static match.
macro_rules! chunk_header {
    ($ctx:ident, $id:ident $(, $mutable:tt)?) => {
        match $id {
            ChunkId::MachHeader => &$($mutable)? $ctx.mach_header.hdr,
            ChunkId::Output(id) => &$($mutable)? $ctx.output_sections[id.index()].hdr,
            ChunkId::Stubs => &$($mutable)? $ctx.stubs.hdr,
            ChunkId::StubHelper => &$($mutable)? $ctx.stub_helper.hdr,
            ChunkId::LazyPtrs => &$($mutable)? $ctx.lazy_ptrs.hdr,
            ChunkId::Got => &$($mutable)? $ctx.got.hdr,
            ChunkId::DelayStubs => &$($mutable)? $ctx.delay_init.stubs_hdr,
            ChunkId::DelayHelper => &$($mutable)? $ctx.delay_init.helper_hdr,
            ChunkId::LazyHelpers => &$($mutable)? $ctx.lazy_helpers.hdr,
            ChunkId::LazyLoadGot => &$($mutable)? $ctx.lazy_load_got.hdr,
            ChunkId::ObjcStubs => &$($mutable)? $ctx.objc_stubs.hdr,
            ChunkId::ObjcMethlist => &$($mutable)? $ctx.objc_methlist.hdr,
            ChunkId::ObjcImageInfo => &$($mutable)? $ctx.objc_imageinfo.hdr,
            ChunkId::SectCreate(i) => &$($mutable)? $ctx.sectcreate_sections[i as usize].hdr,
            ChunkId::InitOffsets => &$($mutable)? $ctx.init_offsets.hdr,
            ChunkId::ChainStarts => &$($mutable)? $ctx.chain_starts.hdr,
            ChunkId::UnwindInfo => &$($mutable)? $ctx.unwind_info.hdr,
            ChunkId::EhFrame => &$($mutable)? $ctx.eh_frame.hdr,
            ChunkId::RebaseInfo => &$($mutable)? $ctx.rebase_info.hdr,
            ChunkId::BindInfo => &$($mutable)? $ctx.bind_info.hdr,
            ChunkId::WeakBindInfo => &$($mutable)? $ctx.weak_bind_info.hdr,
            ChunkId::LazyBindInfo => &$($mutable)? $ctx.lazy_bind_info.hdr,
            ChunkId::ChainedFixups => &$($mutable)? $ctx.chained_fixups.hdr,
            ChunkId::ExportTrie => &$($mutable)? $ctx.export_trie.hdr,
            ChunkId::FunctionStarts => &$($mutable)? $ctx.function_starts.hdr,
            ChunkId::DataInCode => &$($mutable)? $ctx.data_in_code.hdr,
            ChunkId::MergeableRecord => &$($mutable)? $ctx.mergeable_record.hdr,
            ChunkId::SplitInfo => &$($mutable)? $ctx.split_info.hdr,
            ChunkId::LazyLoadInfo => &$($mutable)? $ctx.lazy_load_info.hdr,
            ChunkId::LocalRelocs => &$($mutable)? $ctx.local_relocs.hdr,
            ChunkId::ExternRelocs => &$($mutable)? $ctx.extern_relocs.hdr,
            ChunkId::IndirectSymtab => &$($mutable)? $ctx.indirect_symtab.hdr,
            ChunkId::Symtab => &$($mutable)? $ctx.symtab.hdr,
            ChunkId::Strtab => &$($mutable)? $ctx.strtab.hdr,
            ChunkId::CodeSignature => &$($mutable)? $ctx.code_signature.hdr,
        }
    };
}

pub struct Context<E: Target> {
    pub args: Args,
    pub objs: Vec<ObjectFile>,
    pub dylibs: Vec<DylibFile>,
    pub symbols: SymbolTable,
    /// All input sections, in one arena.
    pub isecs: crate::input_sections::InputSections,
    /// The object that owns the linker-synthesized sections and
    /// symbols, once created; mold's internal_obj.
    pub internal_obj: Option<usize>,
    /// Input-order counter for resolution tie-breaking.
    pub priority_counter: u32,
    /// The first priority of the files auto-link options brought in,
    /// which come after the libraries the command line names and the
    /// ones they re-export (see passes::dylib_ranks); u32::MAX before
    /// auto-linking.
    pub autolink_priority: u32,
    /// Files already loaded, so a library named twice (command line
    /// plus auto-link) is read once.
    pub visited_files: std::collections::HashSet<std::path::PathBuf>,
    /// Under -dependency_info, the files of the libraries loaded as
    /// another's re-exports, as found (see load_reexports).
    pub reexport_files: Vec<std::path::PathBuf>,
    /// Under -dependency_info, the files a search for an input looked
    /// for and did not find (see reader::Prober).
    pub missing_files: std::sync::Mutex<Vec<std::path::PathBuf>>,
    /// The loaded libLTO, once a bitcode input has been seen.
    pub lto_plugin: Option<crate::lto::Plugin>,
    /// Bitcode modules registered for LTO.
    pub lto_modules: Vec<crate::lto::BitcodeModule>,
    /// The objects LTO compiled the live bitcode modules to: one per
    /// ThinLTO module, in input order, then the merged modules' one.
    pub lto_objs: std::ops::Range<usize>,
    /// The bitcode files LTO compiled, in input order.
    pub lto_inputs: Vec<crate::lto::LtoInput>,
    /// Auto-link options already acted on.
    pub processed_linker_options: std::collections::HashSet<Vec<Vec<u8>>>,
    /// -add_linker_option's auto-link options, once read as an
    /// object's are (see reader::read_linker_options).
    pub cmdline_linker_options: Option<Vec<Vec<Vec<u8>>>>,
    /// The libraries and frameworks auto-link options named that were
    /// not found, or that don't take this link as a client: reported if
    /// symbols stay undefined.
    pub autolink_misses: Vec<crate::error::Message>,
    /// The files only -possible-l and the like name, which load with
    /// the auto-linked libraries (see reader::load_autolink_deps).
    pub possible_files: Vec<std::path::PathBuf>,
    /// The files -dylib_file names for re-exported libraries that are
    /// no libraries, to load as inputs (see collect_indirect_files).
    pub indirect_files: Vec<&'static crate::mapped_file::MappedFile>,
    /// The dylibs the mergeable dylibs merged into the image link, by
    /// their recorded identities, to add after the command line's (see
    /// reader::add_merged_dependencies).
    pub merged_dependencies: Vec<crate::mergeable::Dependency>,
    /// The mergeable dylibs merged into the image, which are none of
    /// its dependencies, in input order.
    pub merged_libraries: Vec<crate::mergeable::MergedLibrary>,
    /// The hook for the classes of the mergeable libraries merged or
    /// re-exported, and the classes it is for.
    pub bundle_hook: crate::bundle_hook::BundleHook,
    /// Unwind records from all objects' __compact_unwind sections.
    pub unwind_records: Vec<crate::input_sections::UnwindRecord>,
    /// DWARF CIEs and FDEs from all objects' __eh_frame sections.
    pub cies: Vec<crate::input_sections::CieRecord>,
    pub fdes: Vec<crate::input_sections::FdeRecord>,
    /// The chunks of the output, in file order, and the segments they
    /// are grouped into. Each chunk's header and data live in the
    /// typed field for its kind below, as in mold.
    pub chunks: Vec<ChunkId>,
    pub segments: Vec<OutputSegment>,
    pub mach_header: OutputMachHeader,
    pub output_sections: Vec<OutputSection>,
    pub stubs: StubsSection,
    pub stub_helper: StubHelperSection,
    pub lazy_ptrs: LazyPtrsSection,
    pub got: GotSection,
    pub delay_init: DelayInit,
    pub lazy_helpers: LazyHelpersSection,
    pub lazy_load_got: LazyLoadGotSection,
    pub objc_stubs: ObjcStubsSection,
    pub objc_methlist: ObjcMethlistSection,
    pub objc_imageinfo: ObjcImageInfoSection,
    pub sectcreate_sections: Vec<SectCreateSection>,
    /// The input section of each -sectcreate or -add_empty_section
    /// option.
    pub sectcreate_inputs: Vec<SectCreateInput>,
    pub init_offsets: InitOffsetsSection,
    pub chain_starts: ChainStartsSection,
    pub unwind_info: UnwindInfoSection,
    pub eh_frame: EhFrameSection,
    pub rebase_info: RebaseInfoSection,
    pub bind_info: BindInfoSection,
    pub weak_bind_info: WeakBindInfoSection,
    pub lazy_bind_info: LazyBindInfoSection,
    pub chained_fixups: ChainedFixupsSection,
    pub export_trie: ExportTrieSection,
    pub function_starts: FunctionStartsSection,
    pub data_in_code: DataInCodeSection,
    pub mergeable_record: crate::make_mergeable::MergeableRecordSection,
    pub split_info: SplitInfoSection,
    pub lazy_load_info: LazyLoadInfoSection,
    pub local_relocs: LocalRelocsSection,
    pub extern_relocs: ExternRelocsSection,
    pub indirect_symtab: IndirectSymtabSection,
    pub symtab: SymtabSection,
    pub strtab: StrtabSection,
    pub code_signature: CodeSignatureSection,
    /// Objective-C data records the linker synthesized (see
    /// merge_objc_categories) and the table of bundle_hook, each placed
    /// as the tail of the output section it names.
    pub data_blobs: Vec<crate::input_files::DataBlob>,
    /// Local symbols the linker names itself, on synthesized data:
    /// ld64's __OBJC_$_INSTANCE_METHODS_Foo(A|B) on a merged method
    /// list, and the like. (name, subsection).
    pub extra_local_syms: Vec<(&'static [u8], u32)>,
    /// The symbols naming the subsections of the functions icf folded
    /// (see icf::folded_subsec_names).
    pub folded_subsec_names: hashbrown::HashSet<SymbolId>,
    /// A DOF section for each provider of DTrace probes the image has
    /// sites of (see dtrace::create_dof_sections).
    pub dof_sections: Vec<crate::dtrace::DofSection>,
    /// -alias and selective reexports: (alias, imported target).
    /// Emitted as N_INDR symbols and re-export trie entries.
    pub indirect_aliases: Vec<(SymbolId, SymbolId)>,
    /// The symbols export lists would re-export that a library the
    /// image re-exports whole exports already (see
    /// passes::warn_redundant_reexports).
    pub redundant_reexports: Vec<SymbolId>,
    /// section$start/end and segment$start/end symbols to resolve
    /// after layout.
    pub boundary_syms: Vec<crate::passes::BoundarySym>,
    /// For -why_load: the symbol that made each object live, refreshed
    /// each resolution round.
    pub why_load: std::collections::HashMap<usize, &'static [u8]>,
    /// For -why_load: the archives -force_load or -force-l loads whole.
    pub force_loaded: std::collections::HashSet<std::path::PathBuf>,
    /// For -t: every input file as it is loaded, by the path it was
    /// found at (a library inlined in a stub by its install name).
    pub traced_files: Vec<Vec<u8>>,
    /// The address of the first thread-local data section. Thread
    /// pointers are encoded relative to it.
    pub tls_begin: u64,
    /// The address ranges where a pointer that needs a fixup is a text
    /// relocation the output can't have (passes::text_reloc_ranges).
    pub text_reloc_ranges: Vec<std::ops::Range<u64>>,
    /// The text relocations found applying relocations, as (subsection,
    /// relocation index) pairs.
    pub text_relocs: std::sync::Mutex<Vec<(u32, u32)>>,
    /// The 32-bit pointers an image dyld loads can't have (see
    /// passes::report_text_relocs), as (subsection, offset) pairs.
    pub pointers32: std::sync::Mutex<Vec<(u32, u32)>>,
    /// The output's UUID, computed from its contents.
    pub uuid: std::sync::Mutex<[u8; 16]>,
    /// The -init function, when LC_ROUTINES_64 names it (the image has
    /// no __init_offsets to run it first from).
    pub init_routine: Option<SymbolId>,
    /// Total size of the output file.
    pub output_size: u64,
    /// -print_statistics timers; inactive otherwise.
    pub timers: Timers,
    _marker: PhantomData<E>,
}

impl<E: Target> Context<E> {
    pub fn new(args: Args) -> Self {
        let timers = if args.perf { Timers::new() } else { Timers::disabled() };
        Self {
            args,
            objs: Vec::new(),
            dylibs: Vec::new(),
            symbols: SymbolTable::default(),
            isecs: Default::default(),
            internal_obj: None,
            priority_counter: 0,
            autolink_priority: u32::MAX,
            lto_plugin: None,
            lto_modules: Vec::new(),
            lto_objs: 0..0,
            lto_inputs: Vec::new(),
            visited_files: std::collections::HashSet::new(),
            reexport_files: Vec::new(),
            missing_files: Default::default(),
            processed_linker_options: std::collections::HashSet::new(),
            cmdline_linker_options: None,
            autolink_misses: Vec::new(),
            possible_files: Vec::new(),
            indirect_files: Vec::new(),
            merged_dependencies: Vec::new(),
            merged_libraries: Vec::new(),
            bundle_hook: Default::default(),
            unwind_records: Vec::new(),
            cies: Vec::new(),
            fdes: Vec::new(),
            chunks: Vec::new(),
            segments: Vec::new(),
            mach_header: OutputMachHeader::new(),
            output_sections: Vec::new(),
            stubs: StubsSection::new(),
            stub_helper: StubHelperSection::new(),
            lazy_ptrs: LazyPtrsSection::new(),
            got: GotSection::new(),
            delay_init: DelayInit::new(),
            lazy_helpers: LazyHelpersSection::new(),
            lazy_load_got: LazyLoadGotSection::new(),
            objc_stubs: ObjcStubsSection::new(),
            objc_methlist: ObjcMethlistSection::new(),
            objc_imageinfo: ObjcImageInfoSection::new(),
            sectcreate_sections: Vec::new(),
            sectcreate_inputs: Vec::new(),
            init_offsets: InitOffsetsSection::new(),
            chain_starts: ChainStartsSection::new(),
            unwind_info: UnwindInfoSection::new(),
            eh_frame: EhFrameSection::new(),
            rebase_info: RebaseInfoSection::new(),
            bind_info: BindInfoSection::new(),
            weak_bind_info: WeakBindInfoSection::new(),
            lazy_bind_info: LazyBindInfoSection::new(),
            chained_fixups: ChainedFixupsSection::new(),
            export_trie: ExportTrieSection::new(),
            function_starts: FunctionStartsSection::new(),
            data_in_code: DataInCodeSection::new(),
            mergeable_record: crate::make_mergeable::MergeableRecordSection::new(),
            split_info: SplitInfoSection::new(),
            lazy_load_info: LazyLoadInfoSection::new(),
            local_relocs: LocalRelocsSection::new(),
            extern_relocs: ExternRelocsSection::new(),
            indirect_symtab: IndirectSymtabSection::new(),
            symtab: SymtabSection::new(),
            strtab: StrtabSection::new(),
            code_signature: CodeSignatureSection::new(),
            data_blobs: Vec::new(),
            extra_local_syms: Vec::new(),
            folded_subsec_names: hashbrown::HashSet::new(),
            dof_sections: Vec::new(),
            indirect_aliases: Vec::new(),
            redundant_reexports: Vec::new(),
            boundary_syms: Vec::new(),
            why_load: std::collections::HashMap::new(),
            force_loaded: std::collections::HashSet::new(),
            traced_files: Vec::new(),
            tls_begin: 0,
            text_reloc_ranges: Vec::new(),
            text_relocs: std::sync::Mutex::new(Vec::new()),
            pointers32: std::sync::Mutex::new(Vec::new()),
            uuid: std::sync::Mutex::new([0; 16]),
            init_routine: None,
            output_size: 0,
            timers,
            _marker: PhantomData,
        }
    }

    /// Starts a -print_statistics timer for a pass.
    pub fn timer(&self, name: &str) -> crate::util::perf::Timer {
        self.timers.start(name)
    }

    /// The header of any chunk.
    pub fn chunk_header(&self, id: ChunkId) -> &ChunkHeader {
        chunk_header!(self, id)
    }

    pub fn chunk_header_mut(&mut self, id: ChunkId) -> &mut ChunkHeader {
        chunk_header!(self, id, mut)
    }

    pub fn output_section(&self, id: OutputSectionId) -> &OutputSection {
        &self.output_sections[id.index()]
    }

    pub fn output_section_mut(&mut self, id: OutputSectionId) -> &mut OutputSection {
        &mut self.output_sections[id.index()]
    }

    /// Returns the next input-order priority value.
    pub fn next_priority(&mut self) -> u32 {
        self.priority_counter += 1;
        self.priority_counter
    }

    /// Returns true if the output uses chained fixups rather than
    /// classic dyld rebase/bind opcodes: it is laid out for them, and
    /// no unaligned pointer turned an x86-64 image's to classic dyld
    /// info.
    pub fn use_chained_fixups(&self) -> bool {
        self.args.fixup_chains && !self.chained_fixups.disabled
    }

    /// Whether the file is the internal object holding synthesized
    /// sections and symbols.
    pub fn is_internal(&self, idx: usize) -> bool {
        self.internal_obj == Some(idx)
    }

    /// Whether the file is the hook for the classes of mergeable
    /// libraries, which ld-prime counts as linker synthesized.
    pub fn is_bundle_hook(&self, idx: usize) -> bool {
        self.bundle_hook.obj == Some(idx)
    }

    /// Binds a symbol the linker's own code calls (dyld_stub_binder,
    /// __dyld_lazy_load), unless something in the link defines it, to
    /// the first loaded dylib that exports it - or, if none does and
    /// the image may look the symbol up dynamically (-undefined
    /// dynamic_lookup, -U), to whatever image dyld finds it in. None if
    /// neither.
    pub fn bind_linker_import(&mut self, name: &'static [u8]) -> Option<SymbolId> {
        let args = &self.args;
        let looked_up =
            args.undefined_dynamic_lookup || args.allowed_undefined.iter().any(|n| n == name);
        let dylib = match self.dylibs.iter().position(|d| d.exports.contains(name)) {
            Some(i) => i as u32,
            None if looked_up => u32::MAX,
            None => return None,
        };
        let id = self.symbols.intern(name);
        let sym = &mut self.symbols[id];
        if !sym.is_defined() {
            sym.set_file(FileId::Dylib(dylib));
            sym.set_imported(true);
            sym.set_extern(true);
            sym.set_input_section(None);
        }
        Some(id)
    }

    /// Returns the address of the __got slot the objc stubs load
    /// _objc_msgSend from.
    pub fn objc_msgsend_got_addr(&self) -> u64 {
        self.symbols[self.objc_stubs.msgsend_sym.unwrap()].got_addr(self)
    }
}
