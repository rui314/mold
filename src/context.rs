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
use crate::chunks::sectcreate::SectCreateSection;
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
            ChunkId::WeakGot => &$($mutable)? $ctx.got.weak_hdr,
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
    /// Per-symbol synthetic-slot indices (SymbolId-indexed), grown
    /// lazily; mold's SymbolAux side table.
    pub sym_aux: Vec<crate::symbol::SymAux>,
    /// Input-order counter for resolution tie-breaking.
    pub priority_counter: u32,
    /// The dylibs built for another platform than the link's, by the
    /// priority of the input they came with and with ld-prime's message,
    /// which it gives as it checks the inputs' versions (see
    /// passes::check_input_versions).
    pub foreign_platform_dylibs: Vec<(u32, String)>,
    /// The inputs that name a dylib loaded before, by another input -
    /// the same file by path, or another with its install name - by
    /// priority, with the dylib's index: ld-prime checks the dylib's
    /// version again for each (see passes::check_input_versions).
    pub dylib_renamings: Vec<(u32, usize)>,
    /// Files already loaded, so a library named twice (command line
    /// plus auto-link) is read once.
    pub visited_files: std::collections::HashSet<std::path::PathBuf>,
    /// Under -dependency_info, the files of the libraries loaded as
    /// another's re-exports, as found (see load_reexports).
    pub reexport_files: Vec<std::path::PathBuf>,
    /// The loaded libLTO, once a bitcode input has been seen.
    pub lto_plugin: Option<crate::lto::Plugin>,
    /// Bitcode modules registered for LTO.
    pub lto_modules: Vec<crate::lto::BitcodeModule>,
    /// The objects LTO compiled the live bitcode modules to: one per
    /// ThinLTO module, in input order, then the merged modules' one.
    pub lto_objs: std::ops::Range<usize>,
    /// The bitcode files LTO compiled, in input order.
    pub lto_inputs: Vec<crate::lto::LtoInput>,
    /// The object LTO merged modules to, if it did (the last of
    /// lto_objs).
    pub merged_lto_obj: Option<usize>,
    /// The symbols two bitcode files define, found before LTO and
    /// reported after it (see passes::find_bitcode_duplicates).
    pub bitcode_duplicates: Vec<crate::passes::Duplicate>,
    /// The imports only code that ld-prime's own dead stripping of an
    /// LTO link removed used, which stay in the symbol table (see
    /// Context::strips_dead_code).
    pub stripped_imports: Vec<crate::symbol::SymbolId>,
    /// Auto-link options already acted on.
    pub processed_linker_options: std::collections::HashSet<Vec<Vec<u8>>>,
    /// -add_linker_option's auto-link options, once read as an
    /// object's are (see passes::read_linker_options).
    pub cmdline_linker_options: Option<Vec<Vec<Vec<u8>>>>,
    /// The warnings about the auto-link options of the objects read
    /// (see passes::warn_linker_options), by object.
    pub linker_option_warnings: Vec<(usize, String)>,
    /// The libraries and frameworks auto-link options named that were
    /// not found, as ld-prime reports them if symbols stay undefined.
    pub autolink_misses: Vec<String>,
    /// The files only -possible-l and the like name, which load with
    /// the auto-linked libraries (see passes::load_autolink_deps).
    pub possible_files: Vec<std::path::PathBuf>,
    /// Under -commons error, the first tentative definition a dylib
    /// defines too, as ld-prime reports it once it has found no
    /// duplicate symbol (see passes::check_common_conflicts).
    pub common_conflict: Option<String>,
    /// The dylibs named on the command line that -dead_strip_dylibs
    /// dropped, which ld-prime's -map still lists: their positions
    /// among the inputs and paths.
    pub stripped_dylibs: Vec<(u32, std::path::PathBuf)>,
    /// The files -dylib_file names for re-exported libraries that are
    /// no libraries, to load as inputs (see collect_indirect_files).
    pub indirect_files: Vec<&'static crate::mapped_file::MappedFile>,
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
    pub split_info: SplitInfoSection,
    pub lazy_load_info: LazyLoadInfoSection,
    pub local_relocs: LocalRelocsSection,
    pub extern_relocs: ExternRelocsSection,
    pub indirect_symtab: IndirectSymtabSection,
    pub symtab: SymtabSection,
    pub strtab: StrtabSection,
    pub code_signature: CodeSignatureSection,
    /// Sequence number of the next dylib named on the command line or
    /// by an auto-link option, or archive an auto-link option names;
    /// orders their load commands, and -map's auto-linked files.
    pub dylib_load_seq: u32,
    /// The archives auto-link options named, with their sequence
    /// numbers (see dylib_load_seq).
    pub autolinked_archives: hashbrown::HashMap<std::path::PathBuf, u32>,
    /// Objective-C data records the linker synthesized (see
    /// merge_objc_categories), each placed as the tail of the output
    /// section it names.
    pub data_blobs: Vec<crate::objc::DataBlob>,
    /// Local symbols the linker names itself, on synthesized data:
    /// ld64's __OBJC_$_INSTANCE_METHODS_Foo(A|B) on a merged method
    /// list, and the like. (name, subsection).
    pub extra_local_syms: Vec<(&'static str, u32)>,
    /// The first object (in input order) that claimed a common symbol:
    /// the synthesized __common section takes its place in the section
    /// order from it, as ld64's does.
    pub common_first_obj: Option<u32>,
    /// The symbols naming the atoms of the functions icf folded, each
    /// with whether the output drops it (see icf::folded_atom_names).
    pub folded_atom_names: hashbrown::HashMap<SymbolId, bool>,
    /// -alias and selective reexports: (alias, imported target).
    /// Emitted as N_INDR symbols and re-export trie entries.
    pub indirect_aliases: Vec<(SymbolId, SymbolId)>,
    /// The symbols export lists would re-export that a library the
    /// image re-exports whole exports already (see
    /// passes::warn_redundant_reexports).
    pub redundant_reexports: Vec<SymbolId>,
    /// The property lists category merging writes, which ld-prime's
    /// -map lists as anonymous atoms of its own.
    pub objc_property_lists: Vec<u32>,
    /// For each class_ro_t list pointer category merging set where the
    /// class had no list of the kind, the object defining the class:
    /// ld-prime's -map lists a dead pointer-sized atom of it for each.
    pub objc_filled_ro_fields: Vec<u32>,
    /// section$start/end and segment$start/end symbols to resolve
    /// after layout: (symbol, is_start, segment, section).
    pub boundary_syms: Vec<(SymbolId, bool, String, Option<String>)>,
    /// For -why_load: the symbol that made each object live, refreshed
    /// each resolution round.
    pub why_load: std::collections::HashMap<usize, &'static str>,
    /// For -why_load: the archives -force_load or -force-l loads whole.
    pub force_loaded: std::collections::HashSet<std::path::PathBuf>,
    /// For -t: every input file as it is loaded, by the path it was
    /// found at (a library inlined in a stub by its install name).
    pub traced_files: Vec<String>,
    /// -trace_implicit_libraries' lines of the libraries re-exports and
    /// archive members' auto-link options bring in, in the order they
    /// do (see passes::print_implicit_trace).
    pub implicit_trace: Vec<crate::passes::ImplicitTrace>,
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
    /// passes::report_32bit_pointer), as (subsection, offset) pairs.
    pub pointers32: std::sync::Mutex<Vec<(u32, u32)>>,
    /// Deduplication map for literal elements: (section type, contents)
    /// to the surviving subsection.
    pub literals: std::collections::HashMap<(u32, &'static [u8]), usize>,
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
            foreign_platform_dylibs: Vec::new(),
            dylib_renamings: Vec::new(),
            lto_plugin: None,
            lto_modules: Vec::new(),
            lto_objs: 0..0,
            lto_inputs: Vec::new(),
            merged_lto_obj: None,
            bitcode_duplicates: Vec::new(),
            stripped_imports: Vec::new(),
            visited_files: std::collections::HashSet::new(),
            reexport_files: Vec::new(),
            processed_linker_options: std::collections::HashSet::new(),
            cmdline_linker_options: None,
            linker_option_warnings: Vec::new(),
            autolink_misses: Vec::new(),
            possible_files: Vec::new(),
            common_conflict: None,
            stripped_dylibs: Vec::new(),
            indirect_files: Vec::new(),
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
            common_first_obj: None,
            folded_atom_names: hashbrown::HashMap::new(),
            dylib_load_seq: 0,
            autolinked_archives: hashbrown::HashMap::new(),
            indirect_aliases: Vec::new(),
            redundant_reexports: Vec::new(),
            objc_property_lists: Vec::new(),
            objc_filled_ro_fields: Vec::new(),
            boundary_syms: Vec::new(),
            why_load: std::collections::HashMap::new(),
            force_loaded: std::collections::HashSet::new(),
            traced_files: Vec::new(),
            implicit_trace: Vec::new(),
            tls_begin: 0,
            text_reloc_ranges: Vec::new(),
            text_relocs: std::sync::Mutex::new(Vec::new()),
            pointers32: std::sync::Mutex::new(Vec::new()),
            literals: std::collections::HashMap::new(),
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

    /// Address of the selector name string for objc stub `i`: an
    /// input's string of that name, else its own in the tail of the
    /// __objc_methname output section.
    pub fn objc_methname_addr(&self, i: usize) -> u64 {
        match self.objc_stubs.name_isec[i] {
            u32::MAX => {
                let osec = self.output_section(self.objc_stubs.methname.unwrap());
                osec.hdr.addr + osec.tail_off + self.objc_stubs.methname_offs[i]
            }
            isec => self.isec_addr(isec as usize),
        }
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

    /// Whether a lone CIE - one no FDE of its object points at - goes
    /// to the output: ld-prime carries it as any other atom of a live
    /// file, so that a -r output keeps it too, but dead stripping drops
    /// it. (A CIE whose FDEs all go, as those of functions with compact
    /// unwind records do, goes with them.) Its personality, if it names
    /// one, then gets a GOT slot for the CIE to point at.
    pub fn keeps_lone_cie(&self, cie: &crate::input_files::Cie) -> bool {
        !cie.has_fdes
            && self.objs[cie.obj as usize].is_alive
            && (self.args.relocatable || !self.strips_dead_code())
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

    /// Returns the output address of an input section. Layout stores
    /// every subsection's final address the moment its output section
    /// is placed (literal-merge losers borrow their survivor's), so
    /// this is one field read.
    /// A subsection's output address: its output section's address plus
    /// its offset there, as mold's isec.addr(ctx) derives it - not
    /// a cached field, which cost 8 bytes on every subsection. A
    /// literal-merge loser reports its surviving copy's address (the
    /// redirect is followed only when one exists, so the common case is
    /// one branch); an unplaced subsection reports 0.
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
            None => {
                error!("undefined symbol: {sym}");
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
    /// passes::create_lazy_loads).
    pub fn is_lazy_import(&self, id: SymbolId) -> bool {
        match self.symbols[id].file() {
            Some(FileId::Dylib(d)) => d != u32::MAX && self.dylibs[d as usize].is_lazy,
            _ => false,
        }
    }

    /// The address of the pointer slot stub `i` (for symbol `id`)
    /// jumps through: its lazy pointer, or its GOT slot. A weak
    /// definition of this image always goes through its GOT slot (the
    /// lazy binder cannot do weak lookup), as in ld64, and so does a
    /// branch shim.
    pub fn stub_ptr_addr(&self, i: usize, id: SymbolId) -> u64 {
        if self.args.lazy_binding && !self.binds_weak_lookup(id) && !self.has_branch_shim(id) {
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

    /// A weak reference to an overlay's __swift_FORCE_LOAD_$_ marker.
    /// The Swift compiler emits one per module to keep the overlay
    /// loaded; ld-prime keeps the dylib as a dependency but writes no
    /// fixup for the slot (it stays zero), and so do we.
    pub fn is_swift_force_load_ref(&self, id: SymbolId) -> bool {
        let sym = &self.symbols[id];
        sym.is_imported() && sym.is_weak_ref() && sym.name().starts_with("__swift_FORCE_LOAD_$_")
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
            && args.interposable.as_ref().is_some_and(|g| g.find(sym.name().as_bytes()) != -1);
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

    /// The library ordinal a bind of this symbol names: its dylib's, or
    /// for an interposable export, the flat lookup under
    /// -flat_namespace, else the image itself.
    pub fn sym_bind_ordinal(&self, id: SymbolId) -> i32 {
        match self.symbols[id].file() {
            Some(FileId::Dylib(dylib)) => self.bind_ordinal(dylib),
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
        // takes its weak definitions through __weak_got as ld-prime
        // links an arm64 kext.
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
            && (sym.name().starts_with("_OBJC_CLASS_$_")
                || sym.name().starts_with("_OBJC_METACLASS_$_"))
    }

    /// True if `id` has a branch shim (see branch_shims): a stub of a
    /// symbol defined here, which jumps through a GOT slot filled in
    /// here, for the branches from 4 GiB away.
    pub fn has_branch_shim(&self, id: SymbolId) -> bool {
        let aux = self.sym_aux(id);
        aux.stub_idx != crate::symbol::NO_IDX
            && aux.got_idx != crate::symbol::NO_IDX
            && !self.binds_at_runtime(id)
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
        self.got.slot_addr(self.objc_stubs.msgsend_got_idx as usize)
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

    /// The symbol that names the atom (subsection) `id` in ld-prime's
    /// diagnostics: of those at its start, the one
    /// input_files::atom_name_rank ranks first. ld-prime merges a
    /// literal by its content (see input_files::has_merged_atoms): the
    /// labels a compiler or assembler makes for itself (see
    /// input_files::is_private_label) name none, and but for an ltmpN,
    /// one takes the literal's bytes from a symbol beside it, leaving it
    /// unnamed (see atom_ordinal).
    pub fn atom_label(&self, id: usize) -> Option<&'static str> {
        use crate::input_files::is_private_label;
        let isec = &self.isecs[id];
        let obj = &self.objs[isec.file as usize];
        let merged = crate::input_files::has_merged_atoms(self.hdr_of(isec));
        let labels = obj
            .nlists
            .iter()
            .zip(&obj.symbols)
            .filter(|(n, _)| {
                !n.is_stab()
                    && n.n_type() == crate::macho::N_SECT
                    && n.n_sect as u32 == isec.shndx + 1
                    && n.n_value == isec.input_addr as u64
            })
            .map(|(n, &id)| (n, self.symbols[id].name()));
        if merged
            && labels.clone().any(|(_, name)| is_private_label(name) && !name.starts_with("ltmp"))
        {
            return None;
        }
        labels
            .filter(|(_, name)| !(merged && is_private_label(name)))
            .map(|(n, name)| (crate::input_files::atom_name_rank(n, name), name))
            .max()
            .map(|(_, name)| name)
    }

    /// The name ld-prime gives the atom (subsection) `id` in a
    /// diagnostic: its label, or else "anon-N" for the object's Nth atom
    /// (see atom_ordinal).
    pub fn atom_name(&self, id: usize) -> std::borrow::Cow<'static, str> {
        match self.atom_label(id) {
            Some(name) => name.into(),
            None => format!("anon-{}", self.atom_ordinal(id)).into(),
        }
    }

    /// The number ld-prime gives atom (subsection) `id` among its
    /// object's atoms, the N of an unnamed one's "anon-N". It numbers
    /// them section by section in section header order, and by address
    /// within a section. It makes none of an empty section no label is
    /// in, of __eh_frame and __objc_imageinfo, or of the sections it
    /// drops (the __DWARF and __LLVM segments, and __LD's but
    /// __compact_unwind, each of whose 32-byte records is an atom).
    /// Besides the atoms mold's subsections stand for, each label that
    /// doesn't name one is an atom of its own, of no size: a second
    /// label at a place, numbered before the atom with the bytes (which
    /// an L label takes on a literal), and one inside an atom - an
    /// alternate entry point, or in an object without subsections any
    /// label past a section's start - but for a literal's or a
    /// fixed-size record's. An arm64 assembler's ltmpN counts only in an
    /// object without subsections, and after a literal's atom or a class
    /// reference's, which ld-prime coalesces by content too.
    pub fn atom_ordinal(&self, id: usize) -> usize {
        use crate::input_files::{has_merged_atoms, is_record_section};
        let obj = &self.objs[self.isecs[id].file as usize];
        let split = obj.subsections_via_symbols;

        // The object's labels, by section and address: (section,
        // address, the order of the atoms of the labels at one place,
        // whether it is an alternate entry point).
        let mut labels: Vec<(u32, u64, u8, bool)> = obj
            .nlists
            .iter()
            .zip(&obj.symbols)
            .filter(|(n, _)| !n.is_stab() && n.n_type() == crate::macho::N_SECT && n.n_sect != 0)
            .filter_map(|(n, &sym)| {
                let name = self.symbols[sym].name();
                let order = if name.starts_with("ltmp") {
                    2
                } else {
                    crate::input_files::is_private_label(name) as u8
                };
                let alt_entry = n.n_desc & crate::macho::N_ALT_ENTRY != 0;
                (order != 2 || !split).then_some((n.n_sect as u32 - 1, n.n_value, order, alt_entry))
            })
            .collect();
        labels.sort_unstable();

        let mut subs = obj.subsecs.clone();
        subs.sort_unstable_by_key(|&i| (self.isecs[i].shndx, self.isecs[i].input_addr));

        let mut n = 0;
        for (shndx, hdr) in obj.sect_hdrs.iter().enumerate() {
            let shndx = shndx as u32;
            if (hdr.segname(), hdr.sectname()) == ("__LD", "__compact_unwind") {
                n += (hdr.size / 32) as usize;
                continue;
            }
            if hdr.segname() == "__LLVM" {
                continue;
            }
            let lo = subs.partition_point(|&i| self.isecs[i].shndx < shndx);
            let hi = subs.partition_point(|&i| self.isecs[i].shndx <= shndx);
            let sect_subs = &subs[lo..hi];
            let lo = labels.partition_point(|l| l.0 < shndx);
            let hi = labels.partition_point(|l| l.0 <= shndx);
            let sect_labels = &labels[lo..hi];
            let merged = has_merged_atoms(hdr) || hdr.sectname() == "__objc_classrefs";
            let records = is_record_section(hdr);
            for (j, &sub) in sect_subs.iter().enumerate() {
                let isec = &self.isecs[sub];
                let start = isec.input_addr as u64;
                let at_start = sect_labels.iter().filter(|l| l.1 == start);
                let k = at_start.clone().count();
                let (count, content) = if merged {
                    let beside = at_start.filter(|l| l.2 != 2).count();
                    (k.max(1), beside.saturating_sub(1))
                } else {
                    let end = match sect_subs.get(j + 1) {
                        Some(&next) => self.isecs[next].input_addr as u64,
                        None => hdr.addr + hdr.size + 1,
                    };
                    let inner = sect_labels
                        .iter()
                        .filter(|l| start < l.1 && l.1 < end && (l.3 || !split) && !records)
                        .count();
                    (k.max((isec.size > 0) as usize) + inner, k.saturating_sub(1))
                };
                if sub as usize == id {
                    return n + content;
                }
                n += count;
            }
        }
        n
    }

    /// Reports that stub `i` can't reach its pointer, as ld-prime does:
    /// a fixup error of the atoms it makes the stubs of, in its
    /// "stubs-got-file", whose first stub is anon-2 (anon-6 with lazy
    /// binding, the stub helper's atoms first). `off` is the offset of
    /// the field in the stub.
    pub fn stub_fixup_error(&self, i: usize, off: u32, kind: &str, msg: std::fmt::Arguments) {
        let atom = if self.stubs.lazy.is_empty() { 2 } else { 6 } + i;
        let fileoff = self.stubs.hdr.fileoff + i as u64 * E::STUB_SIZE;
        self.synthetic_fixup_error("stubs-got-file", atom, fileoff, off, kind, msg);
    }

    /// Reports a relocation that can't be applied `off` bytes into
    /// anon-`atom` of `file`, one of the files ld-prime makes its own
    /// atoms in, as it does (see fixup_error). The atom is `fileoff`
    /// bytes into the output file.
    pub fn synthetic_fixup_error(
        &self,
        file: &str,
        atom: usize,
        fileoff: u64,
        off: u32,
        kind: &str,
        msg: std::fmt::Arguments,
    ) {
        let at = fileoff + off as u64;
        match off {
            0 => crate::layout_error_at!(
                at,
                "fixup error (kind={kind}) at 'anon-{atom}' from {file}, {msg}"
            ),
            _ => crate::layout_error_at!(
                at,
                "fixup error (kind={kind}) at 'anon-{atom}'+0x{off:X} from {file}, {msg}"
            ),
        }
    }

    /// How a fixup error names the target of relocation `rel` of object
    /// `obj`: by its symbol, or else by the label of the atom it points
    /// into, if that has one - as for a label an assembler made for
    /// itself on a literal (see literal_label_target).
    pub fn fixup_target_name(&self, obj: usize, rel: &Reloc) -> &'static str {
        match self.literal_label_target(obj, rel) {
            Some(isec) => self.atom_label(isec).unwrap_or(""),
            None => match rel.target() {
                RelocTarget::Sym(idx) => self.symbols[self.objs[obj].symbols[idx as usize]].name(),
                RelocTarget::Section(idx) => self.atom_label(idx as usize).unwrap_or(""),
            },
        }
    }

    /// How a fixup error names the target of branch `rel` of object
    /// `obj`: a branch through a stub (to an import, or to a definition
    /// dyld may interpose; see branch_target_addr) goes to an atom of
    /// ld-prime's "stubs-got-file", which has no name, as a GOT slot
    /// hasn't. Any other target is named as by fixup_target_name.
    pub fn branch_target_name(&self, obj: usize, rel: &Reloc) -> &'static str {
        match self.reloc_target_sym(obj, rel) {
            Some(id)
                if self.sym_aux(id).stub_idx != crate::symbol::NO_IDX
                    && (self.symbols[id].is_imported() || self.is_interposable(id)) =>
            {
                ""
            }
            _ => self.fixup_target_name(obj, rel),
        }
    }

    /// How a text-relocation diagnostic names the target of relocation
    /// `rel` of object `obj`: by its symbol, or else by the atom it
    /// points into - as for a label an assembler made for itself on a
    /// literal (see literal_label_target). A class reference slot folded
    /// into the GOT is its class's GOT entry, an atom of ld-prime's
    /// stubs-got-file (see stubs_got_ordinal); one left in place is an
    /// atom no label names, whatever labels it has: the copy of it
    /// ld-prime keeps.
    pub fn text_reloc_target_name(
        &self,
        obj: usize,
        rel: &Reloc,
    ) -> std::borrow::Cow<'static, str> {
        if let Some(class) = self.folded_classref_target(obj, rel) {
            return format!("anon-{}", self.stubs_got_ordinal(class)).into();
        }
        if let Some(isec) = self.reloc_target_isec(obj, rel)
            && self.hdr_of(&self.isecs[isec]).sectname() == "__objc_classrefs"
        {
            return format!("anon-{}", self.atom_ordinal(self.resolve_isec(isec))).into();
        }
        match self.literal_label_target(obj, rel) {
            Some(isec) => self.atom_name(isec),
            None => match rel.target() {
                RelocTarget::Sym(idx) => {
                    self.symbols[self.objs[obj].symbols[idx as usize]].name().into()
                }
                RelocTarget::Section(idx) => self.atom_name(idx as usize),
            },
        }
    }

    /// The class whose GOT entry stands in for the class reference slot
    /// relocation `rel` of object `obj` points to, if the slot is folded
    /// into the GOT (see objc::fold_objc_classrefs).
    fn folded_classref_target(&self, obj: usize, rel: &Reloc) -> Option<SymbolId> {
        let slot = match rel.target() {
            RelocTarget::Sym(idx) => {
                self.symbols[self.objs[obj].symbols[idx as usize]].input_section()? as usize
            }
            RelocTarget::Section(idx) => idx as usize,
        };
        let isec = &self.isecs[slot];
        if self.hdr_of(isec).sectname() != "__objc_classrefs" {
            return None;
        }
        let stand_in = self.got.stand_ins.iter().find(|&&(id, _)| id == isec.replacement)?;
        Some(stand_in.1)
    }

    /// The number ld-prime gives `class`'s GOT entry among the atoms of
    /// its "stubs-got-file", the N of its "anon-N". It makes them as it
    /// meets the references, object by object in address order: two for
    /// each GOT entry, and one for a stub, after its GOT entry's. A lazy
    /// stub has no GOT entry but a lazy pointer and a stub helper entry,
    /// three atoms in all, after the four of the stub helper's header.
    fn stubs_got_ordinal(&self, class: SymbolId) -> usize {
        use crate::target::RelocClass;
        let mut with_got = hashbrown::HashSet::new();
        let mut with_stub = hashbrown::HashSet::new();
        let mut has_helper = false;
        let mut n = 0;
        for (obj_idx, obj) in self.objs.iter().enumerate().filter(|(_, obj)| obj.is_alive) {
            let mut subs = obj.subsecs.clone();
            subs.sort_unstable_by_key(|&i| (self.isecs[i].shndx, self.isecs[i].input_addr));
            for isec in subs {
                let isec = isec as usize;
                if !self.isecs[isec].is_alive() {
                    continue;
                }
                for rel in self.isec_relocs(isec) {
                    let (id, kind) = match self.folded_classref_target(obj_idx, rel) {
                        Some(id) => (id, RelocClass::Got),
                        None => match self.reloc_target_sym(obj_idx, rel) {
                            Some(id) => (id, E::classify_reloc(rel.r_type)),
                            None => continue,
                        },
                    };
                    let aux = self.sym_aux(id);
                    let stub = kind == RelocClass::Branch && aux.stub_idx != crate::symbol::NO_IDX;
                    let lazy = stub && self.stubs.lazy.contains(&aux.stub_idx);
                    let got = aux.got_idx != crate::symbol::NO_IDX
                        && (matches!(kind, RelocClass::Got | RelocClass::GotLoad) || stub && !lazy);
                    if got && with_got.insert(id) {
                        if id == class {
                            return n;
                        }
                        n += 2;
                    }
                    if stub && with_stub.insert(id) {
                        if lazy && !has_helper {
                            n += 4;
                            has_helper = true;
                        }
                        n += if lazy { 3 } else { 1 };
                    }
                }
            }
        }
        n
    }

    /// The literal (subsection of object `obj`) relocation `rel` points
    /// into through a label a compiler or assembler made for itself (see
    /// input_files::is_private_label) on literals ld-prime merges by
    /// content, which it takes for a reference to the literal: an arm64
    /// assembler refers to a literal by such a label, and an addend.
    fn literal_label_target(&self, obj: usize, rel: &Reloc) -> Option<usize> {
        let RelocTarget::Sym(idx) = rel.target() else { return None };
        let obj = &self.objs[obj];
        // A symbol the linker gave the object has no nlist (see
        // name_classref_targets).
        let nlist = obj.nlists.get(idx as usize)?;
        let name = self.symbols[obj.symbols[idx as usize]].name();
        if nlist.is_stab()
            || nlist.n_type() != crate::macho::N_SECT
            || !crate::input_files::is_private_label(name)
            || !crate::input_files::has_merged_atoms(&obj.sect_hdrs[nlist.n_sect as usize - 1])
        {
            return None;
        }
        let addr = nlist.n_value.wrapping_add_signed(rel.addend);
        crate::input_files::find_subsec(&self.isecs, &obj.subsecs, addr).map(|(id, _)| id)
    }

    /// Reports a relocation that can't be applied where it is, `offset`
    /// bytes into atom `isec`, as ld-prime does: naming the fixup's
    /// kind as ld-prime calls it, and the object by its leaf name (an
    /// archive member's archive[index](member)).
    pub fn fixup_error(&self, isec: usize, offset: u32, kind: &str, msg: std::fmt::Arguments) {
        let sec = &self.isecs[isec];
        let obj = &self.objs[sec.file as usize];
        let path = crate::passes::resolved_file_name(obj.mf);
        let file = path.rsplit_once('/').map_or(path.as_str(), |(_, leaf)| leaf);
        let atom = self.atom_name(isec);
        let osec = self.chunk_header(sec.output_section().unwrap());
        let at = osec.fileoff + sec.offset as u64 + offset as u64;
        if offset == 0 {
            crate::layout_error_at!(at, "fixup error (kind={kind}) at '{atom}' from {file}, {msg}");
        } else {
            crate::layout_error_at!(
                at,
                "fixup error (kind={kind}) at '{atom}'+0x{offset:X} from {file}, {msg}"
            );
        }
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
            Some(id) if self.is_absolute_symbol(id) || self.is_swift_force_load_ref(id) => false,
            _ => slides && !self.reloc_target_is_tls(file, rel),
        };
        if needs_fixup {
            self.text_relocs.lock().unwrap().push((isec as u32, i as u32));
        }
    }

    /// Names the place `offset` bytes into atom `isec` as ld-prime's
    /// other diagnostics do: "'atom'+0xOFF (path)", with the object's
    /// full path.
    pub fn atom_ref(&self, isec: usize, offset: u32) -> String {
        let path = crate::passes::resolved_file_name(self.objs[self.isecs[isec].file as usize].mf);
        let atom = self.atom_name(isec);
        if offset == 0 {
            format!("'{atom}' ({path})")
        } else {
            format!("'{atom}'+0x{offset:X} ({path})")
        }
    }
}
