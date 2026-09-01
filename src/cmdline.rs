//! Command-line argument parsing.

use std::collections::{HashMap, HashSet};
use std::io::IsTerminal;

use crate::arch;
use crate::elf::*;
use crate::error::Diagnostics;
use crate::mapped_file::MappedFile;
use crate::util::glob::Glob;
use crate::util::perf::Counter;
use crate::util::{self, align_down, path_clean, path_filename};
use crate::{fatal, out, warn};

const HELP: &str = "
Options:
  --help                      Report usage information
  -v, --version               Report version information
  -V                          Report version and target information
  -(, --start-group           Ignored
  -), --end-group             Ignored
  -C DIR, --directory DIR     Change to DIR before doing anything
  -E, --export-dynamic        Put symbols in the dynamic symbol table
    --no-export-dynamic
  -F LIBNAME, --filter LIBNAME
                              Set DT_FILTER to the specified value
  -I FILE, --dynamic-linker FILE
                              Set dynamic linker path
    --no-dynamic-linker
  -L DIR, --library-path DIR  Add DIR to library search path
  -M, --print-map             Write map file to stdout
  -N, --omagic                Do not page align data; do not make text readonly
    --no-omagic
  -O NUMBER                   Ignored
  -P AUDITLIB, --depaudit AUDITLIB
                              Set DT_DEPAUDIT to the specified value
  -S, --strip-debug           Strip .debug_* sections
  -T FILE, --script FILE      Read linker script
  -X, --discard-locals        Discard temporary local symbols (default)
  -e SYMBOL, --entry SYMBOL   Set program entry point
  -f SHLIB, --auxiliary SHLIB Set DT_AUXILIARY to the specified value
  -h LIBNAME, --soname LIBNAME
                              Set shared library name
  -l LIBNAME, --library LIBNAME
                              Search for a given library
  -m TARGET                   Set target
  -o FILE, --output FILE      Set output filename
  -q, --emit-relocs           Leaves relocation sections in the output
  -r, --relocatable           Generate relocatable output
  -s, --strip-all             Strip .symtab section
  -u SYMBOL, --undefined SYMBOL
                              Force to resolve SYMBOL
  -w, --no-warnings           Suppress warnings
  -y SYMBOL, --trace-symbol SYMBOL
                              Trace references to SYMBOL
  --Bdynamic, --dy            Link against shared libraries (default)
  --Bstatic, --dn, --static   Do not link against shared libraries
  --Bsymbolic                 Bind all symbols locally
  --Bsymbolic-functions       Bind function symbols locally
  --Bsymbolic-non-weak        Bind all but weak symbols locally
  --Bsymbolic-non-weak-functions
                              Bind all but weak function symbols locally
  --Bno-symbolic              Cancel --Bsymbolic options
  --Map FILE                  Write map file to a given file
  --Tbss=ADDR                 Set address to .bss
  --Tdata=ADDR                Set address to .data
  --Ttext=ADDR                Set address to .text
  --Ttext-segment=ADDR        Set address of text segment
  --allow-multiple-definition Allow multiple definitions
  --apply-dynamic-relocs      Apply link-time values for dynamic relocations (default)
    --no-apply-dynamic-relocs
  --as-needed                 Only set DT_NEEDED if used
    --no-as-needed
  --audit LIBNAME             Set DT_AUDIT to the specified value
  --build-id [none,md5,sha1,sha256,fast,uuid,HEXSTRING]
                              Generate build ID
    --no-build-id
  --chroot DIR                Set a given path to the root directory
  --color-diagnostics=[auto,always,never]
                              Use colors in diagnostics
  --color-diagnostics         Alias for --color-diagnostics=always
  --compress-debug-sections=[none,zlib,zlib:0,...,zlib:9,zstd,zstd:1,...,zstd:22]
                              Compress .debug_* sections
  --dc                        Ignored
  --dependency-file=FILE      Write Makefile-style dependency rules to FILE
  --defsym=SYMBOL=VALUE       Define a symbol alias
  --demangle                  Demangle C++ symbols in log messages (default)
    --no-demangle
  --detach                    Create separate debug info file in the background (default)
    --no-detach
  --discard-none              Keep all local symbols in the symbol table
  --enable-new-dtags          Emit DT_RUNPATH for --rpath (default)
    --disable-new-dtags       Emit DT_RPATH for --rpath
  --execute-only              Make executable segments unreadable
  --dp                        Ignored
  --dynamic-list=FILE         Read a list of dynamic symbols (implies -Bsymbolic)
  --dynamic-list-data         Add data symbols to dynamic symbols
  --eh-frame-hdr              Create .eh_frame_hdr section
    --no-eh-frame-hdr
  --exclude-libs LIB,LIB,..   Mark all symbols in given libraries as hidden
  --export-dynamic-symbol     Put symbols matching glob in the dynamic symbol table
  --export-dynamic-symbol-list=FILE
                              Read a list of dynamic symbols
  --fatal-warnings            Treat warnings as errors
    --no-fatal-warnings       Do not treat warnings as errors (default)
  --fini SYMBOL               Call SYMBOL at unload-time
  --fork                      Spawn a child process (default)
    --no-fork
  --gc-sections               Remove unreferenced sections
    --no-gc-sections
  --gdb-index                 Create .gdb_index for faster gdb startup
  --hash-style [sysv,gnu,both,none]
                              Set hash style
  --icf=[all,safe,none]       Fold identical code
    --no-icf
  --ignore-data-address-equality
                              Allow merging non-executable sections with --icf
  --image-base ADDR           Set the base address to a given value
  --init SYMBOL               Call SYMBOL at load-time
  --nmagic                    Do not page align sections
    --no-nmagic
  --no-undefined              Report undefined symbols (even with --shared)
  --noinhibit-exec            Create an output file even if errors occur
  --oformat=binary            Omit ELF, section, and program headers
  --pack-dyn-relocs=[relr,android,android+relr,none]
                              Pack dynamic relocations
  --package-metadata=PERCENT_ENCODED_STRING
                              Set a given string to .note.package
  --perf                      Print performance statistics
  --pie, --pic-executable     Create a position-independent executable
    --no-pie, --no-pic-executable
  --pop-state                 Restore the state of flags governing input file handling
  --print-gc-sections[=FILE]  Print, or save in FILE, removed unreferenced sections
    --no-print-gc-sections
  --print-icf-sections[=FILE] Print, or save in FILE, folded identical sections
    --no-print-icf-sections
  --push-state                Save the state of flags governing input file handling
  --quick-exit                Use quick_exit to exit (default)
    --no-quick-exit
  --relax                     Optimize instructions (default)
    --no-relax
  --repro                     Embed input files in .repro section
  --require-defined SYMBOL    Require SYMBOL be defined in the final output
  --retain-symbols-file FILE  Keep only symbols listed in FILE
  --reverse-sections          Reverse input sections in the output file
  --rosegment                 Put read-only non-executable sections in their own segment (default)
    --no-rosegment            Put read-only non-executable sections in an executable segment
  --rpath DIR                 Add DIR to the runtime search path
  --rpath-link DIR            Ignored
  --run COMMAND ARG...        Run COMMAND with mold as /usr/bin/ld
  --section-start=SECTION=ADDR Set address for section
  --separate-debug-file[=FILE] Separate debug info to the specified file
    --no-separate-debug-file
  --shared, --Bshareable      Create a shared library
  --shuffle-sections[=SEED]   Randomize the output by shuffling input sections
  --sort-common               Ignored
  --sort-section              Ignored
  --spare-dynamic-tags NUMBER Reserve the given number of tags in the .dynamic section
  --spare-program-headers NUMBER
                              Reserve the given number of slots in the program header
  --start-lib                 Give following object files in-archive-file semantics
    --end-lib                 End the effect of --start-lib
  --stats                     Print input statistics
  --sysroot DIR               Set the target system root directory
  --thread-count COUNT, --threads=COUNT
                              Use COUNT number of threads
  --threads                   Use multiple threads (default)
    --no-threads
  --trace                     Print the name of each input file
  --undefined-glob PATTERN    Force to resolve all symbols that match a given pattern
  --undefined-version         Do not report version scripts that refer to undefined symbols
    --no-undefined-version    Report version scripts that refer to undefined symbols (default)
  --unique PATTERN            Don't merge input sections that match a given pattern
  --unresolved-symbols [report-all,ignore-all,ignore-in-object-files,ignore-in-shared-libs]
                              Handle unresolved symbols
  --version-script FILE       Read version script
  --warn-common               Warn about common symbols
    --no-warn-common
  --warn-once                 Only warn once for each undefined symbol
  --warn-shared-textrel       Warn if the output .so needs text relocations
  --warn-textrel              Warn if the output file needs text relocations
  --warn-unresolved-symbols   Report unresolved symbols as warnings
    --error-unresolved-symbols
                              Report unresolved symbols as errors (default)
  --whole-archive             Include all objects from static archives
    --no-whole-archive
  --wrap SYMBOL               Use a wrapper function for a given symbol
  --zero-to-bss               Convert all-zero data sections into BSS
  -z defs                     Report undefined symbols (even with --shared)
    -z nodefs
  -z common-page-size=VALUE   Ignored
  -z execstack                Require an executable stack
    -z noexecstack
  -z execstack-if-needed      Make the stack area executable if an input file explicitly requests it
  -z initfirst                Mark DSO to be initialized first at runtime
  -z interpose                Mark object to interpose all DSOs but the executable
  -z keep-text-section-prefix Keep .text.{hot,unknown,unlikely,startup,exit} as separate sections in the final binary
    -z nokeep-text-section-prefix
  -z lazy                     Enable lazy function resolution (default)
  -z max-page-size=VALUE      Use VALUE as the memory page size
  -z nocopyreloc              Do not create copy relocations
  -z nodefaultlib             Make the dynamic loader ignore default search paths
  -z nodelete                 Mark DSO non-deletable at runtime
  -z nodlopen                 Mark DSO not available to dlopen
  -z nodump                   Mark DSO not available to dldump
  -z now                      Disable lazy function resolution
  -z origin                   Mark object requiring immediate $ORIGIN processing at runtime
  -z pack-relative-relocs     Alias for --pack-dyn-relocs=relr
    -z nopack-relative-relocs
  -z sectionheader            Do not omit section header (default)
    -z nosectionheader        Omit section header
  -z start_stop_visibility=[hidden,protected]
                              Specify symbol visibility for \"__start_SECNAME\" and \"__stop_SECNAME\" symbols
  -z separate-loadable-segments
                              Separate all loadable segments onto different pages
    -z separate-code          Separate code and data onto different pages
    -z noseparate-code        Allow overlap in pages
  -z stack-size=VALUE         Set the size of the stack segment
  -z relro                    Make some sections read-only after relocation (default)
    -z norelro
  -z rewrite-endbr            Rewrite indirect branch target instructions with NOPs
    -z norewrite-endbr
  -z rodynamic                Make the .dynamic section read-only
  -z text                     Report error if DT_TEXTREL is set
    -z notext
    -z textoff

mold: supported targets: elf32-i386 elf64-x86-64 elf32-littlearm elf64-littleaarch64 elf64-bigaarch64 elf32-littleriscv elf32-bigriscv elf64-littleriscv elf64-bigriscv elf32-powerpc elf64-powerpc elf64-powerpc elf64-powerpcle elf64-s390 elf64-sparc elf32-m68k elf32-sh-linux elf64-loongarch elf32-loongarch
mold: supported emulations: elf_i386 elf_x86_64 armelf_linux_eabi aarch64elf aarch64linux aarch64elfb aarch64linuxb elf32lriscv elf32briscv elf64lriscv elf64briscv elf32ppc elf32ppclinux elf64ppc elf64lppc elf64_s390 elf64_sparc m68kelf shlelf_linux shelf_linux elf64loongarch elf32loongarch";

pub const VERSION: &str = env!("MOLD_VERSION");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BuildIdKind {
    #[default]
    None,
    Hex,
    Hash,
    Uuid,
}

#[derive(Clone, Debug, Default)]
pub struct BuildId {
    pub kind: BuildIdKind,
    pub value: Vec<u8>,
    pub hash_size: usize,
}

impl BuildId {
    pub fn size(&self) -> usize {
        match self.kind {
            BuildIdKind::None => 0,
            BuildIdKind::Hex => self.value.len(),
            BuildIdKind::Hash => self.hash_size,
            BuildIdKind::Uuid => 16,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum UnresolvedKind {
    Error,
    Warn,
    #[default]
    Ignore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BsymbolicKind {
    #[default]
    None,
    All,
    Functions,
    NonWeak,
    NonWeakFunctions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SeparateCodeKind {
    SeparateLoadableSegments,
    SeparateCode,
    #[default]
    NoSeparateCode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CetReportKind {
    #[default]
    None,
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ShuffleSectionsKind {
    #[default]
    None,
    Shuffle,
    Reverse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionOrderKind {
    Section,
    Group,
    Addr,
    Align,
    Symbol,
}

#[derive(Clone, Debug)]
pub struct SectionOrder {
    pub kind: SectionOrderKind,
    pub name: String,
    pub value: u64,
    // for error reporting
    pub token: String,
}

/// The right-hand side of `--defsym=SYMBOL=VALUE`.
#[derive(Clone, Debug)]
pub enum DefsymValue {
    Addr(u64),
    Symbol(String),
}

/// A source of dynamic-list patterns, kept in command line order.
#[derive(Clone, Debug)]
pub enum DynamicListSource {
    File(String),
    Pattern(String),
}

// The position of the file currently being read in the command
// line. We read input files in parallel, so files are not read in
// the command line order; instead, we record each file's position
// when it's found and sort files by position afterwards.
//
// Positions are hierarchical: the n-th file found inside another
// file, such as an archive member or a file named by a GROUP linker
// script command, gets its parent file's position extended with n.
// Comparing positions lexicographically thus gives the command line
// order.
#[derive(Clone, Debug, Default)]
pub struct ReaderContext {
    pub as_needed: bool,
    pub in_lib: bool,
    pub is_static: bool,
    pub whole_archive: bool,

    /// The position of the file in the command line. Files inside other
    /// files (archive members, GROUP entries) extend their parent's
    /// position, so comparing positions lexicographically yields the
    /// command line order.
    pub pos: Vec<u32>,

    // The number of files found so far in the current file.
    pub num_children: u32,
}

impl ReaderContext {
    // Returns a context for the next file found inside the current file.
    pub fn next_child(&mut self) -> ReaderContext {
        let mut child = self.clone();
        child.pos.push(self.num_children);
        child.num_children = 0;
        self.num_children += 1;
        child
    }
}

// A file to read along with the reader state at its command line
// position. parse_nonpositional_args() creates one ReaderJob per
// input file argument; `name` is a path or, if `is_lib` is set, a
// library name to search for. read_input_files() additionally
// enqueues archive members as jobs in an already-opened form, with
// `mf` and `archive_name` set instead.
#[derive(Clone, Debug, Default)]
pub struct ReaderJob {
    pub rctx: ReaderContext,
    pub name: String,
    pub is_lib: bool,
    pub mf: Option<&'static MappedFile>,

    // For an archive member. A member of a regular archive is a slice of
    // the archive's mapping and comes already opened as `mf`; a member of
    // a thin archive is a separate file that the job opens by `name`.
    pub archive_name: String,
    pub thin_parent: Option<&'static MappedFile>,
}

/// All command line options.
#[derive(Debug)]
pub struct Args {
    pub bsymbolic: BsymbolicKind,
    pub build_id: BuildId,
    pub z_cet_report: CetReportKind,
    pub undefined_glob: Glob,
    pub unique: Glob,
    pub z_separate_code: SeparateCodeKind,
    pub shuffle_sections: ShuffleSectionsKind,
    pub entry: String,
    pub fini: String,
    pub init: String,
    pub unresolved_symbols: UnresolvedKind,
    pub allow_multiple_definition: bool,
    pub allow_shlib_undefined: bool,
    pub apply_dynamic_relocs: bool,
    pub be8: bool,
    pub color_diagnostics: bool,
    pub default_symver: bool,
    pub demangle: bool,
    pub detach: bool,
    pub discard_all: bool,
    pub discard_locals: bool,
    pub dynamic_list_data: bool,
    pub eh_frame_hdr: bool,
    pub emit_relocs: bool,
    pub enable_new_dtags: bool,
    pub execute_only: bool,
    pub export_dynamic: bool,
    pub fatal_warnings: bool,
    pub fork: bool,
    pub gc_sections: bool,
    pub gdb_index: bool,
    pub hash_style_gnu: bool,
    pub hash_style_sysv: bool,
    pub icf: bool,
    pub icf_all: bool,
    pub ignore_data_address_equality: bool,
    pub lto_pass2: bool,
    pub nmagic: bool,
    pub noinhibit_exec: bool,
    pub oformat_binary: bool,
    pub omagic: bool,
    pub pack_dyn_relocs_android: bool,
    pub pack_dyn_relocs_relr: bool,
    pub perf: bool,
    pub pic: bool,
    pub pie: bool,
    pub print_dependencies: bool,
    pub print_map: bool,
    pub quick_exit: bool,
    pub relax: bool,
    pub relocatable: bool,
    pub relocatable_merge_sections: bool,
    pub repro: bool,
    pub rosegment: bool,
    pub shared: bool,
    pub start_stop: bool,
    pub is_static: bool,
    pub stats: bool,
    pub strip_all: bool,
    pub strip_debug: bool,
    pub suppress_warnings: bool,
    pub trace: bool,
    pub undefined_version: bool,
    pub use_android_relr_tags: bool,
    pub warn_common: bool,
    pub warn_once: bool,
    pub warn_textrel: bool,
    pub z_copyreloc: bool,
    pub z_delete: bool,
    pub z_dlopen: bool,
    pub z_dump: bool,
    pub z_dynamic_undefined_weak: bool,
    pub z_execstack: bool,
    pub z_execstack_if_needed: bool,
    pub z_ibt: bool,
    pub z_initfirst: bool,
    pub z_interpose: bool,
    pub z_keep_text_section_prefix: bool,
    pub z_nodefaultlib: bool,
    pub z_now: bool,
    pub z_origin: bool,
    pub z_relro: bool,
    pub z_rewrite_endbr: bool,
    pub z_rodynamic: bool,
    pub z_sectionheader: bool,
    pub z_shstk: bool,
    pub z_start_stop_visibility_protected: bool,
    pub z_text: bool,
    pub zero_to_bss: bool,
    pub compress_debug_sections: u32,
    pub compress_debug_sections_level: i64,
    pub filler: Option<u8>,
    pub spare_dynamic_tags: i64,
    pub spare_program_headers: i64,
    pub z_stack_size: u64,
    pub thread_count: Option<usize>,
    pub retain_symbols_file: Option<Vec<String>>,
    pub physical_image_base: Option<u64>,
    pub ttext_segment: Option<u64>,
    pub map: String,
    pub audit: String,
    pub chroot: String,
    pub depaudit: String,
    pub dependency_file: String,
    pub directory: String,
    pub dynamic_linker: String,
    pub output: String,
    pub package_metadata: String,
    pub plugin: String,
    pub print_gc_sections: String,
    pub print_icf_sections: String,
    pub rpaths: String,
    pub separate_debug_file: String,
    pub soname: String,
    pub sysroot: String,
    pub emulation: String,
    pub section_align: HashMap<String, u64>,
    pub section_start: HashMap<String, u64>,
    pub discard_section: HashSet<String>,
    pub exclude_libs: HashSet<String>,
    pub ignore_ir_file: HashSet<String>,
    pub wrap: HashSet<String>,
    pub section_order: Vec<SectionOrder>,
    pub require_defined: Vec<String>,
    pub undefined: Vec<String>,
    pub defsyms: Vec<(String, DefsymValue)>,
    pub library_paths: Vec<String>,
    pub plugin_opt: Vec<String>,
    pub version_definitions: Vec<String>,
    pub version_scripts: Vec<String>,
    pub dynamic_list: Vec<DynamicListSource>,
    pub auxiliary: Vec<String>,
    pub filter: Vec<String>,
    pub trace_symbol: Vec<String>,
    pub z_x86_64_isa_level: u32,
    pub image_base: u64,
    pub shuffle_sections_seed: u64,
    pub page_size: u64,

    /// Whether an existing output file may be overwritten in place.
    pub overwrite_output_file: bool,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            bsymbolic: BsymbolicKind::None,
            build_id: BuildId::default(),
            z_cet_report: CetReportKind::None,
            undefined_glob: Glob::new(),
            unique: Glob::new(),
            z_separate_code: SeparateCodeKind::NoSeparateCode,
            shuffle_sections: ShuffleSectionsKind::None,
            entry: "_start".to_string(),
            fini: "_fini".to_string(),
            init: "_init".to_string(),
            unresolved_symbols: UnresolvedKind::Ignore,
            allow_multiple_definition: false,
            allow_shlib_undefined: true,
            apply_dynamic_relocs: true,
            be8: false,
            color_diagnostics: false,
            default_symver: false,
            demangle: true,
            detach: true,
            discard_all: false,
            discard_locals: true,
            dynamic_list_data: false,
            eh_frame_hdr: true,
            emit_relocs: false,
            enable_new_dtags: true,
            execute_only: false,
            export_dynamic: false,
            fatal_warnings: false,
            fork: true,
            gc_sections: false,
            gdb_index: false,
            hash_style_gnu: true,
            hash_style_sysv: true,
            icf: false,
            icf_all: false,
            ignore_data_address_equality: false,
            lto_pass2: false,
            nmagic: false,
            noinhibit_exec: false,
            oformat_binary: false,
            omagic: false,
            pack_dyn_relocs_android: false,
            pack_dyn_relocs_relr: false,
            perf: false,
            pic: false,
            pie: false,
            print_dependencies: false,
            print_map: false,
            quick_exit: true,
            relax: true,
            relocatable: false,
            relocatable_merge_sections: false,
            repro: false,
            rosegment: true,
            shared: false,
            start_stop: false,
            is_static: false,
            stats: false,
            strip_all: false,
            strip_debug: false,
            suppress_warnings: false,
            trace: false,
            undefined_version: false,
            use_android_relr_tags: false,
            warn_common: false,
            warn_once: false,
            warn_textrel: false,
            z_copyreloc: true,
            z_delete: true,
            z_dlopen: true,
            z_dump: true,
            z_dynamic_undefined_weak: true,
            z_execstack: false,
            z_execstack_if_needed: false,
            z_ibt: false,
            z_initfirst: false,
            z_interpose: false,
            z_keep_text_section_prefix: false,
            z_nodefaultlib: false,
            z_now: false,
            z_origin: false,
            z_relro: true,
            z_rewrite_endbr: false,
            z_rodynamic: false,
            z_sectionheader: true,
            z_shstk: false,
            z_start_stop_visibility_protected: false,
            z_text: false,
            zero_to_bss: false,
            compress_debug_sections: ELFCOMPRESS_NONE,
            compress_debug_sections_level: 0,
            filler: None,
            spare_dynamic_tags: 5,
            spare_program_headers: 0,
            z_stack_size: 0,
            thread_count: None,
            retain_symbols_file: None,
            physical_image_base: None,
            ttext_segment: None,
            map: String::new(),
            audit: String::new(),
            chroot: String::new(),
            depaudit: String::new(),
            dependency_file: String::new(),
            directory: String::new(),
            dynamic_linker: String::new(),
            output: "a.out".to_string(),
            package_metadata: String::new(),
            plugin: String::new(),
            print_gc_sections: String::new(),
            print_icf_sections: String::new(),
            rpaths: String::new(),
            separate_debug_file: String::new(),
            soname: String::new(),
            sysroot: String::new(),
            emulation: String::new(),
            section_align: HashMap::new(),
            section_start: HashMap::new(),
            discard_section: HashSet::new(),
            exclude_libs: HashSet::new(),
            ignore_ir_file: HashSet::new(),
            wrap: HashSet::new(),
            section_order: Vec::new(),
            require_defined: Vec::new(),
            undefined: Vec::new(),
            defsyms: Vec::new(),
            library_paths: Vec::new(),
            plugin_opt: Vec::new(),
            version_definitions: Vec::new(),
            version_scripts: Vec::new(),
            dynamic_list: Vec::new(),
            auxiliary: Vec::new(),
            filter: Vec::new(),
            trace_symbol: Vec::new(),
            z_x86_64_isa_level: 0,
            image_base: 0x200000,
            shuffle_sections_seed: 0,
            page_size: 0,
            overwrite_output_file: false,
        }
    }
}

/// Properties of the target that influence option defaults and validation.
#[derive(Clone, Copy, Debug)]
pub struct TargetTraits {
    pub name: &'static str,
    pub is_rela: bool,
    pub is_sparc: bool,
    pub is_riscv: bool,
    pub is_sh4: bool,
    pub is_x86_64: bool,
    pub is_arm64: bool,
    pub page_size: u64,
}

fn is_space(c: u8) -> bool {
    // Same as isspace() in the C locale, without the function call that the
    // tokenizer below would otherwise make for every byte of a response file.
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

// True for the characters that end or alter a token: whitespace, quotes
// and backslash. A table keeps the tokenizer's scan loop tight.
//
// Rust tests these characters directly in the tokenizer below.

// If a command line argument is in the form of `@path/to/some/file` (i.e.
// it starts with an atsign), the linker reads the given file and
// interprets its contents as a list of command line arguments. A file
// containing command line arguments is called a "response file".
//
// A response file is often used to pass a very large number of arguments
// to the linker without exceeding the kernel's command line length limit.
//
// This function opens a given file, tokenizes its contents, and returns a
// list of tokens.
fn read_response_file(diag: &Diagnostics, path: &str, depth: usize) -> Vec<String> {
    if depth > 10 {
        fatal!(diag, "{path}: response file nesting too deep");
    }

    let mf = MappedFile::must_open(diag, path);
    mf.set_dependency(false);
    let data = mf.data();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < data.len() {
        if is_space(data[i]) {
            i += 1;
            continue;
        }

        // The C++ tokenizer preserves this zero-copy fast path:
        // A token containing no quotes or backslashes, which is by far the
        // common case, is returned as a substring of the file.
        //
        // Rust's owned command-line strings require copying either way.

        // Otherwise, copy the token, removing quotes and backslashes. A
        // backslash escapes the next character, and a quoted part may be
        // followed by more characters of the same token.
        let mut tok = Vec::new();
        let mut quote = None;
        while i < data.len() {
            let c = data[i];
            if c == b'\\' {
                if i + 1 == data.len() {
                    fatal!(diag, "{path}: premature end of input");
                }
                tok.push(data[i + 1]);
                i += 2;
            } else if let Some(q) = quote {
                if c == q {
                    quote = None;
                } else {
                    tok.push(c);
                }
                i += 1;
            } else if c == b'\'' || c == b'"' {
                quote = Some(c);
                i += 1;
            } else if is_space(c) {
                break;
            } else {
                tok.push(c);
                i += 1;
            }
        }
        if quote.is_some() {
            fatal!(diag, "{path}: premature end of input");
        }
        tokens.push(String::from_utf8_lossy(&tok).into_owned());
    }

    let mut expanded = Vec::new();
    for tok in tokens {
        if let Some(nested) = tok.strip_prefix('@') {
            expanded.extend(read_response_file(diag, nested, depth + 1));
        } else {
            expanded.push(tok);
        }
    }
    expanded
}

// Replace "@path/to/some/text/file" with its file contents.
pub fn expand_response_files(diag: &Diagnostics, argv: &[String]) -> Vec<String> {
    let mut args = Vec::new();
    for arg in argv {
        if let Some(path) = arg.strip_prefix('@') {
            args.extend(read_response_file(diag, path, 1));
        } else {
            args.push(arg.clone());
        }
    }
    args
}

// This function matches a command line argument against an option
// name and, on success, returns the remainder of the argument. For
// example, matching "--foo=bar" against "foo" yields "=bar", and
// matching "--foo" against "foo" yields an empty string. On
// mismatch, it returns std::nullopt.
//
// Multi-letter option names can be preceded by either a single dash
// or double dashes except ones starting with "o", which must be
// preceded by double dashes. For example, "-omagic" is interpreted
// as "-o magic". If you really want to specify the "omagic" option,
// you have to pass "--omagic". Single-letter option names take a
// single dash.
fn match_option<'a>(arg: &'a str, name: &str) -> Option<&'a str> {
    let arg = arg.strip_prefix('-')?;

    if name.len() == 1 {
        return arg.strip_prefix(name);
    }
    // Options beginning with "o" require double dashes
    if name.starts_with('o') && !arg.starts_with('-') {
        return None;
    }
    let arg = arg.strip_prefix('-').unwrap_or(arg);
    arg.strip_prefix(name)
}

fn parse_hex(diag: &Diagnostics, opt: &str, value: &str) -> u64 {
    let digits = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        fatal!(diag, "option -{opt}: not a hexadecimal number");
    }
    u64::from_str_radix(digits, 16)
        .unwrap_or_else(|_| fatal!(diag, "option -{opt}: not a hexadecimal number"))
}

/// Parses an integer in C syntax (decimal, `0x` hex or leading-zero octal).
fn parse_c_number(s: &str) -> Option<u64> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else if s.len() > 1 && s.starts_with('0') {
        u64::from_str_radix(&s[1..], 8).ok()
    } else {
        s.parse().ok()
    }
}

fn parse_number(diag: &Diagnostics, opt: &str, value: &str) -> i64 {
    let (negative, digits) = match value.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, value),
    };
    let n = parse_c_number(digits)
        .unwrap_or_else(|| fatal!(diag, "option -{opt}: not a number: {value}")) as i64;
    if negative {
        -n
    } else {
        n
    }
}

fn from_hex(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        _ => c - b'A' + 10,
    }
}

fn parse_hex_build_id(diag: &Diagnostics, arg: &str) -> Vec<u8> {
    let digits = arg
        .strip_prefix("0x")
        .or_else(|| arg.strip_prefix("0X"))
        .filter(|d| !d.is_empty() && d.len() % 2 == 0 && d.bytes().all(|b| b.is_ascii_hexdigit()))
        .unwrap_or_else(|| fatal!(diag, "invalid build-id: {arg}"));
    digits
        .as_bytes()
        .chunks(2)
        .map(|pair| (from_hex(pair[0]) << 4) | from_hex(pair[1]))
        .collect()
}

fn parse_package_metadata(diag: &Diagnostics, arg: &str) -> String {
    let bytes = arg.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len()
                || !bytes[i + 1].is_ascii_hexdigit()
                || !bytes[i + 2].is_ascii_hexdigit()
            {
                fatal!(diag, "--package-metadata: invalid string: {arg}");
            }
            out.push((from_hex(bytes[i + 1]) << 4) | from_hex(bytes[i + 2]));
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn read_retain_symbols_file(diag: &Diagnostics, path: &str) -> Vec<String> {
    let mf = MappedFile::must_open(diag, path);
    String::from_utf8_lossy(mf.data())
        .lines()
        .map(|line| line.trim_matches(|c| c == ' ' || c == '\t'))
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_section_order(diag: &Diagnostics, arg: &str) -> Vec<SectionOrder> {
    let parse_value = |s: &str| -> Option<u64> {
        if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
            u64::from_str_radix(hex, 16).ok()
        } else {
            s.parse().ok()
        }
    };
    let is_section_name = |s: &str| {
        s.bytes()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'.')
            && !s.bytes().any(|c| c.is_ascii_whitespace())
    };

    let mut orders = Vec::new();
    for tok in arg.split([' ', '\t']).filter(|t| !t.is_empty()) {
        let mut order = SectionOrder {
            kind: SectionOrderKind::Section,
            name: String::new(),
            value: 0,
            token: tok.to_string(),
        };

        if matches!(
            tok.to_ascii_uppercase().as_str(),
            "TEXT" | "DATA" | "RODATA" | "BSS"
        ) {
            order.kind = SectionOrderKind::Group;
            order.name = tok.to_string();
        } else if let Some(v) = tok.strip_prefix('=').and_then(parse_value) {
            order.kind = SectionOrderKind::Addr;
            order.value = v;
        } else if let Some(v) = tok.strip_prefix('%').and_then(parse_value) {
            order.kind = SectionOrderKind::Align;
            order.value = v;
        } else if let Some(name) = tok.strip_prefix('!').filter(|n| !n.is_empty()) {
            order.kind = SectionOrderKind::Symbol;
            order.name = name.to_string();
        } else if is_section_name(tok) || tok == "EHDR" || tok == "PHDR" {
            order.kind = SectionOrderKind::Section;
            order.name = tok.to_string();
        } else {
            fatal!(diag, "--section-order: parse error: {arg}");
        }
        orders.push(order);
    }

    let mut is_first = true;
    for order in &orders {
        if order.kind == SectionOrderKind::Section {
            if is_first {
                is_first = false;
            } else if order.name == "EHDR" {
                fatal!(
                    diag,
                    "--section-order: EHDR must be the first section specifier: {arg}"
                );
            }
        }
    }
    orders
}

fn parse_defsym_value(diag: &Diagnostics, s: &str) -> DefsymValue {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        let Ok(v) = u64::from_str_radix(hex, 16) else {
            fatal!(diag, "-defsym: not a number: {s}");
        };
        return DefsymValue::Addr(v);
    }
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
        return DefsymValue::Addr(s.parse().unwrap_or(0));
    }
    DefsymValue::Symbol(s.to_string())
}

// Version 6.11 and 6.12 of the Linux kernel does not return ETXTBSY for
// open(2) on an executable file that is currently running. This function
// returns true if we are running on a Linux kernel older than 6.11 or newer
// than 6.12.
fn returns_etxtbsy() -> bool {
    let Ok(release) = std::fs::read_to_string("/proc/sys/kernel/osrelease") else {
        return false;
    };
    // Parses a kernel version string, e.g. "6.8.0-47-generic".
    let mut parts = release
        .trim()
        .split(['.', '-'])
        .map(|p| p.parse::<u32>().unwrap_or(0));
    let ver = (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    );
    !((6, 11, 0)..(6, 13, 0)).contains(&ver)
}

/// The result of parsing the command line.
pub struct ParsedArgs {
    pub args: Args,
    pub jobs: Vec<ReaderJob>,
    /// The reader state at the end of the command line; `-static` there
    /// means a static link.
    pub final_rctx: ReaderContext,
}

/// Parses all options. `cmdline` includes the program name.
pub fn parse_args(diag: &Diagnostics, target: &TargetTraits, cmdline: &[String]) -> ParsedArgs {
    // Input file arguments are turned into ReaderJobs for
    // read_input_files(). rctx tracks the reader state options, such as
    // --as-needed, that apply to the files after them; each job gets a
    // snapshot of the state at its position.
    let mut a = Args::default();
    let mut jobs: Vec<ReaderJob> = Vec::new();
    let mut rctx = ReaderContext::default();
    let mut rctx_stack: Vec<ReaderContext> = Vec::new();
    let mut visited_libs: HashSet<String> = HashSet::new();

    a.color_diagnostics = stderr_is_tty();
    diag.set_color(a.color_diagnostics);
    diag.set_fatal_warnings(a.fatal_warnings);
    diag.set_suppress_warnings(a.suppress_warnings);
    diag.set_noinhibit_exec(a.noinhibit_exec);
    crate::error::set_demangle(a.demangle);
    a.page_size = target.page_size;

    let mut version_shown = false;
    let mut warn_shared_textrel = false;
    let mut error_unresolved_symbols = true;
    let mut z_separate_code: Option<SeparateCodeKind> = None;
    let mut allow_shlib_undefined: Option<bool> = None;
    let mut report_undefined: Option<bool> = None;
    let mut z_relro: Option<bool> = None;
    let mut z_dynamic_undefined_weak: Option<bool> = None;
    let mut separate_debug_file: Option<String> = None;
    let mut shuffle_sections_seed: Option<u64> = None;
    let mut rpaths: HashSet<String> = HashSet::new();

    // We generally don't need to write addends to relocated places if the
    // relocation type is RELA because RELA records contain addends.
    // However, there are too much code that wrongly assumes that addends
    // are written to both RELA records and relocated places, so we write
    // addends to relocated places by default. There are a few exceptions:
    //
    // - It looks like the SPARC's dynamic linker takes both RELA's r_addend
    // and the value at the relocated place. So we don't want to write
    // values to relocated places.
    //
    // - Static PIE binaries crash on startup in some RISC-V environment if
    // we write addends to relocated places.
    a.apply_dynamic_relocs = !target.is_sparc && !target.is_riscv;

    let mut i = 1;
    let mut arg = String::new();

    // An option and its argument are either separate command line
    // arguments or a single one, as in "-o foo" vs. "-ofoo" or
    // "--output foo" vs. "--output=foo".
    macro_rules! read_arg {
        ($name:expr) => {{
            let name: &str = $name;
            match match_option(&cmdline[i], name) {
                None => false,
                Some(rest) if rest.is_empty() => {
                    if i + 1 == cmdline.len() {
                        fatal!(diag, "option -{name}: argument missing");
                    }
                    arg = cmdline[i + 1].clone();
                    i += 2;
                    true
                }
                Some(rest) if name.len() == 1 => {
                    arg = rest.to_string();
                    i += 1;
                    true
                }
                Some(rest) => match rest.strip_prefix('=') {
                    Some(value) => {
                        arg = value.to_string();
                        i += 1;
                        true
                    }
                    None => false,
                },
            }
        }};
    }

    macro_rules! read_eq {
        ($name:expr) => {{
            match match_option(&cmdline[i], $name).and_then(|rest| rest.strip_prefix('=')) {
                Some(value) => {
                    arg = value.to_string();
                    i += 1;
                    true
                }
                None => false,
            }
        }};
    }

    macro_rules! read_flag {
        ($name:expr) => {{
            if match_option(&cmdline[i], $name) == Some("") {
                i += 1;
                true
            } else {
                false
            }
        }};
    }

    macro_rules! read_z_flag {
        ($name:expr) => {{
            let name: &str = $name;
            if i + 1 < cmdline.len() && cmdline[i] == "-z" && cmdline[i + 1] == name {
                i += 2;
                true
            } else if cmdline[i].strip_prefix("-z") == Some(name) {
                i += 1;
                true
            } else {
                false
            }
        }};
    }

    macro_rules! read_z_arg {
        ($name:expr) => {{
            let name: &str = $name;
            let prefix = format!("{name}=");
            if i + 1 < cmdline.len() && cmdline[i] == "-z" && cmdline[i + 1].starts_with(&prefix) {
                arg = cmdline[i + 1][prefix.len()..].to_string();
                i += 2;
                true
            } else if let Some(value) = cmdline[i]
                .strip_prefix("-z")
                .and_then(|s| s.strip_prefix(prefix.as_str()))
            {
                arg = value.to_string();
                i += 1;
                true
            } else {
                false
            }
        }};
    }

    while i < cmdline.len() {
        if !cmdline[i].starts_with('-') {
            let mut job = ReaderJob {
                rctx: rctx.clone(),
                name: cmdline[i].clone(),
                ..Default::default()
            };
            job.rctx.pos = vec![jobs.len() as u32];
            jobs.push(job);
            i += 1;
            continue;
        }

        if read_flag!("help") {
            out!(diag, "Usage: {} [options] file...\n{}", cmdline[0], HELP);
            std::process::exit(0);
        }

        if read_arg!("o") || read_arg!("output") {
            a.output = arg.clone();
        } else if read_arg!("dynamic-linker") || read_arg!("I") {
            a.dynamic_linker = arg.clone();
        } else if read_flag!("no-dynamic-linker") {
            a.dynamic_linker.clear();
        } else if read_flag!("v") {
            out!(diag, "{VERSION}");
            version_shown = true;
        } else if read_flag!("version") {
            out!(diag, "{VERSION}");
            std::process::exit(0);
        } else if read_flag!("V") {
            out!(
                diag,
                "{VERSION}\n  Supported emulations:\n   elf_x86_64\n   elf_i386\n   aarch64elf\n   \
                 aarch64linux\n   aarch64elfb\n   aarch64linuxb\n   armelf_linux_eabi\n   elf64lriscv\n   \
                 elf64briscv\n   elf32lriscv\n   elf32briscv\n   elf32ppc\n   elf64ppc\n   elf64lppc\n   \
                 elf64_s390\n   elf64_sparc\n   m68kelf\n   shlelf_linux\n   shelf_linux\n   \
                 elf64loongarch\n   elf32loongarch"
            );
            version_shown = true;
        } else if read_arg!("mllvm") {
            a.plugin_opt.push(arg.clone());
        } else if read_arg!("m") {
            match arch::emulation_to_target(&arg) {
                Some(name) => a.emulation = name.to_string(),
                None => fatal!(diag, "unknown -m argument: {arg}"),
            }
        } else if read_flag!("end-lib") {
            rctx.in_lib = false;
        } else if read_flag!("export-dynamic") || read_flag!("E") {
            a.export_dynamic = true;
        } else if read_flag!("no-export-dynamic") {
            a.export_dynamic = false;
        } else if read_flag!("Bsymbolic") {
            a.bsymbolic = BsymbolicKind::All;
        } else if read_flag!("Bsymbolic-functions") {
            a.bsymbolic = BsymbolicKind::Functions;
        } else if read_flag!("Bsymbolic-non-weak") {
            a.bsymbolic = BsymbolicKind::NonWeak;
        } else if read_flag!("Bsymbolic-non-weak-functions") {
            a.bsymbolic = BsymbolicKind::NonWeakFunctions;
        } else if read_flag!("Bno-symbolic") {
            a.bsymbolic = BsymbolicKind::None;
        } else if read_arg!("exclude-libs") {
            for lib in arg.split([',', ':']) {
                a.exclude_libs.insert(lib.to_string());
            }
        } else if read_flag!("q") || read_flag!("emit-relocs") {
            a.emit_relocs = true;
            a.discard_locals = false;
        } else if read_arg!("e") || read_arg!("entry") {
            a.entry = arg.clone();
        } else if read_arg!("Map") {
            a.map = arg.clone();
            a.print_map = true;
        } else if read_flag!("print-dependencies") {
            a.print_dependencies = true;
        } else if read_flag!("print-map") || read_flag!("M") {
            a.print_map = true;
        } else if read_flag!("Bstatic") || read_flag!("dn") || read_flag!("static") {
            rctx.is_static = true;
        } else if read_flag!("Bdynamic") || read_flag!("dy") {
            rctx.is_static = false;
        } else if read_flag!("shared") || read_flag!("Bshareable") {
            a.shared = true;
        } else if read_arg!("spare-dynamic-tags") {
            a.spare_dynamic_tags = parse_number(diag, "spare-dynamic-tags", &arg);
        } else if read_arg!("spare-program-headers") {
            a.spare_program_headers = parse_number(diag, "spare-program-headers", &arg);
        } else if read_flag!("start-lib") {
            rctx.in_lib = true;
        } else if read_flag!("start-stop") {
            a.start_stop = true;
        } else if read_arg!("dependency-file") {
            a.dependency_file = arg.clone();
        } else if read_arg!("defsym") {
            let Some((name, value)) = arg.split_once('=').filter(|(_, v)| !v.is_empty()) else {
                fatal!(diag, "-defsym: syntax error: {arg}");
            };
            a.defsyms
                .push((name.to_string(), parse_defsym_value(diag, value)));
        } else if read_flag!(":lto-pass2") {
            a.lto_pass2 = true;
        } else if read_arg!(":ignore-ir-file") {
            a.ignore_ir_file.insert(arg.clone());
        } else if read_flag!("demangle") {
            a.demangle = true;
            crate::error::set_demangle(true);
        } else if read_flag!("no-demangle") {
            a.demangle = false;
            crate::error::set_demangle(false);
        } else if read_flag!("detach") {
            a.detach = true;
        } else if read_flag!("no-detach") {
            a.detach = false;
        } else if read_flag!("default-symver") {
            a.default_symver = true;
        } else if read_flag!("noinhibit-exec") {
            a.noinhibit_exec = true;
            diag.set_noinhibit_exec(true);
        } else if read_flag!("shuffle-sections") {
            a.shuffle_sections = ShuffleSectionsKind::Shuffle;
        } else if read_eq!("shuffle-sections") {
            a.shuffle_sections = ShuffleSectionsKind::Shuffle;
            shuffle_sections_seed = Some(parse_number(diag, "shuffle-sections", &arg) as u64);
        } else if read_flag!("reverse-sections") {
            a.shuffle_sections = ShuffleSectionsKind::Reverse;
        } else if read_flag!("rosegment") {
            a.rosegment = true;
        } else if read_flag!("no-rosegment") {
            a.rosegment = false;
        } else if read_arg!("y") || read_arg!("trace-symbol") {
            a.trace_symbol.push(arg.clone());
        } else if read_arg!("filler") {
            a.filler = Some(parse_hex(diag, "filler", &arg) as u8);
        } else if read_arg!("L") || read_arg!("library-path") {
            a.library_paths.push(arg.clone());
        } else if read_arg!("sysroot") {
            a.sysroot = arg.clone();
        } else if read_arg!("unique") {
            if !a.unique.add(arg.as_bytes(), 1) {
                fatal!(diag, "-unique: invalid glob pattern: {arg}");
            }
        } else if read_arg!("unresolved-symbols") {
            match arg.as_str() {
                "report-all" | "ignore-in-shared-libs" => report_undefined = Some(true),
                "ignore-all" | "ignore-in-object-files" => report_undefined = Some(false),
                _ => fatal!(diag, "unknown --unresolved-symbols argument: {arg}"),
            }
        } else if read_arg!("undefined") || read_arg!("u") {
            a.undefined.push(arg.clone());
        } else if read_arg!("undefined-glob") {
            if !a.undefined_glob.add(arg.as_bytes(), 0) {
                fatal!(diag, "--undefined-glob: invalid pattern: {arg}");
            }
        } else if read_arg!("require-defined") {
            a.require_defined.push(arg.clone());
        } else if read_arg!("init") {
            a.init = arg.clone();
        } else if read_arg!("fini") {
            a.fini = arg.clone();
        } else if read_arg!("hash-style") {
            match arg.as_str() {
                "sysv" => {
                    a.hash_style_sysv = true;
                    a.hash_style_gnu = false;
                }
                "gnu" => {
                    a.hash_style_sysv = false;
                    a.hash_style_gnu = true;
                }
                "both" => {
                    a.hash_style_sysv = true;
                    a.hash_style_gnu = true;
                }
                "none" => {
                    a.hash_style_sysv = false;
                    a.hash_style_gnu = false;
                }
                _ => fatal!(diag, "invalid --hash-style argument: {arg}"),
            }
        } else if read_arg!("soname") || read_arg!("h") {
            a.soname = arg.clone();
        } else if read_arg!("audit") {
            if !a.audit.is_empty() {
                a.audit.push(':');
            }
            a.audit.push_str(&arg);
        } else if read_arg!("depaudit") || read_arg!("P") {
            if !a.depaudit.is_empty() {
                a.depaudit.push(':');
            }
            a.depaudit.push_str(&arg);
        } else if read_flag!("allow-multiple-definition") {
            a.allow_multiple_definition = true;
        } else if read_flag!("apply-dynamic-relocs") {
            a.apply_dynamic_relocs = true;
        } else if read_flag!("no-apply-dynamic-relocs") {
            a.apply_dynamic_relocs = false;
        } else if read_flag!("trace") {
            a.trace = true;
        } else if read_flag!("eh-frame-hdr") {
            a.eh_frame_hdr = true;
        } else if read_flag!("no-eh-frame-hdr") {
            a.eh_frame_hdr = false;
        } else if read_flag!("pie") || read_flag!("pic-executable") {
            a.pic = true;
            a.pie = true;
        } else if read_flag!("no-pie") || read_flag!("no-pic-executable") || read_flag!("nopie") {
            a.pic = false;
            a.pie = false;
        } else if read_flag!("relax") {
            a.relax = true;
        } else if read_flag!("no-relax") {
            a.relax = false;
        } else if read_flag!("gdb-index") {
            a.gdb_index = true;
        } else if read_flag!("no-gdb-index") {
            a.gdb_index = false;
        } else if read_flag!("r") || read_flag!("relocatable") {
            a.relocatable = true;
            a.emit_relocs = true;
            a.discard_locals = false;
        } else if read_flag!("relocatable-merge-sections") {
            a.relocatable_merge_sections = true;
        } else if read_flag!("perf") {
            a.perf = true;
        } else if read_flag!("pack-dyn-relocs=relr") || read_z_flag!("pack-relative-relocs") {
            a.pack_dyn_relocs_relr = true;
            a.pack_dyn_relocs_android = false;
        } else if read_flag!("pack-dyn-relocs=android") {
            a.pack_dyn_relocs_android = true;
            a.pack_dyn_relocs_relr = false;
        } else if read_flag!("pack-dyn-relocs=android+relr") {
            a.pack_dyn_relocs_android = true;
            a.pack_dyn_relocs_relr = true;
        } else if read_flag!("pack-dyn-relocs=none") || read_z_flag!("nopack-relative-relocs") {
            a.pack_dyn_relocs_relr = false;
            a.pack_dyn_relocs_android = false;
        } else if read_flag!("use-android-relr-tags") {
            a.use_android_relr_tags = true;
        } else if read_flag!("no-use-android-relr-tags") {
            a.use_android_relr_tags = false;
        } else if read_arg!("package-metadata") {
            a.package_metadata = parse_package_metadata(diag, &arg);
        } else if read_flag!("stats") {
            a.stats = true;
            Counter::enable();
        } else if read_arg!("C") || read_arg!("directory") {
            a.directory = arg.clone();
        } else if read_arg!("chroot") {
            a.chroot = arg.clone();
        } else if read_flag!("color-diagnostics") || read_flag!("color-diagnostics=auto") {
            a.color_diagnostics = stderr_is_tty();
            diag.set_color(a.color_diagnostics);
        } else if read_flag!("color-diagnostics=always") {
            a.color_diagnostics = true;
            diag.set_color(true);
        } else if read_flag!("color-diagnostics=never") {
            a.color_diagnostics = false;
            diag.set_color(false);
        } else if read_flag!("warn-common") {
            a.warn_common = true;
        } else if read_flag!("no-warn-common") {
            a.warn_common = false;
        } else if read_flag!("warn-once") {
            a.warn_once = true;
        } else if read_flag!("warn-shared-textrel") {
            warn_shared_textrel = true;
        } else if read_flag!("warn-textrel") {
            a.warn_textrel = true;
        } else if read_flag!("enable-new-dtags") {
            a.enable_new_dtags = true;
        } else if read_flag!("disable-new-dtags") {
            a.enable_new_dtags = false;
        } else if read_flag!("execute-only") {
            a.execute_only = true;
        } else if read_flag!("zero-to-bss") {
            a.zero_to_bss = true;
        } else if read_arg!("compress-debug-sections") {
            match arg.as_str() {
                "zlib" | "zlib-gabi" => {
                    a.compress_debug_sections = ELFCOMPRESS_ZLIB;
                    a.compress_debug_sections_level = 1;
                }
                "zstd" => {
                    a.compress_debug_sections = ELFCOMPRESS_ZSTD;
                    a.compress_debug_sections_level = 3;
                }
                "none" => a.compress_debug_sections = ELFCOMPRESS_NONE,
                s if s.starts_with("zlib:") => {
                    a.compress_debug_sections = ELFCOMPRESS_ZLIB;
                    let level = parse_number(diag, "compress-debug-sections", &s[5..]);
                    if !(0..=9).contains(&level) {
                        fatal!(
                            diag,
                            "invalid --compress-debug-sections argument: {arg} (zlib level must be between 0 and 9)"
                        );
                    }
                    a.compress_debug_sections_level = level;
                }
                s if s.starts_with("zstd:") => {
                    a.compress_debug_sections = ELFCOMPRESS_ZSTD;
                    let level = parse_number(diag, "compress-debug-sections", &s[5..]);
                    if !(1..=22).contains(&level) {
                        fatal!(
                            diag,
                            "invalid --compress-debug-sections argument: {arg} (zstd level must be between 1 and 22)"
                        );
                    }
                    a.compress_debug_sections_level = level;
                }
                _ => fatal!(diag, "invalid --compress-debug-sections argument: {arg}"),
            }
        } else if read_arg!("wrap") {
            a.wrap.insert(arg.clone());
        } else if read_flag!("omagic") || read_flag!("N") {
            a.omagic = true;
            rctx.is_static = true;
        } else if read_flag!("no-omagic") {
            a.omagic = false;
        } else if read_arg!("oformat") {
            if arg != "binary" {
                fatal!(diag, "-oformat: {arg} is not supported");
            }
            a.oformat_binary = true;
        } else if read_arg!("retain-symbols-file") {
            a.retain_symbols_file = Some(read_retain_symbols_file(diag, &arg));
        } else if read_arg!("section-align") {
            let Some((name, value)) = arg.split_once('=').filter(|(_, v)| !v.is_empty()) else {
                fatal!(diag, "--section-align: syntax error: {arg}");
            };
            let value = parse_number(diag, "section-align", value);
            if value <= 0 || !(value as u64).is_power_of_two() {
                fatal!(diag, "--section-align={arg}: value must be a power of 2");
            }
            a.section_align.insert(name.to_string(), value as u64);
        } else if read_arg!("section-start") {
            let Some((name, value)) = arg.split_once('=').filter(|(_, v)| !v.is_empty()) else {
                fatal!(diag, "--section-start: syntax error: {arg}");
            };
            a.section_start
                .insert(name.to_string(), parse_hex(diag, "section-start", value));
        } else if read_arg!("section-order") {
            a.section_order = parse_section_order(diag, &arg);
        } else if read_arg!("Tbss") {
            a.section_start
                .insert(".bss".to_string(), parse_hex(diag, "Tbss", &arg));
        } else if read_arg!("Tdata") {
            a.section_start
                .insert(".data".to_string(), parse_hex(diag, "Tdata", &arg));
        } else if read_arg!("Ttext") {
            a.section_start
                .insert(".text".to_string(), parse_hex(diag, "Ttext", &arg));
        } else if read_arg!("Ttext-segment") {
            a.ttext_segment = Some(parse_number(diag, "Ttext-segment", &arg) as u64);
        } else if read_flag!("repro") {
            a.repro = true;
        } else if read_z_flag!("now") {
            a.z_now = true;
        } else if read_z_flag!("lazy") {
            a.z_now = false;
        } else if read_z_flag!("cet-report=none") {
            a.z_cet_report = CetReportKind::None;
        } else if read_z_flag!("cet-report=warning") {
            a.z_cet_report = CetReportKind::Warning;
        } else if read_z_flag!("cet-report=error") {
            a.z_cet_report = CetReportKind::Error;
        } else if read_z_flag!("execstack") {
            a.z_execstack = true;
        } else if read_z_flag!("execstack-if-needed") {
            a.z_execstack_if_needed = true;
        } else if read_z_arg!("max-page-size") {
            a.page_size = parse_number(diag, "-z max-page-size", &arg) as u64;
            if !a.page_size.is_power_of_two() {
                fatal!(diag, "-z max-page-size {arg}: value must be a power of 2");
            }
        } else if read_z_flag!("start-stop-visibility=protected") {
            a.z_start_stop_visibility_protected = true;
        } else if read_z_flag!("start-stop-visibility=hidden") {
            a.z_start_stop_visibility_protected = false;
        } else if read_z_flag!("noexecstack") {
            a.z_execstack = false;
        } else if read_z_flag!("relro") {
            z_relro = Some(true);
        } else if read_z_flag!("norelro") {
            z_relro = Some(false);
        } else if read_z_flag!("defs") || read_flag!("no-undefined") {
            report_undefined = Some(true);
        } else if read_z_flag!("undefs") {
            report_undefined = Some(false);
        } else if read_z_flag!("nodlopen") {
            a.z_dlopen = false;
        } else if read_z_flag!("nodelete") {
            a.z_delete = false;
        } else if read_z_flag!("nocopyreloc") {
            a.z_copyreloc = false;
        } else if read_z_flag!("nodump") {
            a.z_dump = false;
        } else if read_z_flag!("initfirst") {
            a.z_initfirst = true;
        } else if read_z_flag!("interpose") {
            a.z_interpose = true;
        } else if read_z_flag!("ibt") {
            a.z_ibt = true;
        } else if read_z_flag!("ibtplt") {
        } else if read_z_flag!("muldefs") {
            a.allow_multiple_definition = true;
        } else if read_z_flag!("keep-text-section-prefix") {
            a.z_keep_text_section_prefix = true;
        } else if read_z_flag!("nokeep-text-section-prefix") {
            a.z_keep_text_section_prefix = false;
        } else if read_z_flag!("shstk") {
            a.z_shstk = true;
        } else if read_z_flag!("text") {
            a.z_text = true;
        } else if read_z_flag!("notext") || read_z_flag!("textoff") {
            a.z_text = false;
        } else if read_z_flag!("origin") {
            a.z_origin = true;
        } else if read_z_flag!("nodefaultlib") {
            a.z_nodefaultlib = true;
        } else if read_eq!("separate-debug-file") {
            separate_debug_file = Some(arg.clone());
        } else if read_flag!("separate-debug-file") {
            separate_debug_file = Some(String::new());
        } else if read_flag!("no-separate-debug-file") {
            separate_debug_file = None;
        } else if read_z_flag!("separate-loadable-segments") {
            z_separate_code = Some(SeparateCodeKind::SeparateLoadableSegments);
        } else if read_z_flag!("separate-code") {
            z_separate_code = Some(SeparateCodeKind::SeparateCode);
        } else if read_z_flag!("noseparate-code") {
            z_separate_code = Some(SeparateCodeKind::NoSeparateCode);
        } else if read_z_arg!("stack-size") {
            a.z_stack_size = parse_number(diag, "-z stack-size", &arg) as u64;
        } else if read_z_flag!("dynamic-undefined-weak") {
            z_dynamic_undefined_weak = Some(true);
        } else if read_z_flag!("nodynamic-undefined-weak") {
            z_dynamic_undefined_weak = Some(false);
        } else if read_z_flag!("sectionheader") {
            a.z_sectionheader = true;
        } else if read_z_flag!("nosectionheader") {
            a.z_sectionheader = false;
        } else if read_z_flag!("rodynamic") {
            a.z_rodynamic = true;
        } else if read_z_flag!("x86-64-v2") {
            a.z_x86_64_isa_level |= GNU_PROPERTY_X86_ISA_1_V2;
        } else if read_z_flag!("x86-64-v3") {
            a.z_x86_64_isa_level |= GNU_PROPERTY_X86_ISA_1_V3;
        } else if read_z_flag!("x86-64-v4") {
            a.z_x86_64_isa_level |= GNU_PROPERTY_X86_ISA_1_V4;
        } else if read_z_flag!("rewrite-endbr") {
            if !target.is_x86_64 && !target.is_arm64 {
                fatal!(
                    diag,
                    "-z rewrite-endbr is supported only on x86-64 and arm64"
                );
            }
            a.z_rewrite_endbr = true;
        } else if read_z_flag!("norewrite-endbr") {
            a.z_rewrite_endbr = false;
        } else if read_flag!("nmagic") {
            a.nmagic = true;
        } else if read_flag!("no-nmagic") {
            a.nmagic = false;
        } else if read_flag!("fatal-warnings") {
            a.fatal_warnings = true;
            diag.set_fatal_warnings(true);
        } else if read_flag!("no-fatal-warnings") {
            a.fatal_warnings = false;
            diag.set_fatal_warnings(false);
        } else if read_flag!("w") || read_flag!("no-warnings") {
            a.suppress_warnings = true;
            diag.set_suppress_warnings(true);
        } else if read_flag!("fork") {
            a.fork = true;
        } else if read_flag!("no-fork") {
            a.fork = false;
        } else if read_flag!("gc-sections") {
            a.gc_sections = true;
        } else if read_flag!("no-gc-sections") {
            a.gc_sections = false;
        } else if read_flag!("print-gc-sections") {
            a.print_gc_sections = "-".to_string();
        } else if read_eq!("print-gc-sections") {
            a.print_gc_sections = arg.clone();
        } else if read_flag!("no-print-gc-sections") {
            a.print_gc_sections.clear();
        } else if read_arg!("discard-section") {
            a.discard_section.insert(arg.clone());
        } else if read_arg!("no-discard-section") {
            a.discard_section.remove(&arg);
        } else if read_arg!("icf") {
            match arg.as_str() {
                "all" => {
                    a.icf = true;
                    a.icf_all = true;
                }
                "safe" => a.icf = true,
                "none" => a.icf = false,
                _ => fatal!(diag, "unknown --icf argument: {arg}"),
            }
        } else if read_flag!("no-icf") {
            a.icf = false;
        } else if read_flag!("ignore-data-address-equality") {
            a.ignore_data_address_equality = true;
        } else if read_arg!("image-base") {
            a.image_base = parse_number(diag, "image-base", &arg) as u64;
        } else if read_arg!("physical-image-base") {
            a.physical_image_base = Some(parse_number(diag, "physical-image-base", &arg) as u64);
        } else if read_flag!("print-icf-sections") {
            a.print_icf_sections = "-".to_string();
        } else if read_eq!("print-icf-sections") {
            a.print_icf_sections = arg.clone();
        } else if read_flag!("no-print-icf-sections") {
            a.print_icf_sections.clear();
        } else if read_flag!("quick-exit") {
            a.quick_exit = true;
        } else if read_flag!("no-quick-exit") {
            a.quick_exit = false;
        } else if read_arg!("plugin") {
            a.plugin = arg.clone();
        } else if read_arg!("plugin-opt") {
            a.plugin_opt.push(arg.clone());
        } else if read_flag!("lto-cs-profile-generate") {
            a.plugin_opt.push("cs-profile-generate".to_string());
        } else if read_arg!("lto-cs-profile-file") {
            a.plugin_opt.push(format!("cs-profile-path={arg}"));
        } else if read_flag!("lto-debug-pass-manager") {
            a.plugin_opt.push("debug-pass-manager".to_string());
        } else if read_flag!("disable-verify") {
            a.plugin_opt.push("disable-verify".to_string());
        } else if read_flag!("lto-emit-asm") {
            a.plugin_opt.push("emit-asm".to_string());
        } else if read_flag!("no-legacy-pass-manager") {
            a.plugin_opt.push("legacy-pass-manager".to_string());
        } else if read_arg!("lto-partitions") {
            a.plugin_opt.push(format!("lto-partitions={arg}"));
        } else if read_flag!("no-lto-legacy-pass-manager") {
            a.plugin_opt.push("new-pass-manager".to_string());
        } else if read_arg!("lto-obj-path") {
            a.plugin_opt.push(format!("obj-path={arg}"));
        } else if read_arg!("opt-remarks-filename") {
            a.plugin_opt.push(format!("opt-remarks-filename={arg}"));
        } else if read_arg!("opt-remarks-format") {
            a.plugin_opt.push(format!("opt-remarks-format={arg}"));
        } else if read_arg!("opt-remarks-hotness-threshold") {
            a.plugin_opt
                .push(format!("opt-remarks-hotness-threshold={arg}"));
        } else if read_arg!("opt-remarks-passes") {
            a.plugin_opt.push(format!("opt-remarks-passes={arg}"));
        } else if read_flag!("opt-remarks-with-hotness") {
            a.plugin_opt.push("opt-remarks-with-hotness".to_string());
        } else if let Some(level) = cmdline[i].strip_prefix("-lto-O") {
            a.plugin_opt.push(format!("O{level}"));
            i += 1;
        } else if let Some(level) = cmdline[i].strip_prefix("--lto-O") {
            a.plugin_opt.push(format!("O{level}"));
            i += 1;
        } else if read_arg!("lto-pseudo-probe-for-profiling") {
            a.plugin_opt
                .push(format!("pseudo-probe-for-profiling={arg}"));
        } else if read_arg!("lto-sample-profile") {
            a.plugin_opt.push(format!("sample-profile={arg}"));
        } else if read_flag!("save-temps") {
            a.plugin_opt.push("save-temps".to_string());
        } else if read_flag!("thinlto-emit-imports-files") {
            a.plugin_opt.push("thinlto-emit-imports-files".to_string());
        } else if read_arg!("thinlto-index-only") {
            a.plugin_opt.push(format!("thinlto-index-only={arg}"));
        } else if read_flag!("thinlto-index-only") {
            a.plugin_opt.push("thinlto-index-only".to_string());
        } else if read_arg!("thinlto-object-suffix-replace") {
            a.plugin_opt
                .push(format!("thinlto-object-suffix-replace={arg}"));
        } else if read_arg!("thinlto-prefix-replace") {
            a.plugin_opt.push(format!("thinlto-prefix-replace={arg}"));
        } else if read_arg!("thinlto-cache-dir") {
            a.plugin_opt.push(format!("cache-dir={arg}"));
        } else if read_arg!("thinlto-cache-policy") {
            a.plugin_opt.push(format!("cache-policy={arg}"));
        } else if read_arg!("thinlto-jobs") {
            a.plugin_opt.push(format!("jobs={arg}"));
        } else if read_arg!("thread-count") {
            a.thread_count = Some(parse_number(diag, "thread-count", &arg).max(1) as usize);
        } else if read_flag!("threads") {
            a.thread_count = None;
        } else if read_flag!("no-threads") {
            a.thread_count = Some(1);
        } else if read_eq!("threads") {
            a.thread_count = Some(parse_number(diag, "threads", &arg).max(1) as usize);
        } else if read_flag!("discard-all") || read_flag!("x") {
            a.discard_all = true;
        } else if read_flag!("discard-locals") || read_flag!("X") {
            a.discard_locals = true;
        } else if read_flag!("discard-none") {
            a.discard_all = false;
            a.discard_locals = false;
        } else if read_flag!("strip-all") || read_flag!("s") {
            a.strip_all = true;
        } else if read_flag!("strip-debug") || read_flag!("S") {
            a.strip_debug = true;
        } else if read_flag!("warn-unresolved-symbols") {
            error_unresolved_symbols = false;
        } else if read_flag!("error-unresolved-symbols") {
            error_unresolved_symbols = true;
        } else if read_arg!("rpath") {
            add_rpath(&mut a, &mut rpaths, &arg);
        } else if read_arg!("R") {
            if crate::mapped_file::is_file(&arg) {
                fatal!(
                    diag,
                    "-R{arg}: -R as an alias for --just-symbols is not supported"
                );
            }
            add_rpath(&mut a, &mut rpaths, &arg);
        } else if read_flag!("undefined-version") {
            a.undefined_version = true;
        } else if read_flag!("no-undefined-version") {
            a.undefined_version = false;
        } else if read_flag!("build-id") {
            a.build_id.kind = BuildIdKind::Hash;
            a.build_id.hash_size = 20;
        } else if read_arg!("build-id") {
            match arg.as_str() {
                "none" => a.build_id.kind = BuildIdKind::None,
                "uuid" => a.build_id.kind = BuildIdKind::Uuid,
                "md5" => {
                    a.build_id.kind = BuildIdKind::Hash;
                    a.build_id.hash_size = 16;
                }
                "sha1" => {
                    a.build_id.kind = BuildIdKind::Hash;
                    a.build_id.hash_size = 20;
                }
                "sha256" | "fast" => {
                    a.build_id.kind = BuildIdKind::Hash;
                    a.build_id.hash_size = 32;
                }
                s if s.starts_with("0x") || s.starts_with("0X") => {
                    a.build_id.kind = BuildIdKind::Hex;
                    a.build_id.value = parse_hex_build_id(diag, s);
                }
                _ => fatal!(diag, "invalid --build-id argument: {arg}"),
            }
        } else if read_flag!("no-build-id") {
            a.build_id.kind = BuildIdKind::None;
        } else if read_flag!("be8") {
            a.be8 = true;
        } else if read_flag!("be32") {
            a.be8 = false;
        } else if read_arg!("format") || read_arg!("b") {
            if arg == "binary" {
                fatal!(
                    diag,
                    "mold does not support `-b binary`. If you want to convert a binary file into an \
                     object file, use `objcopy -I binary -O default <input-file> <output-file.o>` instead."
                );
            }
            fatal!(diag, "unknown command line option: -b {arg}");
        } else if read_arg!("fuse-ld") {
        } else if read_arg!("auxiliary") || read_arg!("f") {
            a.auxiliary.push(arg.clone());
        } else if read_arg!("filter") || read_arg!("F") {
            a.filter.push(arg.clone());
        } else if read_flag!("allow-shlib-undefined") {
            allow_shlib_undefined = Some(true);
        } else if read_flag!("no-allow-shlib-undefined") {
            allow_shlib_undefined = Some(false);
        } else if read_arg!("O")
            || read_flag!("EB")
            || read_flag!("EL")
            || read_flag!("O0")
            || read_flag!("O1")
            || read_flag!("O2")
            || read_flag!("verbose")
            || read_flag!("color-diagnostics")
            || read_flag!("eh-frame-hdr")
            || read_flag!("start-group")
            || read_flag!("end-group")
            || read_flag!("(")
            || read_flag!(")")
            || read_flag!("fatal-warnings")
            || read_flag!("enable-new-dtags")
            || read_flag!("disable-new-dtags")
            || read_flag!("nostdlib")
            || read_flag!("no-add-needed")
            || read_flag!("no-call-graph-profile-sort")
            || read_flag!("no-copy-dt-needed-entries")
            || read_arg!("sort-section")
            || read_flag!("sort-common")
            || read_flag!("dc")
            || read_flag!("dp")
            || read_flag!("fix-cortex-a53-835769")
            || read_flag!("fix-cortex-a53-843419")
            || read_flag!("warn-once")
            || read_flag!("nodefaultlibs")
            || read_flag!("warn-constructors")
            || read_flag!("warn-execstack")
            || read_flag!("no-warn-execstack")
            || read_flag!("long-plt")
            || read_flag!("secure-plt")
            || read_arg!("rpath-link")
            || read_z_flag!("combreloc")
            || read_z_flag!("nocombreloc")
            || read_z_arg!("common-page-size")
            || read_flag!("no-keep-memory")
            || read_arg!("max-cache-size")
            || read_flag!("mmap-output-file")
            || read_flag!("no-mmap-output-file")
        {
            // Ignored for compatibility.
        } else if read_arg!("version-script") {
            a.version_scripts.push(arg.clone());
        } else if read_arg!("dynamic-list") {
            a.bsymbolic = BsymbolicKind::All;
            a.dynamic_list.push(DynamicListSource::File(arg.clone()));
        } else if read_arg!("dynamic-list-data") {
            a.dynamic_list_data = true;
        } else if read_arg!("export-dynamic-symbol") {
            a.dynamic_list.push(DynamicListSource::Pattern(arg.clone()));
        } else if read_arg!("export-dynamic-symbol-list") {
            a.dynamic_list.push(DynamicListSource::File(arg.clone()));
        } else if read_flag!("as-needed") {
            rctx.as_needed = true;
        } else if read_flag!("no-as-needed") {
            rctx.as_needed = false;
        } else if read_flag!("whole-archive") {
            rctx.whole_archive = true;
        } else if read_flag!("no-whole-archive") {
            rctx.whole_archive = false;
        } else if read_arg!("l") || read_arg!("library") {
            if visited_libs.insert(arg.clone()) {
                let mut job = ReaderJob {
                    rctx: rctx.clone(),
                    name: arg.clone(),
                    is_lib: true,
                    ..Default::default()
                };
                job.rctx.pos = vec![jobs.len() as u32];
                jobs.push(job);
            }
        } else if read_arg!("script") || read_arg!("T") {
            let mut job = ReaderJob {
                rctx: rctx.clone(),
                name: arg.clone(),
                ..Default::default()
            };
            job.rctx.pos = vec![jobs.len() as u32];
            jobs.push(job);
        } else if read_flag!("push-state") {
            rctx_stack.push(rctx.clone());
        } else if read_flag!("pop-state") {
            rctx = rctx_stack
                .pop()
                .unwrap_or_else(|| fatal!(diag, "no state pushed before popping"));
        } else if cmdline[i].starts_with("-z") && cmdline[i].len() > 2 {
            warn!(diag, "unknown command line option: {}", cmdline[i]);
            i += 1;
        } else if cmdline[i] == "-z" && i + 1 < cmdline.len() {
            warn!(diag, "unknown command line option: -z {}", cmdline[i + 1]);
            i += 2;
        } else if cmdline[i] == "-dynamic" {
            fatal!(
                diag,
                "unknown command line option: -dynamic; -dynamic is a macOS linker's option. mold does not support macOS."
            );
        } else {
            fatal!(diag, "unknown command line option: {}", cmdline[i]);
        }
    }

    if !a.chroot.is_empty() {
        if !a.map.is_empty() {
            a.map = format!("{}/{}", a.chroot, a.map);
        }
        if !a.dependency_file.is_empty() {
            a.dependency_file = format!("{}/{}", a.chroot, a.dependency_file);
        }
    }

    if !a.directory.is_empty() {
        if let Err(e) = std::env::set_current_dir(&a.directory) {
            fatal!(diag, "chdir failed: {}: {e}", a.directory);
        }
    }

    if !a.sysroot.is_empty() {
        for path in &mut a.library_paths {
            if let Some(rest) = path.strip_prefix('=') {
                *path = format!("{}{rest}", a.sysroot);
            } else if let Some(rest) = path.strip_prefix("$SYSROOT") {
                *path = format!("{}{rest}", a.sysroot);
            }
        }
    }

    // Clean library paths by removing redundant `/..` and `/.` so that
    // they are easier to read in log messages.
    for path in &mut a.library_paths {
        *path = path_clean(path);
    }

    if a.shared {
        a.pic = true;
    }

    if let Some(val) = a.ttext_segment {
        if val % a.page_size != 0 {
            warn!(diag, "-Ttext-segment is not a multiple of page size: {val}");
        }
        a.image_base = align_down(val, a.page_size);
    } else if a.pic {
        a.image_base = 0;
    }

    // A shared library is loaded by the dynamic linker, so it doesn't need
    // PT_INTERP. (-pie makes the output an executable even with -shared.)
    if a.shared && !a.pie {
        a.dynamic_linker.clear();
    }

    a.allow_shlib_undefined = allow_shlib_undefined.unwrap_or(a.shared);

    let report_undefined = report_undefined.unwrap_or(!a.shared);
    a.unresolved_symbols = if report_undefined {
        if error_unresolved_symbols {
            UnresolvedKind::Error
        } else {
            UnresolvedKind::Warn
        }
    } else {
        UnresolvedKind::Ignore
    };

    if a.retain_symbols_file.is_some() {
        a.strip_all = false;
        a.discard_all = false;
    }

    if a.shuffle_sections == ShuffleSectionsKind::Shuffle {
        a.shuffle_sections_seed = shuffle_sections_seed.unwrap_or_else(|| {
            let mut buf = [0u8; 8];
            util::random_bytes(&mut buf);
            u64::from_ne_bytes(buf)
        });
    }

    // --section-order implies `-z separate-loadable-segments`
    a.z_separate_code = z_separate_code.unwrap_or(if a.section_order.is_empty() {
        SeparateCodeKind::NoSeparateCode
    } else {
        SeparateCodeKind::SeparateLoadableSegments
    });

    // `-z dynamic-undefined-weak` is enabled by default for DSOs.
    a.z_dynamic_undefined_weak = z_dynamic_undefined_weak.unwrap_or(a.shared);

    // --section-order implies `-z norelro`
    a.z_relro = z_relro.unwrap_or(a.section_order.is_empty());
    if a.nmagic || a.omagic {
        a.z_relro = false;
    }

    if !a.shared {
        if !a.filter.is_empty() {
            fatal!(diag, "-filter may not be used without -shared");
        }
        if !a.auxiliary.is_empty() {
            fatal!(diag, "-auxiliary may not be used without -shared");
        }
    }

    // Even though SH4 is RELA, addends in its relocation records are always
    // zero, and actual addends are written to relocated places. So we need
    // to handle it as an exception.
    if (!target.is_rela || target.is_sh4) && !a.apply_dynamic_relocs {
        fatal!(
            diag,
            "--no-apply-dynamic-relocs may not be used on {}",
            target.name
        );
    }
    if target.is_sparc && a.apply_dynamic_relocs {
        fatal!(diag, "--apply-dynamic-relocs may not be used on SPARC64");
    }

    if !a.section_start.is_empty() && !a.section_order.is_empty() {
        fatal!(diag, "--section-start may not be used with --section-order");
    }
    if a.image_base % a.page_size != 0 {
        fatal!(diag, "-image-base must be a multiple of -max-page-size");
    }
    if a.emulation == "arm32be" && !a.be8 {
        fatal!(diag, "--be32 is not supported");
    }

    if std::env::var("MOLD_REPRO").is_ok_and(|v| !v.is_empty()) {
        a.repro = true;
    }

    if a.default_symver {
        let ver = if a.soname.is_empty() {
            path_filename(&a.output)
        } else {
            a.soname.clone()
        };
        a.version_definitions.push(ver);
    }

    if let Some(file) = separate_debug_file {
        a.separate_debug_file = if file.is_empty() {
            format!("{}.dbg", a.output)
        } else {
            file
        };
    }

    if a.shared && warn_shared_textrel {
        a.warn_textrel = true;
    }

    // We don't want the background process to write to stdout.
    if a.stats || a.perf {
        a.detach = false;
    }

    // Mark GC root symbols
    a.undefined.push(a.entry.clone());
    for (_, value) in &a.defsyms {
        if let DefsymValue::Symbol(sym) = value {
            a.undefined.push(sym.clone());
        }
    }

    // --oformat=binary implies --strip-all because without a section
    // header, there's no way to identify the locations of a symbol
    // table in an output file in the first place.
    if a.oformat_binary {
        a.strip_all = true;
    }

    // By default, mold tries to ovewrite to an output file if exists
    // because at least on Linux, writing to an existing file is much
    // faster than creating a fresh file and writing to it.
    //
    // However, if an existing file is in use, writing to it will mess
    // up processes that are executing that file. Linux prevents a write
    // to a running executable file; it returns ETXTBSY on open(2).
    // However, that mechanism doesn't protect .so files. Therefore, we
    // want to disable this optimization if we are creating a shared
    // object file.
    a.overwrite_output_file = !a.shared && returns_etxtbsy();

    if version_shown && jobs.is_empty() {
        std::process::exit(0);
    }

    if rctx.is_static || a.relocatable {
        a.is_static = true;
        a.dynamic_linker.clear();
    }

    ParsedArgs {
        args: a,
        jobs,
        final_rctx: rctx,
    }
}

fn add_rpath(a: &mut Args, seen: &mut HashSet<String>, path: &str) {
    if seen.insert(path.to_string()) {
        if !a.rpaths.is_empty() {
            a.rpaths.push(':');
        }
        a.rpaths.push_str(path);
    }
}

fn stderr_is_tty() -> bool {
    std::io::stderr().is_terminal()
}
