//! The linker context: everything about one link, from parsed arguments
//! to output chunks.

use std::marker::PhantomData;

use crate::chunks::bind_info::BindInfoSection;
use crate::chunks::chained_fixups::ChainedFixupsSection;
use crate::chunks::code_signature::CodeSignatureSection;
use crate::chunks::data_in_code::DataInCodeSection;
use crate::chunks::eh_frame::EhFrameSection;
use crate::chunks::export_trie::ExportTrieSection;
use crate::chunks::function_starts::FunctionStartsSection;
use crate::chunks::got::GotSection;
use crate::chunks::indirect_symtab::IndirectSymtabSection;
use crate::chunks::init_offsets::InitOffsetsSection;
use crate::chunks::lazy_bind_info::LazyBindInfoSection;
use crate::chunks::lazy_ptrs::LazyPtrsSection;
use crate::chunks::objc_imageinfo::ObjcImageInfoSection;
use crate::chunks::objc_methlist::ObjcMethlistSection;
use crate::chunks::objc_stubs::ObjcStubsSection;
use crate::chunks::rebase_info::RebaseInfoSection;
use crate::chunks::sectcreate::SectCreateSection;
use crate::chunks::strtab::StrtabSection;
use crate::chunks::stub_helper::StubHelperSection;
use crate::chunks::stubs::StubsSection;
use crate::chunks::symtab::SymtabSection;
use crate::chunks::thread_ptrs::ThreadPtrsSection;
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
            ChunkId::ThreadPtrs => &$($mutable)? $ctx.thread_ptrs.hdr,
            ChunkId::ObjcStubs => &$($mutable)? $ctx.objc_stubs.hdr,
            ChunkId::ObjcMethlist => &$($mutable)? $ctx.objc_methlist.hdr,
            ChunkId::ObjcImageInfo => &$($mutable)? $ctx.objc_imageinfo.hdr,
            ChunkId::SectCreate(i) => &$($mutable)? $ctx.sectcreate_sections[i as usize].hdr,
            ChunkId::InitOffsets => &$($mutable)? $ctx.init_offsets.hdr,
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
    /// Files already loaded, so a library named twice (command line
    /// plus auto-link) is read once.
    pub visited_files: std::collections::HashSet<std::path::PathBuf>,
    /// The loaded libLTO, once a bitcode input has been seen.
    pub lto_plugin: Option<crate::lto::Plugin>,
    /// Bitcode modules registered for LTO: the pseudo object index and
    /// the lto_module handle.
    pub lto_modules: Vec<(usize, usize)>,
    /// Auto-link options already acted on.
    pub processed_linker_options: std::collections::HashSet<Vec<Vec<u8>>>,
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
    pub thread_ptrs: ThreadPtrsSection,
    pub objc_stubs: ObjcStubsSection,
    pub objc_methlist: ObjcMethlistSection,
    pub objc_imageinfo: ObjcImageInfoSection,
    pub sectcreate_sections: Vec<SectCreateSection>,
    pub init_offsets: InitOffsetsSection,
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
    pub indirect_symtab: IndirectSymtabSection,
    pub symtab: SymtabSection,
    pub strtab: StrtabSection,
    pub code_signature: CodeSignatureSection,
    /// Sequence number of the next dylib named on the command line or
    /// by an auto-link option; orders their load commands.
    pub dylib_load_seq: u32,
    /// Objective-C data records the linker synthesized (see
    /// merge_objc_categories), each placed as the tail of the output
    /// section it names.
    pub data_blobs: Vec<crate::passes::DataBlob>,
    /// Local symbols the linker names itself, on synthesized data:
    /// ld64's __OBJC_$_INSTANCE_METHODS_Foo(A|B) on a merged method
    /// list, and the like. (name, subsection).
    pub extra_local_syms: Vec<(&'static str, u32)>,
    /// -alias and selective reexports: (alias, imported target).
    /// Emitted as N_INDR symbols and re-export trie entries.
    pub indirect_aliases: Vec<(SymbolId, SymbolId)>,
    /// section$start/end and segment$start/end symbols to resolve
    /// after layout: (symbol, is_start, segment, section).
    pub boundary_syms: Vec<(SymbolId, bool, String, Option<String>)>,
    /// For -why_load: the symbol that made each object live, refreshed
    /// each resolution round.
    pub why_load: std::collections::HashMap<usize, &'static str>,
    /// The address of the first thread-local data section. Thread
    /// pointers are encoded relative to it.
    pub tls_begin: u64,
    /// Deduplication map for literal elements: (section type, contents)
    /// to the surviving subsection.
    pub literals: std::collections::HashMap<(u32, &'static [u8]), usize>,
    /// The output's UUID, computed from its contents.
    pub uuid: std::sync::Mutex<[u8; 16]>,
    /// The resolved address of the entry point symbol.
    pub entry_addr: u64,
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
            lto_plugin: None,
            lto_modules: Vec::new(),
            visited_files: std::collections::HashSet::new(),
            processed_linker_options: std::collections::HashSet::new(),
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
            thread_ptrs: ThreadPtrsSection::new(),
            objc_stubs: ObjcStubsSection::new(),
            objc_methlist: ObjcMethlistSection::new(),
            objc_imageinfo: ObjcImageInfoSection::new(),
            sectcreate_sections: Vec::new(),
            init_offsets: InitOffsetsSection::new(),
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
            indirect_symtab: IndirectSymtabSection::new(),
            symtab: SymtabSection::new(),
            strtab: StrtabSection::new(),
            code_signature: CodeSignatureSection::new(),
            data_blobs: Vec::new(),
            extra_local_syms: Vec::new(),
            dylib_load_seq: 0,
            indirect_aliases: Vec::new(),
            boundary_syms: Vec::new(),
            why_load: std::collections::HashMap::new(),
            tls_begin: 0,
            literals: std::collections::HashMap::new(),
            uuid: std::sync::Mutex::new([0; 16]),
            entry_addr: 0,
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

    /// Address of the selector reference slot for objc stub `i` (or,
    /// past the stubs, extra selector reference `i - stubs`): an
    /// input's slot the stub reuses, else its slot in the tail of the
    /// __objc_selrefs output section.
    pub fn objc_selref_addr(&self, i: usize) -> u64 {
        // (There is no tail chunk when every stub reuses a slot and no
        // extra reference exists.)
        let stubs = &self.objc_stubs;
        let tail = |slot: usize| {
            let osec = self.output_section(stubs.selrefs.unwrap());
            osec.hdr.addr + osec.tail_off + slot as u64 * 8
        };
        if i < stubs.symbols.len() {
            let reused = stubs.selref[i];
            if reused != u32::MAX {
                return self.isec_addr(reused as usize);
            }
            tail(stubs.tail[i] as usize)
        } else {
            tail(stubs.tail_slots + (i - stubs.symbols.len()))
        }
    }

    /// True if selector stub `i` loads an input's selector reference
    /// rather than a synthesized slot.
    pub fn objc_stub_reuses_selref(&self, i: usize) -> bool {
        self.objc_stubs.selref.get(i).is_some_and(|&s| s != u32::MAX)
    }

    /// Address of the synthesized selector name string for objc stub
    /// `i`: in the tail of the __objc_methname output section.
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

    /// The library ordinal in an undefined symbol's n_desc:
    /// EXECUTABLE_ORDINAL (0xff) for the -bundle_loader executable,
    /// DYNAMIC_LOOKUP_ORDINAL (0xfe) for flat lookup, else the dylib's.
    pub fn nlist_library_ordinal(&self, dylib: u32) -> u8 {
        match self.bind_ordinal(dylib) {
            crate::macho::BIND_SPECIAL_DYLIB_MAIN_EXECUTABLE => 0xff,
            n => n as u8,
        }
    }

    /// Returns the bind ordinal for a symbol imported from `dylib`:
    /// the dylib's load-command ordinal under two-level namespace, or
    /// the flat-lookup sentinel with -flat_namespace / dynamic lookup.
    pub fn bind_ordinal(&self, dylib: u32) -> i32 {
        if self.args.flat_namespace || dylib == u32::MAX {
            crate::macho::BIND_SPECIAL_DYLIB_FLAT_LOOKUP
        } else {
            self.dylibs[dylib as usize].dylib_idx
        }
    }

    /// Returns the next input-order priority value.
    pub fn next_priority(&mut self) -> u32 {
        self.priority_counter += 1;
        self.priority_counter
    }

    /// Returns true if the output is ad-hoc code signed. ld-prime signs
    /// arm64 images by default and leaves x86_64 ones unsigned (Intel
    /// Macs and Rosetta run unsigned code).
    pub fn adhoc_codesign(&self) -> bool {
        self.args.adhoc_codesign.unwrap_or(E::CPUTYPE == crate::macho::CPU_TYPE_ARM64)
    }

    /// Returns true if the output uses chained fixups rather than
    /// classic dyld rebase/bind opcodes.
    pub fn use_chained_fixups(&self) -> bool {
        // ld-prime's defaults: chained fixups from macOS 12 on arm64 and
        // from macOS 13 on x86_64 (below that, classic dyld info with
        // lazy binding), and never under -undefined dynamic_lookup or
        // suppress - only an explicit -fixup_chains overrides that.
        self.args.fixup_chains.unwrap_or_else(|| {
            if self.args.undefined_dynamic_lookup && !self.args.undefined_is_warning {
                return false;
            }
            let min = if E::CPUTYPE == crate::macho::CPU_TYPE_ARM64 { 12 } else { 13 };
            self.args.platform == crate::macho::PLATFORM_MACOS
                && self.args.platform_minos >= crate::macho::encode_version(min, 0, 0)
        })
    }

    /// Whether the file is the internal object holding synthesized
    /// sections and symbols.
    pub fn is_internal(&self, idx: usize) -> bool {
        self.internal_obj == Some(idx)
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
                        + self.sym_aux(id).objc_stub_idx as u64 * E::OBJC_STUB_SIZE
                } else {
                    sym.value
                }
            }
            // A branch to a dylib symbol goes through its stub. Other
            // references to dylib symbols are filled in by dyld; the
            // relocation scan has already validated them.
            Some(FileId::Dylib(_)) => {
                if self.sym_aux(id).stub_idx != crate::symbol::NO_IDX {
                    self.sym_stub_addr(id)
                } else {
                    0
                }
            }
        }
    }

    /// Returns the address of a symbol's __stubs entry.
    pub fn sym_stub_addr(&self, id: SymbolId) -> u64 {
        self.stubs.hdr.addr + self.sym_aux(id).stub_idx as u64 * E::STUB_SIZE
    }

    /// Whether imported functions are called through lazy pointers
    /// bound on first use (classic dyld info's __la_symbol_ptr and
    /// __stub_helper), as ld64 does below the chained-fixups
    /// deployment targets unless -bind_at_load.
    pub fn lazy_binding(&self) -> bool {
        !self.args.relocatable && !self.use_chained_fixups() && !self.args.bind_at_load
    }

    /// The address of the pointer slot stub `i` (for symbol `id`)
    /// jumps through: its lazy pointer, or its GOT slot. A weak
    /// definition of this image always goes through its GOT slot (the
    /// lazy binder cannot do weak lookup), as in ld64.
    pub fn stub_ptr_addr(&self, i: usize, id: SymbolId) -> u64 {
        if self.lazy_binding() && !self.binds_weak_lookup(id) {
            self.lazy_ptrs.hdr.addr + i as u64 * 8
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
    pub fn is_weak_coalesced(&self, id: SymbolId) -> bool {
        if self.args.relocatable {
            return false;
        }
        let sym = &self.symbols[id];
        matches!(sym.file(), Some(FileId::Obj(_)))
            && sym.is_weak_def()
            && sym.is_extern()
            && !sym.is_private_extern()
    }

    /// True if dyld fills the references to this symbol: an import, or
    /// a weak definition subject to coalescing.
    pub fn binds_at_runtime(&self, id: SymbolId) -> bool {
        self.symbols[id].is_imported() || self.is_weak_coalesced(id)
    }

    /// An input's N_ABS definition has no section and never slides.
    /// Sectionless symbols in the internal object instead describe the
    /// image (its header and layout boundaries), so their values slide.
    pub fn is_absolute_symbol(&self, id: SymbolId) -> bool {
        let sym = &self.symbols[id];
        sym.input_section().is_none()
            && matches!(sym.file(), Some(FileId::Obj(obj)) if !self.is_internal(obj as usize))
    }

    /// A PC-relative address computation cannot replace a GOT load of
    /// an absolute constant: the instruction slides but the value does not.
    pub fn can_relax_got(&self, id: SymbolId) -> bool {
        !self.binds_at_runtime(id) && !self.is_absolute_symbol(id)
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

    /// The address a branch to `id` targets: the symbol's stub when it
    /// has one and dyld may redirect it, else the symbol itself.
    pub fn branch_target_addr(&self, id: SymbolId) -> u64 {
        if self.is_weak_coalesced(id) && self.sym_aux(id).stub_idx != crate::symbol::NO_IDX {
            self.sym_stub_addr(id)
        } else {
            self.sym_addr(id)
        }
    }

    /// Returns the address of a symbol's __got slot.
    pub fn sym_got_addr(&self, id: SymbolId) -> u64 {
        self.got.hdr.addr + self.sym_aux(id).got_idx as u64 * 8
    }

    /// Returns the address of a symbol's __thread_ptrs slot.
    pub fn sym_tlv_ptr_addr(&self, id: SymbolId) -> u64 {
        self.thread_ptrs.hdr.addr + self.sym_aux(id).tlv_idx as u64 * 8
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
}
