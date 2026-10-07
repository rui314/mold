//! The linker context: everything about one link, from parsed arguments
//! to output chunks.

use std::marker::PhantomData;

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
use crate::error;
use crate::input_files::{DylibFile, FileId, ObjectFile};
use crate::input_sections::{InputSection, Reloc, RelocTarget};
use crate::macho::{S_THREAD_LOCAL_REGULAR, S_THREAD_LOCAL_ZEROFILL};
use crate::symbol::{SymbolId, SymbolTable};
use crate::target::Target;
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

/// A section$start/end or segment$start/end symbol: (symbol, is_start,
/// segment, section), the names those the symbol gives until the
/// layout renames them.
pub type BoundarySym = (SymbolId, bool, &'static [u8], Option<&'static [u8]>);

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
    /// Per-symbol synthetic-slot indices (SymbolId-indexed), grown
    /// lazily; mold's SymbolAux side table.
    pub sym_aux: Vec<crate::symbol::SymAux>,
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
    pub unwind_records: Vec<crate::input_files::UnwindRecord>,
    /// DWARF CIEs and FDEs from all objects' __eh_frame sections.
    pub cies: Vec<crate::input_files::Cie>,
    pub fdes: Vec<crate::input_files::Fde>,
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
    pub data_blobs: Vec<crate::objc::DataBlob>,
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
    pub boundary_syms: Vec<BoundarySym>,
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
    /// The resolved address of the entry point symbol.
    pub entry_addr: u64,
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
            sym_aux: Vec::new(),
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
            entry_addr: 0,
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

    /// The section ordinal (nlist n_sect) of the chunk a subsection is
    /// laid out in; 0 when it has none.
    pub fn isec_n_sect(&self, isec: &InputSection) -> u8 {
        isec.output_section().map_or(0, |id| self.chunk_header(id).n_sect)
    }

    /// Address of the selector reference slot `i` in the tail of the
    /// __objc_selrefs output section: objc stub `i`'s, or past the
    /// stubs, extra selector reference `i - stubs`.
    pub fn objc_selref_addr(&self, i: usize) -> u64 {
        let osec = self.output_section(self.objc_stubs.selrefs.unwrap());
        osec.hdr.addr + osec.tail_off + i as u64 * 8
    }

    /// Address of the selector name string for objc stub `i`, in the
    /// tail of the __objc_methname output section.
    pub fn objc_methname_addr(&self, i: usize) -> u64 {
        let osec = self.output_section(self.objc_stubs.methname.unwrap());
        osec.hdr.addr + osec.tail_off + self.objc_stubs.methname_offs[i]
    }

    /// The library ordinal as the chained-fixups import formats encode
    /// it in a `bits`-wide field: dylib ordinals as they are, the
    /// special ones as negative values in the field's two's complement,
    /// and, unlike the bind opcodes, the main executable as -1 (0 is
    /// the image itself there).
    pub fn chained_import_ordinal(&self, dylib: u32, bits: u32) -> u64 {
        let ordinal = match self.bind_ordinal(dylib) {
            crate::macho::BIND_SPECIAL_DYLIB_MAIN_EXECUTABLE => -1i64,
            n => n as i64,
        };
        (ordinal as u64) & ((1u64 << bits) - 1)
    }

    /// The library ordinal in an undefined symbol's n_desc, which only
    /// a two-level namespace image has: EXECUTABLE_ORDINAL (0xff) for
    /// the -bundle_loader executable, DYNAMIC_LOOKUP_ORDINAL (0xfe) for
    /// a symbol left to dynamic lookup, else the dylib's. ld-prime
    /// writes 0 in a -flat_namespace image, where every import is a
    /// flat lookup.
    pub fn nlist_library_ordinal(&self, dylib: u32) -> u8 {
        if self.args.flat_namespace {
            return 0;
        }
        match self.bind_ordinal(dylib) {
            crate::macho::BIND_SPECIAL_DYLIB_MAIN_EXECUTABLE => 0xff,
            n => n as u8,
        }
    }

    /// Returns the bind ordinal for a symbol imported from `dylib`:
    /// the dylib's load-command ordinal under two-level namespace (the
    /// image's own for one of its private re-exports; see
    /// DylibFile::binds_to_image), or the flat-lookup sentinel with
    /// -flat_namespace / dynamic lookup.
    pub fn bind_ordinal(&self, dylib: u32) -> i32 {
        if self.args.flat_namespace || dylib == u32::MAX {
            crate::macho::BIND_SPECIAL_DYLIB_FLAT_LOOKUP
        } else if self.dylibs[dylib as usize].binds_to_image {
            crate::macho::BIND_SPECIAL_DYLIB_SELF
        } else {
            self.dylibs[dylib as usize].dylib_idx
        }
    }

    /// Returns the next input-order priority value.
    pub fn next_priority(&mut self) -> u32 {
        self.priority_counter += 1;
        self.priority_counter
    }

    /// The address the segments are laid out from: -image_base (or
    /// -segaddr __TEXT) as resolve_image_base settles it, else the end
    /// of __PAGEZERO. __TEXT, and the mach header with it, goes here
    /// unless -segaddr pins __TEXT in a PIE executable, which then
    /// fails to link.
    pub fn image_base(&self) -> u64 {
        self.args.image_base.unwrap_or(self.args.pagezero_size)
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

    /// Whether an object is one LTO compiled.
    pub fn is_lto_obj(&self, idx: usize) -> bool {
        self.lto_objs.contains(&idx)
    }

    /// Whether the link strips dead code: under -dead_strip, or - as
    /// ld-prime does unasked - in a final image of code LTO compiled,
    /// executable or not, which it walks as dead stripping does to
    /// find what LTO must preserve (see
    /// dead_strip::native_refs_before_lto). That strip leaves the
    /// imports only stripped code used in the symbol table, unbound,
    /// and the map lists nothing it removed.
    pub fn strips_dead_code(&self) -> bool {
        self.args.dead_strip || (!self.args.relocatable && !self.lto_inputs.is_empty())
    }

    /// Adds a section the linker synthesizes to the internal object,
    /// returning the (file, shndx) pair a subsection standing for it
    /// carries.
    pub fn add_synthetic_section(&mut self, hdr: crate::macho::MachSection) -> (u32, u32) {
        let file = self.internal_obj.expect("internal object not created yet");
        let hdrs = self.objs[file].sect_hdrs.to_mut();
        hdrs.push(hdr);
        (file as u32, (hdrs.len() - 1) as u32)
    }

    /// The parent section header of a subsection, through its object's
    /// section list - mold resolves a section's shdr through its
    /// file the same way.
    #[inline]
    pub fn hdr_of(&self, isec: &InputSection) -> &crate::macho::MachSection {
        &self.objs[isec.file as usize].sect_hdrs[isec.shndx as usize]
    }

    /// Follows literal-merge redirects to the surviving subsection.
    pub fn resolve_isec(&self, mut id: usize) -> usize {
        while self.isecs[id].replacement != crate::input_sections::NO_REPLACEMENT {
            id = self.isecs[id].replacement as usize;
        }
        id
    }

    /// A symbol's synthetic-slot indices, from the side table. Returns
    /// the all-absent default for symbols with no slots (the table is
    /// grown lazily by the first setter).
    pub fn sym_aux(&self, id: SymbolId) -> &crate::symbol::SymAux {
        // Sparse, as mold's SymbolAux: the symbol carries an index
        // into the table, NONE for the vast majority that have no slot.
        match self.symbols[id].aux_idx {
            crate::symbol::NONE => &crate::symbol::NONE_AUX,
            i => &self.sym_aux[i as usize],
        }
    }

    /// Mutable access to a symbol's slot indices, growing the side table
    /// to cover it. Called only from the serial slot-assignment passes.
    pub fn sym_aux_mut(&mut self, id: SymbolId) -> &mut crate::symbol::SymAux {
        Self::sym_aux_mut_in(&mut self.symbols, &mut self.sym_aux, id)
    }

    /// `sym_aux_mut` over the two tables it touches, for callers that
    /// hold another part of the context borrowed at the same time.
    pub fn sym_aux_mut_in<'a>(
        symtab: &mut crate::symbol::SymbolTable,
        sym_aux: &'a mut Vec<crate::symbol::SymAux>,
        id: SymbolId,
    ) -> &'a mut crate::symbol::SymAux {
        // Allocate the symbol's entry on first use; the table holds only
        // the symbols that take a slot (mold's sparse SymbolAux).
        if symtab[id].aux_idx == crate::symbol::NONE {
            symtab[id].aux_idx = sym_aux.len() as u32;
            sym_aux.push(Default::default());
        }
        let i = symtab[id].aux_idx as usize;
        &mut sym_aux[i]
    }

    /// A subsection's relocations, sliced from its object's reloc arena
    /// (subsections keep only a rel_offset/nrels range, sold-style).
    pub fn isec_relocs(&self, id: usize) -> &[crate::input_sections::Reloc] {
        let isec = &self.isecs[id];
        let off = isec.rel_offset as usize;
        &self.objs[isec.file as usize].relocs[off..off + isec.nrels as usize]
    }

    /// A subsection's output address: its output section's address plus
    /// its offset there, as mold's isec.addr(ctx) derives it. A
    /// literal-merge loser reports its surviving copy's address; an
    /// unplaced subsection reports 0.
    #[inline]
    pub fn isec_addr(&self, id: usize) -> u64 {
        let mut isec = &self.isecs[id];
        if isec.replacement != crate::input_sections::NO_REPLACEMENT {
            isec = &self.isecs[self.resolve_isec(id)];
        }
        let Some(chunk) = isec.output_section() else {
            return 0;
        };
        if isec.offset == u32::MAX {
            return 0;
        }
        self.chunk_header(chunk).addr + isec.offset as u64
    }

    /// Returns the output address of a symbol.
    pub fn sym_addr(&self, id: SymbolId) -> u64 {
        let sym = &self.symbols[id];
        match sym.file() {
            // A DTrace symbol, never defined, is at address 0 for
            // ld-prime: where a branch that is no probe site goes.
            None => {
                if !crate::dtrace::is_dtrace_symbol(sym.name()) {
                    error!("undefined symbol: {sym}");
                }
                0
            }
            Some(FileId::Obj(_)) => {
                if let Some(isec) = sym.input_section().map(|i| i as usize) {
                    self.isec_addr(isec) + sym.value
                } else if self.sym_aux(id).objc_stub_idx != crate::symbol::NO_IDX {
                    self.objc_stubs.hdr.addr
                        + self.sym_aux(id).objc_stub_idx as u64 * self.objc_stub_size()
                } else {
                    sym.value
                }
            }
            // A branch to a dylib symbol goes through its stub, or for
            // a lazily loaded dylib's, its call helper. Other
            // references to dylib symbols are filled in by dyld; the
            // relocation scan has already validated them.
            Some(FileId::Dylib(_)) => {
                let aux = self.sym_aux(id);
                if aux.stub_idx != crate::symbol::NO_IDX {
                    self.sym_stub_addr(id)
                } else if aux.lazy_stub_idx != crate::symbol::NO_IDX {
                    self.lazy_helper_addr(aux.lazy_stub_idx as usize)
                } else if aux.delay_stub_idx != crate::symbol::NO_IDX {
                    self.delay_stub_addr(aux.delay_stub_idx as usize)
                } else {
                    0
                }
            }
        }
    }

    /// The size of one __objc_stubs entry.
    pub fn objc_stub_size(&self) -> u64 {
        if self.args.objc_stubs_small { E::OBJC_SMALL_STUB_SIZE } else { E::OBJC_STUB_SIZE }
    }

    /// Returns the address of a symbol's __stubs entry.
    pub fn sym_stub_addr(&self, id: SymbolId) -> u64 {
        self.stubs.hdr.addr + self.sym_aux(id).stub_idx as u64 * E::STUB_SIZE
    }

    /// Returns the address of __lazy_helpers entry `i`.
    pub fn lazy_helper_addr(&self, i: usize) -> u64 {
        self.lazy_helpers.hdr.addr + self.lazy_helpers.helpers[i].offset as u64
    }

    /// Returns the address of __delay_stubs entry `i`.
    pub fn delay_stub_addr(&self, i: usize) -> u64 {
        self.delay_init.stubs_hdr.addr + i as u64 * E::DELAY_STUB_SIZE
    }

    /// Returns the address of __delay_helper's load helper `i`.
    pub fn delay_helper_addr(&self, i: usize) -> u64 {
        self.delay_init.helper_hdr.addr + self.delay_init.helpers[i].offset as u64
    }

    /// Returns the address of __delay_helper's dlopen helper `i`.
    pub fn dlopen_helper_addr(&self, i: usize) -> u64 {
        self.delay_init.helper_hdr.addr + self.delay_init.dlopens[i].offset as u64
    }

    /// True for a symbol of a dylib whose initializers wait for the
    /// image's first use of it (see delay_init::create_delay_init).
    pub fn is_delay_import(&self, id: SymbolId) -> bool {
        match self.symbols[id].file() {
            Some(FileId::Dylib(d)) => d != u32::MAX && self.dylibs[d as usize].delay_init.is_some(),
            _ => false,
        }
    }

    /// True for a symbol of a dylib dyld loads lazily (see
    /// lazy_load::create_lazy_loads).
    pub fn is_lazy_import(&self, id: SymbolId) -> bool {
        match self.symbols[id].file() {
            Some(FileId::Dylib(d)) => d != u32::MAX && self.dylibs[d as usize].is_lazy,
            _ => false,
        }
    }

    /// The size of __stub_helper's header, the code its entries jump to
    /// that enters dyld_stub_binder. Legacy LINKEDIT's entries go to
    /// crt1.o's dyld_stub_binding_helper instead, and its helper has no
    /// header.
    pub fn stub_helper_header_size(&self) -> u64 {
        if self.args.legacy_linkedit { 0 } else { E::STUB_HELPER_HEADER_SIZE }
    }

    /// The address of the pointer slot stub `i` (for symbol `id`)
    /// jumps through: its lazy pointer, or its GOT slot. A weak
    /// definition of this image always goes through its GOT slot (the
    /// lazy binder cannot do weak lookup), as in ld64.
    pub fn stub_ptr_addr(&self, i: usize, id: SymbolId) -> u64 {
        if self.args.lazy_binding && !self.binds_weak_lookup(id) {
            let slot = self.stubs.lazy.binary_search(&(i as u32)).unwrap();
            self.lazy_ptrs.hdr.addr + slot as u64 * 8
        } else {
            self.sym_got_addr(id)
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
    pub fn is_weak_coalesced(&self, id: SymbolId) -> bool {
        if self.args.relocatable || self.args.is_dylinker() {
            return false;
        }
        let sym = &self.symbols[id];
        matches!(sym.file(), Some(FileId::Obj(_)))
            && sym.is_weak_def()
            && sym.is_extern()
            && !sym.is_private_extern()
    }

    /// True for a live weak definition the image exports, which another
    /// image's copy may replace at load time (and so for which ld-prime
    /// sets MH_WEAK_DEFINES): not an auto-hidden or private extern one,
    /// nor one -dead_strip removed.
    pub fn exports_weak_def(&self, id: SymbolId) -> bool {
        let sym = &self.symbols[id];
        sym.is_weak_def()
            && sym.is_extern()
            && !sym.is_private_extern()
            && sym.input_section().is_some_and(|isec| self.isecs[isec as usize].is_alive())
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
    pub fn is_interposable_export(&self, id: SymbolId) -> bool {
        let args = &self.args;
        let sym = &self.symbols[id];
        let flat = args.flat_namespace
            && matches!(args.output_type, crate::macho::MH_DYLIB | crate::macho::MH_BUNDLE);
        let listed = !args.relocatable
            && !args.without_dyld()
            && !args.is_dylinker()
            && args.interposable.as_ref().is_some_and(|g| g.find(sym.name()) != -1);
        (flat || listed)
            && matches!(sym.file(), Some(FileId::Obj(_)))
            && sym.input_section().is_some()
            && sym.is_extern()
            && !sym.is_private_extern()
    }

    /// True if dyld binds the slots referring to this symbol as it
    /// binds an import's, by name from the bind stream: an import, or an
    /// interposable export.
    pub fn binds_as_import(&self, id: SymbolId) -> bool {
        self.symbols[id].is_imported() || self.is_interposable_export(id)
    }

    /// True if dyld binds a pointer in data to this symbol rather than
    /// sliding it: one to an import's (see binds_as_import), or, in
    /// legacy LINKEDIT (Args::legacy_linkedit), to one of the image's
    /// coalescable weak definitions, which an external relocation
    /// binds by name. LC_DYLD_INFO slides that one and weak-binds it.
    pub fn binds_pointer(&self, id: SymbolId) -> bool {
        self.binds_as_import(id) || (self.args.legacy_linkedit && self.is_weak_coalesced(id))
    }

    /// The library ordinal a bind of this symbol names: its dylib's, or
    /// for an interposable export, the flat lookup under
    /// -flat_namespace, else the image itself.
    pub fn sym_bind_ordinal(&self, id: SymbolId) -> i32 {
        match self.symbols[id].file() {
            Some(FileId::Dylib(dylib)) => self.bind_ordinal(dylib),
            _ if self.is_dtrace_pointer_target(id) => crate::macho::BIND_SPECIAL_DYLIB_FLAT_LOOKUP,
            _ => self.export_bind_ordinal(),
        }
    }

    /// The library ordinal an interposable export binds with.
    pub fn export_bind_ordinal(&self) -> i32 {
        match self.args.flat_namespace {
            true => crate::macho::BIND_SPECIAL_DYLIB_FLAT_LOOKUP,
            false => crate::macho::BIND_SPECIAL_DYLIB_SELF,
        }
    }

    /// True for a definition of this image that dyld may replace with
    /// another image's at load time, so that its references go through
    /// slots dyld binds and its calls through its stub: a weak
    /// definition subject to coalescing, or an interposable export.
    pub fn is_interposable(&self, id: SymbolId) -> bool {
        self.is_weak_coalesced(id) || self.is_interposable_export(id)
    }

    /// True for a DTrace symbol (see dtrace), never defined, which a
    /// pointer in data binds by flat lookup, as ld-prime has it: no
    /// import, it takes no stub, GOT slot or symbol table entry.
    pub fn is_dtrace_pointer_target(&self, id: SymbolId) -> bool {
        let sym = &self.symbols[id];
        sym.file().is_none() && crate::dtrace::is_dtrace_symbol(sym.name())
    }

    /// True if dyld fills the references to this symbol: an import, or
    /// a definition it may interpose.
    pub fn binds_at_runtime(&self, id: SymbolId) -> bool {
        self.symbols[id].is_imported() || self.is_interposable(id)
    }

    /// An input's N_ABS definition has no section and never slides.
    /// Sectionless symbols in the internal object instead describe the
    /// image (its header and layout boundaries), so their values slide.
    pub fn is_absolute_symbol(&self, id: SymbolId) -> bool {
        let sym = &self.symbols[id];
        sym.input_section().is_none()
            && matches!(sym.file(), Some(FileId::Obj(obj)) if !self.is_internal(obj as usize))
    }

    /// A GOT load relaxes to a PC-relative address computation unless
    /// dyld fills the slot - an import or an interposable export, or a
    /// weak definition it binds by weak lookup, which a -static image's
    /// code never is - or the target is an absolute constant: the
    /// instruction slides but the value does not.
    pub fn can_relax_got(&self, id: SymbolId) -> bool {
        !self.binds_as_import(id) && !self.binds_weak_lookup(id) && !self.is_absolute_symbol(id)
    }

    /// True for a definition this image exports that some dylib in the
    /// link exports as a weak definition: the program's own operator
    /// new overriding libc++'s. dyld must let it win coalescing, so
    /// the image is marked WEAK_DEFINES and, with classic dyld info,
    /// the symbol is listed in the weak_bind stream as a non-weak
    /// definition (ld64 does both).
    pub fn overrides_weak_export(&self, id: SymbolId) -> bool {
        let sym = &self.symbols[id];
        matches!(sym.file(), Some(FileId::Obj(_)))
            && sym.is_extern()
            && !sym.is_private_extern()
            && !sym.is_weak_def()
            && self.dylibs.iter().any(|d| d.weak_exports.contains(sym.name()))
    }

    /// True if dyld resolves this symbol by weak lookup - searching
    /// every loaded image for the coalesced definition - rather than
    /// in one dylib: a coalescable weak definition of this image, or
    /// an import that its dylib exports as a weak definition (libc++'s
    /// operator new and delete, which a program may override). ld64
    /// binds both with library ordinal -3, never lazily, and lists
    /// them in the classic weak_bind stream.
    pub fn binds_weak_lookup(&self, id: SymbolId) -> bool {
        // A static image has no dyld to perform runtime weak lookup, so a
        // call to a weakly-defined symbol in the image binds directly.
        // ld64 emits neither a stub nor a weak bind for it (the stock
        // XNU kernel has no stubs and an empty weak bind table). Nor
        // does a kext's but in the shared region, where it calls and
        // takes its weak definitions through the GOT as ld-prime links
        // an arm64 kext.
        if self.args.static_link || (self.args.is_kext() && !self.args.shared_region) {
            return false;
        }
        if self.is_weak_coalesced(id) {
            return true;
        }
        let sym = &self.symbols[id];
        match sym.file() {
            Some(FileId::Dylib(d)) if d != u32::MAX => {
                self.dylibs[d as usize].weak_exports.contains(sym.name())
            }
            _ => false,
        }
    }

    /// True for an exported Objective-C class (or metaclass) of a dylib
    /// bound for the shared region, whose pointers ld-prime writes as
    /// binds to the image itself rather than rebases, with chained
    /// fixups: the cache builder may redirect them to a class that
    /// replaces this one.
    pub fn binds_to_self(&self, id: SymbolId) -> bool {
        let sym = &self.symbols[id];
        self.args.shared_region
            && self.args.output_type == crate::macho::MH_DYLIB
            && self.use_chained_fixups()
            && matches!(sym.file(), Some(FileId::Obj(_)))
            && sym.is_extern()
            && !sym.is_private_extern()
            && (sym.name().starts_with(b"_OBJC_CLASS_$_")
                || sym.name().starts_with(b"_OBJC_METACLASS_$_"))
    }

    /// The address a branch to `id` targets: the symbol's stub when it
    /// has one and dyld may redirect it, else the symbol itself.
    pub fn branch_target_addr(&self, id: SymbolId) -> u64 {
        if self.is_interposable(id) && self.sym_aux(id).stub_idx != crate::symbol::NO_IDX {
            self.sym_stub_addr(id)
        } else {
            self.sym_addr(id)
        }
    }

    /// Returns the address of a symbol's __got slot, or for a lazily
    /// loaded dylib's symbol, its __lazy_load_got slot.
    pub fn sym_got_addr(&self, id: SymbolId) -> u64 {
        let aux = self.sym_aux(id);
        if aux.got_idx == crate::symbol::NO_IDX && aux.lazy_got_idx != crate::symbol::NO_IDX {
            return self.lazy_load_got.slot_addr(aux.lazy_got_idx);
        }
        self.got.slot_addr(aux.got_idx as usize)
    }

    /// Returns the address of the __got slot the objc stubs load
    /// _objc_msgSend from.
    pub fn objc_msgsend_got_addr(&self) -> u64 {
        self.sym_got_addr(self.objc_stubs.msgsend_sym.unwrap())
    }

    /// Returns the symbol a relocation refers to, if it refers to one.
    pub fn reloc_target_sym(&self, obj: usize, rel: &Reloc) -> Option<SymbolId> {
        match rel.target() {
            RelocTarget::Sym(idx) => Some(self.objs[obj].symbols[idx as usize]),
            RelocTarget::Section(_) => None,
        }
    }

    /// Returns the input section a relocation's target lives in, if any.
    pub fn reloc_target_isec(&self, obj: usize, rel: &Reloc) -> Option<usize> {
        match rel.target() {
            RelocTarget::Sym(idx) => self.symbols[self.objs[obj].symbols[idx as usize]]
                .input_section()
                .map(|i| i as usize),
            RelocTarget::Section(idx) => Some(idx as usize),
        }
    }

    /// Returns true if a relocation's target is thread-local data.
    pub fn reloc_target_is_tls(&self, obj: usize, rel: &Reloc) -> bool {
        self.reloc_target_isec(obj, rel).is_some_and(|isec| {
            matches!(
                self.hdr_of(&self.isecs[isec]).section_type(),
                S_THREAD_LOCAL_REGULAR | S_THREAD_LOCAL_ZEROFILL
            )
        })
    }

    /// Resolves a relocation target to its output address.
    pub fn reloc_target_addr(&self, obj: usize, rel: &Reloc) -> u64 {
        match rel.target() {
            RelocTarget::Sym(idx) => self.sym_addr(self.objs[obj].symbols[idx as usize]),
            RelocTarget::Section(idx) => self.isec_addr(idx as usize),
        }
    }

    /// The symbol that names subsection `id`: of those at its start, the
    /// one input_files::subsec_name_rank ranks first. A literal merged by
    /// its content (see input_files::has_merged_subsecs) is named by none
    /// of the labels a compiler or assembler makes for itself (see
    /// input_files::is_private_label), and by nothing at all if one but
    /// an ltmpN is among them, as ld-prime has it (mergeable records name
    /// their entries so).
    pub fn subsec_label(&self, id: usize) -> Option<&'static [u8]> {
        let isec = &self.isecs[id];
        let obj = &self.objs[isec.file as usize];
        self.subsec_label_index(id).map(|i| self.symbols[obj.symbols[i]].name())
    }

    /// The index in its object's symbol table of the symbol that names
    /// subsection `id` (see subsec_label).
    pub fn subsec_label_index(&self, id: usize) -> Option<usize> {
        use crate::input_files::{has_merged_subsecs, is_private_label, subsec_name_rank};
        let isec = &self.isecs[id];
        let obj = &self.objs[isec.file as usize];
        let key = Some(label_key(isec));
        let labels = (0..obj.nlists.len())
            .filter(|&i| nlist_label_key(&obj.nlists[i]) == key)
            .map(|i| (i, &obj.nlists[i], self.symbols[obj.symbols[i]].name()));
        let merged = has_merged_subsecs(self.hdr_of(isec));
        if merged
            && labels
                .clone()
                .any(|(_, _, name)| is_private_label(name) && !name.starts_with(b"ltmp"))
        {
            return None;
        }
        labels
            .filter(|(_, _, name)| !(merged && is_private_label(name)))
            .max_by_key(|&(i, n, name)| (subsec_name_rank(n, name), name, i))
            .map(|(i, _, _)| i)
    }

    /// The name of subsection `id` in a diagnostic: its label (see
    /// subsec_label), or else its section and its offset there,
    /// "__TEXT,__cstring+0x10".
    pub fn subsec_name(&self, id: usize) -> std::borrow::Cow<'static, [u8]> {
        if let Some(name) = self.subsec_label(id) {
            return name.into();
        }
        let isec = &self.isecs[id];
        let hdr = self.hdr_of(isec);
        let (seg, sect) = (crate::error::raw(hdr.segname()), crate::error::raw(hdr.sectname()));
        let off = isec.input_addr as u64 - hdr.addr;
        crate::error::render(format_args!("{seg},{sect}+0x{off:x}")).into()
    }

    /// How a diagnostic names the target of relocation `rel` of object
    /// `obj`: its symbol, or the subsection it points to.
    pub fn reloc_target_name(&self, obj: usize, rel: &Reloc) -> std::borrow::Cow<'static, [u8]> {
        match rel.target() {
            RelocTarget::Sym(idx) => {
                self.symbols[self.objs[obj].symbols[idx as usize]].name().into()
            }
            RelocTarget::Section(idx) => self.subsec_name(idx as usize),
        }
    }

    /// Reports a relocation that can't be applied where it is, `offset`
    /// bytes into subsection `isec`.
    pub fn fixup_error(&self, isec: usize, offset: u32, msg: std::fmt::Arguments) {
        let file =
            crate::error::RawPath::raw(self.objs[self.isecs[isec].file as usize].mf.name.as_path());
        let name = self.subsec_name(isec);
        let name = crate::error::raw(&name);
        crate::error!("{file}: {name}+0x{offset:x}: {msg}");
    }

    /// Whether the target of relocation `r` of subsection `isec`, of
    /// object `obj`, has an address in the image, as a PC-relative
    /// reference that goes through no stub or GOT slot needs (an
    /// x86-64 RIP-relative one, an arm64 adrp or the offset into its
    /// page): an import has none, which is an error.
    pub fn target_has_address(&self, obj: usize, isec: usize, r: &Reloc) -> bool {
        let Some(id) = self.reloc_target_sym(obj, r).filter(|&id| self.symbols[id].is_imported())
        else {
            return true;
        };
        let msg = format_args!("target '{}' does not have address", self.symbols[id]);
        self.fixup_error(isec, r.offset, msg);
        false
    }

    /// Notes relocation `i` of `rels`, subsection `isec`'s, whose
    /// pointer is at `addr`, if it is a text relocation: in a range of
    /// text_reloc_ranges, and needing a fixup.
    #[inline]
    pub fn check_text_reloc(&self, isec: usize, rels: &[Reloc], i: usize, addr: u64) {
        if self.text_reloc_ranges.iter().any(|range| range.contains(&addr)) {
            self.note_text_reloc(isec, rels, i);
        }
    }

    /// Records a pointer in a read-only segment if dyld (or whatever
    /// loads the image) has to bind or slide it, as the fixup builders
    /// decide.
    #[cold]
    fn note_text_reloc(&self, isec: usize, rels: &[Reloc], i: usize) {
        let file = self.isecs[isec].file as usize;
        let rel = &rels[i];
        let slides = self.args.pie || self.args.output_type != crate::macho::MH_EXECUTE;
        let needs_fixup = match self.reloc_target_sym(file, rel) {
            Some(id) if self.binds_at_runtime(id) || self.binds_to_self(id) => true,
            Some(id) if self.is_absolute_symbol(id) => false,
            _ => slides && !self.reloc_target_is_tls(file, rel),
        };
        if needs_fixup {
            self.text_relocs.lock().unwrap().push((isec as u32, i as u32));
        }
    }

    /// Names the place `offset` bytes into subsection `isec`:
    /// "'NAME'+0xOFF (path)".
    pub fn subsec_ref(&self, isec: usize, offset: u32) -> crate::error::Message {
        let path =
            crate::error::RawPath::raw(self.objs[self.isecs[isec].file as usize].mf.name.as_path());
        let name = self.subsec_name(isec);
        let name = crate::error::raw(&name);
        if offset == 0 {
            crate::error::render(format_args!("'{name}' ({path})"))
        } else {
            crate::error::render(format_args!("'{name}'+0x{offset:X} ({path})"))
        }
    }
}

/// Where the labels at the start of a subsection sit: its section,
/// counted from 1 as nlists count them, and its address.
fn label_key(isec: &InputSection) -> (u32, u64) {
    (isec.shndx + 1, isec.input_addr as u64)
}

/// Where a symbol labels a subsection's start, if it is a label at all.
fn nlist_label_key(nlist: &crate::macho::NList) -> Option<(u32, u64)> {
    (!nlist.is_stab() && nlist.n_type() == crate::macho::N_SECT)
        .then_some((nlist.n_sect as u32, nlist.n_value))
}
