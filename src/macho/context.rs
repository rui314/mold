//! The linker context: everything about one link, from parsed arguments
//! to output chunks.

use std::marker::PhantomData;

use crate::error;
use crate::macho::arch::Arch;
use crate::macho::cmdline::Args;
use crate::macho::format::{S_THREAD_LOCAL_REGULAR, S_THREAD_LOCAL_ZEROFILL};
use crate::macho::input_files::{DylibFile, FileId, ObjectFile};
use crate::macho::input_sections::{InputSection, Reloc, RelocTarget};
use crate::macho::output_chunks::chained_fixups::ChainedFixupsSection;
use crate::macho::output_chunks::dyld_info::{
    BindInfoSection, LazyBindInfoSection, RebaseInfoSection, WeakBindInfoSection,
};
use crate::macho::output_chunks::eh_frame::EhFrameSection;
use crate::macho::output_chunks::export_trie::ExportTrieSection;
use crate::macho::output_chunks::got::{
    GotSection, LazyPtrsSection, StubHelperSection, StubsSection, ThreadPtrsSection,
};
use crate::macho::output_chunks::misc::{
    CodeSignatureSection, DataInCodeSection, FunctionStartsSection, InitOffsetsSection,
    SectCreateSection,
};
use crate::macho::output_chunks::objc::{
    ObjcImageInfoSection, ObjcMethlistSection, ObjcStubsSection,
};
use crate::macho::output_chunks::symtab::{IndirectSymtabSection, StrtabSection, SymtabSection};
use crate::macho::output_chunks::unwind_info::UnwindInfoSection;
use crate::macho::output_chunks::{
    ChunkHeader, ChunkId, OutputMachHeader, OutputSection, OutputSectionId, OutputSegment,
};
use crate::macho::symbol::{SymbolId, SymbolTable};

pub struct Context<E: Arch> {
    pub args: Args,
    pub objs: Vec<ObjectFile>,
    pub dylibs: Vec<DylibFile>,
    pub symbols: SymbolTable,
    /// All input sections, in one arena.
    pub isecs: crate::macho::input_sections::InputSections,
    /// The object that owns the linker-synthesized sections and
    /// symbols, once created; mold-rust's internal_obj.
    pub internal_obj: Option<usize>,
    /// Per-symbol synthetic-slot indices (SymbolId-indexed), grown
    /// lazily; mold-rust's SymbolAux side table.
    pub sym_aux: Vec<crate::macho::symbol::SymAux>,
    /// Input-order counter for resolution tie-breaking.
    pub priority_counter: u32,
    /// Files already loaded, so a library named twice (command line
    /// plus auto-link) is read once.
    pub visited_files: std::collections::HashSet<String>,
    /// The loaded libLTO, once a bitcode input has been seen.
    pub lto_plugin: Option<crate::macho::lto::Plugin>,
    /// Bitcode modules registered for LTO: the pseudo object index and
    /// the lto_module handle.
    pub lto_modules: Vec<(usize, usize)>,
    /// Auto-link options already acted on.
    pub processed_linker_options: std::collections::HashSet<Vec<String>>,
    /// Unwind records from all objects' __compact_unwind sections.
    pub unwind_records: Vec<crate::macho::input_files::UnwindRecord>,
    /// DWARF CIEs and FDEs from all objects' __eh_frame sections.
    pub cies: Vec<crate::macho::input_files::Cie>,
    pub fdes: Vec<crate::macho::input_files::Fde>,
    /// The chunks of the output, in file order, and the segments they
    /// are grouped into. Each chunk's header and data live in the
    /// typed field for its kind below, as in mold-rust.
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
    pub data_blobs: Vec<crate::macho::passes::DataBlob>,
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
    _marker: PhantomData<E>,
}

impl<E: Arch> Context<E> {
    pub fn new(args: Args) -> Context<E> {
        Context {
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
            _marker: PhantomData,
        }
    }

    /// The header of any chunk.
    pub fn chunk_header(&self, id: ChunkId) -> &ChunkHeader {
        match id {
            ChunkId::MachHeader => &self.mach_header.hdr,
            ChunkId::Output(id) => &self.output_sections[id.index()].hdr,
            ChunkId::Stubs => &self.stubs.hdr,
            ChunkId::StubHelper => &self.stub_helper.hdr,
            ChunkId::LazyPtrs => &self.lazy_ptrs.hdr,
            ChunkId::Got => &self.got.hdr,
            ChunkId::ThreadPtrs => &self.thread_ptrs.hdr,
            ChunkId::ObjcStubs => &self.objc_stubs.hdr,
            ChunkId::ObjcMethlist => &self.objc_methlist.hdr,
            ChunkId::ObjcImageInfo => &self.objc_imageinfo.hdr,
            ChunkId::SectCreate(i) => &self.sectcreate_sections[i as usize].hdr,
            ChunkId::InitOffsets => &self.init_offsets.hdr,
            ChunkId::UnwindInfo => &self.unwind_info.hdr,
            ChunkId::EhFrame => &self.eh_frame.hdr,
            ChunkId::RebaseInfo => &self.rebase_info.hdr,
            ChunkId::BindInfo => &self.bind_info.hdr,
            ChunkId::WeakBindInfo => &self.weak_bind_info.hdr,
            ChunkId::LazyBindInfo => &self.lazy_bind_info.hdr,
            ChunkId::ChainedFixups => &self.chained_fixups.hdr,
            ChunkId::ExportTrie => &self.export_trie.hdr,
            ChunkId::FunctionStarts => &self.function_starts.hdr,
            ChunkId::DataInCode => &self.data_in_code.hdr,
            ChunkId::IndirectSymtab => &self.indirect_symtab.hdr,
            ChunkId::Symtab => &self.symtab.hdr,
            ChunkId::Strtab => &self.strtab.hdr,
            ChunkId::CodeSignature => &self.code_signature.hdr,
        }
    }

    pub fn chunk_header_mut(&mut self, id: ChunkId) -> &mut ChunkHeader {
        match id {
            ChunkId::MachHeader => &mut self.mach_header.hdr,
            ChunkId::Output(id) => &mut self.output_sections[id.index()].hdr,
            ChunkId::Stubs => &mut self.stubs.hdr,
            ChunkId::StubHelper => &mut self.stub_helper.hdr,
            ChunkId::LazyPtrs => &mut self.lazy_ptrs.hdr,
            ChunkId::Got => &mut self.got.hdr,
            ChunkId::ThreadPtrs => &mut self.thread_ptrs.hdr,
            ChunkId::ObjcStubs => &mut self.objc_stubs.hdr,
            ChunkId::ObjcMethlist => &mut self.objc_methlist.hdr,
            ChunkId::ObjcImageInfo => &mut self.objc_imageinfo.hdr,
            ChunkId::SectCreate(i) => &mut self.sectcreate_sections[i as usize].hdr,
            ChunkId::InitOffsets => &mut self.init_offsets.hdr,
            ChunkId::UnwindInfo => &mut self.unwind_info.hdr,
            ChunkId::EhFrame => &mut self.eh_frame.hdr,
            ChunkId::RebaseInfo => &mut self.rebase_info.hdr,
            ChunkId::BindInfo => &mut self.bind_info.hdr,
            ChunkId::WeakBindInfo => &mut self.weak_bind_info.hdr,
            ChunkId::LazyBindInfo => &mut self.lazy_bind_info.hdr,
            ChunkId::ChainedFixups => &mut self.chained_fixups.hdr,
            ChunkId::ExportTrie => &mut self.export_trie.hdr,
            ChunkId::FunctionStarts => &mut self.function_starts.hdr,
            ChunkId::DataInCode => &mut self.data_in_code.hdr,
            ChunkId::IndirectSymtab => &mut self.indirect_symtab.hdr,
            ChunkId::Symtab => &mut self.symtab.hdr,
            ChunkId::Strtab => &mut self.strtab.hdr,
            ChunkId::CodeSignature => &mut self.code_signature.hdr,
        }
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
            crate::macho::format::BIND_SPECIAL_DYLIB_MAIN_EXECUTABLE => -1i64,
            n => n as i64,
        };
        (ordinal as u64) & ((1u64 << bits) - 1)
    }

    /// The library ordinal in an undefined symbol's n_desc:
    /// EXECUTABLE_ORDINAL (0xff) for the -bundle_loader executable,
    /// DYNAMIC_LOOKUP_ORDINAL (0xfe) for flat lookup, else the dylib's.
    pub fn nlist_library_ordinal(&self, dylib: u32) -> u8 {
        match self.bind_ordinal(dylib) {
            crate::macho::format::BIND_SPECIAL_DYLIB_MAIN_EXECUTABLE => 0xff,
            n => n as u8,
        }
    }

    /// Returns the bind ordinal for a symbol imported from `dylib`:
    /// the dylib's load-command ordinal under two-level namespace, or
    /// the flat-lookup sentinel with -flat_namespace / dynamic lookup.
    pub fn bind_ordinal(&self, dylib: u32) -> i32 {
        if self.args.flat_namespace || dylib == u32::MAX {
            crate::macho::format::BIND_SPECIAL_DYLIB_FLAT_LOOKUP
        } else {
            self.dylibs[dylib as usize].dylib_idx
        }
    }

    /// Returns the next input-order priority value.
    pub fn next_priority(&mut self) -> u32 {
        self.priority_counter += 1;
        self.priority_counter
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
            let min = if E::CPUTYPE == crate::macho::format::CPU_TYPE_ARM64 { 12 } else { 13 };
            self.args.platform == crate::macho::format::PLATFORM_MACOS
                && self.args.platform_minos >= crate::macho::format::encode_version(min, 0, 0)
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
    pub fn add_synthetic_section(&mut self, hdr: crate::macho::format::MachSection) -> (u32, u32) {
        let file = self.internal_obj.expect("internal object not created yet");
        let hdrs = self.objs[file].sect_hdrs.to_mut();
        hdrs.push(hdr);
        (file as u32, (hdrs.len() - 1) as u32)
    }

    /// The parent section header of a subsection, through its object's
    /// section list - mold-rust resolves a section's shdr through its
    /// file the same way.
    #[inline]
    pub fn hdr_of(&self, isec: &InputSection) -> &crate::macho::format::MachSection {
        &self.objs[isec.file as usize].sect_hdrs[isec.shndx as usize]
    }

    /// Follows literal-merge redirects to the surviving subsection.
    pub fn resolve_isec(&self, mut id: usize) -> usize {
        while self.isecs[id].replacement != crate::macho::input_sections::NO_REPLACEMENT {
            id = self.isecs[id].replacement as usize;
        }
        id
    }

    /// A symbol's synthetic-slot indices, from the side table. Returns
    /// the all-absent default for symbols with no slots (the table is
    /// grown lazily by the first setter).
    pub fn sym_aux(&self, id: SymbolId) -> &crate::macho::symbol::SymAux {
        // Sparse, as mold-rust's SymbolAux: the symbol carries an index
        // into the table, NONE for the vast majority that have no slot.
        match self.symbols[id].aux_idx {
            crate::macho::symbol::NONE => &crate::macho::symbol::NONE_AUX,
            i => &self.sym_aux[i as usize],
        }
    }

    /// Mutable access to a symbol's slot indices, growing the side table
    /// to cover it. Called only from the serial slot-assignment passes.
    pub fn sym_aux_mut(&mut self, id: SymbolId) -> &mut crate::macho::symbol::SymAux {
        Self::sym_aux_mut_in(&mut self.symbols, &mut self.sym_aux, id)
    }

    /// `sym_aux_mut` over the two tables it touches, for callers that
    /// hold another part of the context borrowed at the same time.
    pub fn sym_aux_mut_in<'a>(
        symtab: &mut crate::macho::symbol::SymbolTable,
        sym_aux: &'a mut Vec<crate::macho::symbol::SymAux>,
        id: SymbolId,
    ) -> &'a mut crate::macho::symbol::SymAux {
        // Allocate the symbol's entry on first use; the table holds only
        // the symbols that take a slot (mold-rust's sparse SymbolAux).
        if symtab[id].aux_idx == crate::macho::symbol::NONE {
            symtab[id].aux_idx = sym_aux.len() as u32;
            sym_aux.push(Default::default());
        }
        let i = symtab[id].aux_idx as usize;
        &mut sym_aux[i]
    }

    /// A subsection's relocations, sliced from its object's reloc arena
    /// (subsections keep only a rel_offset/nrels range, sold-style).
    pub fn isec_relocs(&self, id: usize) -> &[crate::macho::input_sections::Reloc] {
        let isec = &self.isecs[id];
        let off = isec.rel_offset as usize;
        &self.objs[isec.file as usize].relocs[off..off + isec.nrels as usize]
    }

    /// Returns the output address of an input section. Layout stores
    /// every subsection's final address the moment its output section
    /// is placed (literal-merge losers borrow their survivor's), so
    /// this is one field read.
    /// A subsection's output address: its output section's address plus
    /// its offset there, as mold-rust's isec.addr(ctx) derives it - not
    /// a cached field, which cost 8 bytes on every subsection. A
    /// literal-merge loser reports its surviving copy's address (the
    /// redirect is followed only when one exists, so the common case is
    /// one branch); an unplaced subsection reports 0.
    #[inline]
    pub fn isec_addr(&self, id: usize) -> u64 {
        let mut isec = &self.isecs[id];
        if isec.replacement != crate::macho::input_sections::NO_REPLACEMENT {
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
                } else if self.sym_aux(id).objc_stub_idx != crate::macho::symbol::NO_IDX {
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
                if self.sym_aux(id).stub_idx != crate::macho::symbol::NO_IDX {
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
        if self.is_weak_coalesced(id) && self.sym_aux(id).stub_idx != crate::macho::symbol::NO_IDX {
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
