//! Command-line argument parsing.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use bstr::ByteSlice;
use mold_common::bits::align_down;
use mold_common::bytes::{display, is_space, os_str};
use mold_common::error::strerror;
use mold_common::glob::{Glob, GlobBuilder};
use mold_common::mapped_file::MappedFile;
use mold_common::path::clean_path;
use mold_common::perf::Counter;
use mold_common::{fatal, out, warn};

use crate::arch::{Family, emulation_to_target};
use crate::elf::*;

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
  --color-diagnostics         Alias for --color-diagnostics=auto
    --no-color-diagnostics    Alias for --color-diagnostics=never
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
  --mmap-output-file          Write the output file using mmap (default)
    --no-mmap-output-file
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
    -z undefs
  -z common-page-size=VALUE   Ignored
  -z execstack                Require an executable stack
    -z noexecstack
  -z execstack-if-needed      Make the stack area executable if an input file explicitly requests it
  -z global                   Set DF_1_GLOBAL flag
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
  -z start-stop-visibility=[hidden,protected]
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

pub static VERSION: LazyLock<String> = LazyLock::new(|| {
    let version = env!("CARGO_PKG_VERSION");
    match mold_common::GIT_HASH {
        Some(hash) => format!("mold {version} ({hash}; compatible with GNU ld)"),
        None => format!("mold {version} (compatible with GNU ld)"),
    }
});

#[derive(Clone, Debug, Default)]
pub enum BuildId {
    #[default]
    None,
    Hex(Vec<u8>),
    Hash(usize),
    Uuid,
}

impl BuildId {
    pub fn size(&self) -> usize {
        match self {
            Self::None => 0,
            Self::Hex(value) => value.len(),
            Self::Hash(size) => *size,
            Self::Uuid => 16,
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
pub enum ReportKind {
    #[default]
    None,
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GcsKind {
    #[default]
    Implicit,
    Never,
    Always,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ShuffleSections {
    #[default]
    None,
    Shuffle(u64),
    Reverse,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DebugCompression {
    #[default]
    None,
    Zlib(i32),
    Zstd(i32),
}

#[derive(Debug)]
pub enum ReportOutput {
    Stdout,
    File(PathBuf),
}

impl ReportOutput {
    pub fn with_writer(
        &self,
        option: &str,
        write: impl FnOnce(&mut dyn Write) -> std::io::Result<()>,
    ) {
        match self {
            Self::Stdout => {
                let mut out = std::io::BufWriter::new(std::io::stdout().lock());
                let _ = write(&mut out).and_then(|()| out.flush());
            }
            Self::File(path) => {
                let file = std::fs::File::create(path).unwrap_or_else(|e| {
                    fatal!("{option}: cannot open {}: {}", path.display(), strerror(&e))
                });
                let mut out = std::io::BufWriter::new(file);
                write(&mut out).and_then(|()| out.flush()).unwrap_or_else(|e| {
                    fatal!("{option}: writing {} failed: {}", path.display(), strerror(&e))
                });
            }
        }
    }
}

fn parse_report_output(path: &OsStr) -> Option<ReportOutput> {
    if path.is_empty() {
        None
    } else if path == "-" {
        Some(ReportOutput::Stdout)
    } else {
        Some(ReportOutput::File(path.into()))
    }
}

#[derive(Clone, Debug)]
pub enum SectionOrder {
    Section(Vec<u8>),
    Group(String),
    // Keep the original token for error reporting.
    Addr { value: u64, token: String },
    Align(u64),
    Symbol(Vec<u8>),
}

/// The right-hand side of `--defsym=SYMBOL=VALUE`.
#[derive(Clone, Debug)]
pub enum DefsymValue {
    Addr(u64),
    Symbol(Vec<u8>),
}

/// A source of dynamic-list patterns, kept in command line order.
#[derive(Clone, Debug)]
pub enum DynamicListSource {
    /// A dynamic list file. It is opened during option parsing, so a
    /// relative path is resolved before -C changes the directory.
    File(&'static MappedFile),
    Pattern(Vec<u8>),
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
    pub fn next_child(&mut self) -> Self {
        let mut child = self.clone();
        child.pos.push(self.num_children);
        child.num_children = 0;
        self.num_children += 1;
        child
    }
}

// A file to read along with the reader state at its command line
// position. parse_args() creates one ReaderJob per
// input file argument; `name` is a path or, if `is_lib` is set, a
// library name to search for.
#[derive(Clone, Debug, Default)]
pub struct ReaderJob {
    pub rctx: ReaderContext,
    pub name: PathBuf,
    pub is_lib: bool,
}

/// All command line options.
#[derive(Debug)]
pub struct Args {
    pub bsymbolic: BsymbolicKind,
    pub build_id: BuildId,
    pub z_cet_report: ReportKind,
    pub z_bti_report: ReportKind,
    pub z_gcs_report: ReportKind,
    pub z_gcs_report_dynamic: ReportKind,
    pub z_gcs: GcsKind,
    pub undefined_glob: Glob,
    pub unique: Glob,
    pub z_separate_code: SeparateCodeKind,
    pub shuffle_sections: ShuffleSections,
    pub entry: Vec<u8>,
    pub fini: Vec<u8>,
    pub init: Vec<u8>,
    pub unresolved_symbols: UnresolvedKind,
    pub allow_multiple_definition: bool,
    pub allow_shlib_undefined: bool,
    pub apply_dynamic_relocs: bool,
    pub default_symver: bool,
    pub detach: bool,
    pub discard_all: bool,
    pub discard_locals: bool,
    pub dynamic_list_data: bool,
    pub eh_frame_hdr: bool,
    pub emit_relocs: bool,
    pub enable_new_dtags: bool,
    pub execute_only: bool,
    pub export_dynamic: bool,
    pub fork: bool,
    pub gc_sections: bool,
    pub gdb_index: bool,
    pub hash_style_gnu: bool,
    pub hash_style_sysv: bool,
    pub icf: bool,
    pub icf_all: bool,
    pub ignore_data_address_equality: bool,
    pub lto_pass2: bool,
    pub mmap_output_file: bool,
    pub nmagic: bool,
    pub oformat_binary: bool,
    pub omagic: bool,
    pub pack_dyn_relocs_android: bool,
    pub pack_dyn_relocs_relr: bool,
    pub perf: bool,
    pub pic: bool,
    pub pie: bool,
    pub print_dependencies: bool,
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
    pub trace: bool,
    pub undefined_version: bool,
    pub use_android_relr_tags: bool,
    pub warn_common: bool,
    pub warn_textrel: bool,
    pub z_copyreloc: bool,
    pub z_delete: bool,
    pub z_dlopen: bool,
    pub z_dump: bool,
    pub z_dynamic_undefined_weak: bool,
    pub z_execstack: bool,
    pub z_execstack_if_needed: bool,
    pub z_force_bti: bool,
    pub z_global: bool,
    pub z_ibt: bool,
    pub z_initfirst: bool,
    pub z_interpose: bool,
    pub z_keep_text_section_prefix: bool,
    pub z_nodefaultlib: bool,
    pub z_now: bool,
    pub z_origin: bool,
    pub z_pac_plt: bool,
    pub z_relro: bool,
    pub z_rewrite_endbr: bool,
    pub z_rodynamic: bool,
    pub z_sectionheader: bool,
    pub z_shstk: bool,
    pub z_start_stop_visibility_protected: bool,
    pub z_text: bool,
    pub zero_to_bss: bool,
    pub compress_debug_sections: DebugCompression,
    pub filler: Option<u8>,
    pub spare_dynamic_tags: i64,
    pub spare_program_headers: i64,
    pub z_stack_size: u64,
    pub thread_count: Option<usize>,
    pub retain_symbols_file: Option<Vec<&'static [u8]>>,
    pub physical_image_base: Option<u64>,
    pub ttext_segment: Option<u64>,
    pub map: Option<ReportOutput>,
    pub audit: Vec<u8>,
    pub chroot: PathBuf,
    pub depaudit: Vec<u8>,
    pub dependency_file: PathBuf,
    pub dynamic_linker: PathBuf,
    pub output: PathBuf,
    /// The working directory before -C changed it.
    pub orig_cwd: Option<PathBuf>,
    pub package_metadata: Vec<u8>,
    pub plugin: PathBuf,
    pub print_gc_sections: Option<ReportOutput>,
    pub print_icf_sections: Option<ReportOutput>,
    pub rpaths: OsString,
    pub separate_debug_file: PathBuf,
    pub soname: OsString,
    pub sysroot: PathBuf,
    pub emulation: &'static str,
    pub section_align: HashMap<Vec<u8>, u64>,
    pub section_start: HashMap<Vec<u8>, u64>,
    pub discard_section: HashSet<Vec<u8>>,
    pub exclude_libs: HashSet<Vec<u8>>,
    pub ignore_ir_file: HashSet<OsString>,
    pub wrap: HashSet<Vec<u8>>,
    pub section_order: Vec<SectionOrder>,
    pub require_defined: Vec<Vec<u8>>,
    pub undefined: Vec<Vec<u8>>,
    pub defsyms: Vec<(Vec<u8>, DefsymValue)>,
    pub library_paths: Vec<PathBuf>,
    pub plugin_opt: Vec<Vec<u8>>,
    pub version_definitions: Vec<Cow<'static, [u8]>>,
    pub version_scripts: Vec<PathBuf>,
    pub dynamic_list: Vec<DynamicListSource>,
    pub auxiliary: Vec<Vec<u8>>,
    pub filter: Vec<Vec<u8>>,
    pub trace_symbol: Vec<Vec<u8>>,
    pub z_x86_64_isa_level: u32,
    pub image_base: u64,
    pub page_size: u64,

    /// Whether an existing output file may be overwritten in place.
    pub overwrite_output_file: bool,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            bsymbolic: BsymbolicKind::None,
            build_id: BuildId::default(),
            z_cet_report: ReportKind::None,
            z_bti_report: ReportKind::None,
            z_gcs_report: ReportKind::None,
            z_gcs_report_dynamic: ReportKind::None,
            z_gcs: GcsKind::Implicit,
            undefined_glob: Glob::new(),
            unique: Glob::new(),
            z_separate_code: SeparateCodeKind::NoSeparateCode,
            shuffle_sections: ShuffleSections::None,
            entry: b"_start".to_vec(),
            fini: b"_fini".to_vec(),
            init: b"_init".to_vec(),
            unresolved_symbols: UnresolvedKind::Ignore,
            allow_multiple_definition: false,
            allow_shlib_undefined: true,
            apply_dynamic_relocs: true,
            default_symver: false,
            detach: true,
            discard_all: false,
            discard_locals: true,
            dynamic_list_data: false,
            eh_frame_hdr: true,
            emit_relocs: false,
            enable_new_dtags: true,
            execute_only: false,
            export_dynamic: false,
            fork: true,
            gc_sections: false,
            gdb_index: false,
            hash_style_gnu: true,
            hash_style_sysv: true,
            icf: false,
            icf_all: false,
            ignore_data_address_equality: false,
            lto_pass2: false,
            mmap_output_file: true,
            nmagic: false,
            oformat_binary: false,
            omagic: false,
            pack_dyn_relocs_android: false,
            pack_dyn_relocs_relr: false,
            perf: false,
            pic: false,
            pie: false,
            print_dependencies: false,
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
            trace: false,
            undefined_version: false,
            use_android_relr_tags: false,
            warn_common: false,
            warn_textrel: false,
            z_copyreloc: true,
            z_delete: true,
            z_dlopen: true,
            z_dump: true,
            z_dynamic_undefined_weak: true,
            z_execstack: false,
            z_execstack_if_needed: false,
            z_force_bti: false,
            z_global: false,
            z_ibt: false,
            z_initfirst: false,
            z_interpose: false,
            z_keep_text_section_prefix: false,
            z_nodefaultlib: false,
            z_now: false,
            z_origin: false,
            z_pac_plt: false,
            z_relro: true,
            z_rewrite_endbr: false,
            z_rodynamic: false,
            z_sectionheader: true,
            z_shstk: false,
            z_start_stop_visibility_protected: false,
            z_text: false,
            zero_to_bss: false,
            compress_debug_sections: DebugCompression::None,
            filler: None,
            spare_dynamic_tags: 5,
            spare_program_headers: 0,
            z_stack_size: 0,
            thread_count: None,
            retain_symbols_file: None,
            physical_image_base: None,
            ttext_segment: None,
            map: None,
            audit: Vec::new(),
            chroot: PathBuf::new(),
            depaudit: Vec::new(),
            dependency_file: PathBuf::new(),
            dynamic_linker: PathBuf::new(),
            output: PathBuf::from("a.out"),
            orig_cwd: None,
            package_metadata: Vec::new(),
            plugin: PathBuf::new(),
            print_gc_sections: None,
            print_icf_sections: None,
            rpaths: OsString::new(),
            separate_debug_file: PathBuf::new(),
            soname: OsString::new(),
            sysroot: PathBuf::new(),
            emulation: "",
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
    pub family: Family,
    pub page_size: u64,
}

// This function matches a command line argument against an option
// name and, on success, returns the remainder of the argument. For
// example, matching "--foo=bar" against "foo" yields "=bar", and
// matching "--foo" against "foo" yields an empty string. On
// mismatch, it returns None.
//
// Single-letter option names take a single dash. Multi-letter names
// accept one or two dashes unless `name` explicitly starts with "--".
// For example, "export-dynamic" accepts "-export-dynamic" as well as
// "--export-dynamic", while "--execute-only" requires two dashes.
// Which options accept a single dash is somewhat arbitrary for
// historical reasons; we follow other linkers for compatibility.
//
// The parser must try long options that accept a single dash before
// single-letter options that could consume the rest as an argument.
// Thus "-export-dynamic" means "--export-dynamic", while "-execute-only"
// is interpreted as "-e xecute-only".
#[cfg_attr(feature = "winnow-args", allow(dead_code))]
fn match_option<'a>(arg: &'a OsStr, name: &str) -> Option<&'a OsStr> {
    let arg = arg.as_encoded_bytes();
    if let Some(name) = name.strip_prefix("--") {
        return arg.strip_prefix(b"--")?.strip_prefix(name.as_bytes()).map(os_str);
    }
    let arg = arg.strip_prefix(b"-")?;
    if name.len() == 1 {
        return arg.strip_prefix(name.as_bytes()).map(os_str);
    }
    arg.strip_prefix(b"-").unwrap_or(arg).strip_prefix(name.as_bytes()).map(os_str)
}

fn parse_hex(opt: &str, value: &str) -> u64 {
    let digits = value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")).unwrap_or(value);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        fatal!("option -{opt}: not a hexadecimal number");
    }
    u64::from_str_radix(digits, 16)
        .unwrap_or_else(|_| fatal!("option -{opt}: not a hexadecimal number"))
}

/// Parses an integer in C syntax (decimal, `0x` hex or leading-zero octal).
fn parse_c_number(s: &str) -> Option<u64> {
    let s = s.trim_start_matches(|c: char| c.is_ascii() && is_space(c as u8));
    let (negative, digits) = if let Some(rest) = s.strip_prefix('-') {
        (true, rest)
    } else {
        (false, s.strip_prefix('+').unwrap_or(s))
    };
    let (digits, radix) =
        if let Some(hex) = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")) {
            (hex, 16)
        } else if digits.starts_with('0') {
            (digits, 8)
        } else {
            (digits, 10)
        };
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    let n = u64::from_str_radix(digits, radix).ok()?;
    Some(if negative { n.wrapping_neg() } else { n })
}

fn parse_number(opt: &str, value: &str) -> i64 {
    let (negative, digits) = match value.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, value),
    };
    let n = parse_c_number(digits).unwrap_or_else(|| fatal!("option -{opt}: not a number: {value}"))
        as i64;
    if negative { n.wrapping_neg() } else { n }
}

fn from_hex(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        _ => c - b'A' + 10,
    }
}

fn parse_hex_build_id(arg: &str) -> Vec<u8> {
    let digits = arg
        .strip_prefix("0x")
        .or_else(|| arg.strip_prefix("0X"))
        .filter(|d| !d.is_empty() && d.len() % 2 == 0 && d.bytes().all(|b| b.is_ascii_hexdigit()))
        .unwrap_or_else(|| fatal!("invalid build-id: {arg}"));
    digits.as_bytes().chunks(2).map(|pair| (from_hex(pair[0]) << 4) | from_hex(pair[1])).collect()
}

// The argument is arbitrary bytes with `%XX` escapes.
fn parse_package_metadata(arg: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < arg.len() {
        if arg[i] == b'%' {
            if i + 2 >= arg.len()
                || !arg[i + 1].is_ascii_hexdigit()
                || !arg[i + 2].is_ascii_hexdigit()
            {
                fatal!("--package-metadata: invalid string: {}", display(arg));
            }
            out.push((from_hex(arg[i + 1]) << 4) | from_hex(arg[i + 2]));
            i += 3;
        } else {
            out.push(arg[i]);
            i += 1;
        }
    }
    out
}

fn read_retain_symbols_file(chroot: &Path, path: &Path) -> Vec<&'static [u8]> {
    let mf = mold_common::mapped_file::must_open_file(chroot, path);
    mf.data()
        .split(|&b| b == b'\n')
        .map(|line| line.trim_with(|c| c == ' ' || c == '\t'))
        .filter(|line| !line.is_empty())
        .collect()
}

fn parse_section_order(arg: &[u8]) -> Vec<SectionOrder> {
    let parse_value = |s: &[u8]| -> Option<u64> {
        let s = std::str::from_utf8(s).ok()?;
        if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
            u64::from_str_radix(hex, 16).ok()
        } else {
            s.parse().ok()
        }
    };
    let is_section_name = |s: &[u8]| {
        s.first().copied().is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'.')
            && !s.iter().any(|c| c.is_ascii_whitespace())
    };

    let mut orders = Vec::new();
    for tok in arg.split(|&c| c == b' ' || c == b'\t').filter(|t| !t.is_empty()) {
        let order = if matches!(
            tok.to_ascii_uppercase().as_slice(),
            b"TEXT" | b"DATA" | b"RODATA" | b"BSS"
        ) {
            SectionOrder::Group(std::str::from_utf8(tok).unwrap().to_string())
        } else if let Some(value) = tok.strip_prefix(b"=").and_then(parse_value) {
            SectionOrder::Addr { value, token: std::str::from_utf8(tok).unwrap().to_string() }
        } else if let Some(v) = tok.strip_prefix(b"%").and_then(parse_value) {
            SectionOrder::Align(v)
        } else if let Some(name) = tok.strip_prefix(b"!").filter(|n| !n.is_empty()) {
            SectionOrder::Symbol(name.to_vec())
        } else if is_section_name(tok) || tok == b"EHDR" || tok == b"PHDR" {
            SectionOrder::Section(tok.to_vec())
        } else {
            fatal!("--section-order: parse error: {}", arg.as_bstr());
        };
        orders.push(order);
    }

    let mut is_first = true;
    for order in &orders {
        if let SectionOrder::Section(name) = order {
            if is_first {
                is_first = false;
            } else if name == b"EHDR" {
                fatal!(
                    "--section-order: EHDR must be the first section specifier: {}",
                    arg.as_bstr()
                );
            }
        }
    }
    orders
}

fn parse_defsym_value(s: &[u8]) -> DefsymValue {
    if let Some(hex) = s.strip_prefix(b"0x").or_else(|| s.strip_prefix(b"0X")) {
        let Some(v) =
            std::str::from_utf8(hex).ok().and_then(|hex| u64::from_str_radix(hex, 16).ok())
        else {
            fatal!("-defsym: not a number: {}", display(s));
        };
        return DefsymValue::Addr(v);
    }
    if !s.is_empty() && s.iter().all(u8::is_ascii_digit) {
        let s = std::str::from_utf8(s).unwrap();
        let value = s.parse().unwrap_or_else(|_| fatal!("-defsym: not a number: {s}"));
        return DefsymValue::Addr(value);
    }
    DefsymValue::Symbol(s.to_vec())
}

// Versions 6.11 and 6.12 of the Linux kernel do not return ETXTBSY for
// open(2) on an executable file that is currently running. This function
// returns true if we are running on a Linux kernel older than 6.11 or newer
// than 6.12.
#[cfg(unix)]
fn returns_etxtbsy() -> bool {
    // uname may leave the tail of each string buffer untouched.
    let mut buf = std::mem::MaybeUninit::<libc::utsname>::zeroed();
    // SAFETY: buf points to writable storage for a complete utsname.
    if unsafe { libc::uname(buf.as_mut_ptr()) } != 0 {
        return false;
    }
    // SAFETY: uname succeeded and initialized the structure, including
    // NUL-terminated sysname and release strings.
    let buf = unsafe { buf.assume_init() };
    let sysname = unsafe { std::ffi::CStr::from_ptr(buf.sysname.as_ptr()) };
    if sysname.to_bytes() != b"Linux" {
        return false;
    }
    let release = unsafe { std::ffi::CStr::from_ptr(buf.release.as_ptr()) }.to_bytes();

    // Match the C++ parser's leading major.minor.patch, ignoring any suffix.
    // An unrecognized version is treated as 0.0.0.
    let ver = (|| {
        let mut parts = release.splitn(3, |&c| c == b'.');
        let mut ver = [0u32; 3];
        for (i, num) in ver.iter_mut().enumerate() {
            let part = parts.next()?;
            let len = part.iter().take_while(|c| c.is_ascii_digit()).count();
            if len == 0 || (i < 2 && len != part.len()) {
                return None;
            }
            *num = std::str::from_utf8(&part[..len]).ok()?.parse().ok()?;
        }
        Some((ver[0], ver[1], ver[2]))
    })()
    .unwrap_or((0, 0, 0));
    !((6, 11, 0)..(6, 13, 0)).contains(&ver)
}

#[cfg(not(unix))]
fn returns_etxtbsy() -> bool {
    false
}

/// The GNU option grammar, with values borrowed from the original OS strings.
#[cfg_attr(feature = "winnow-args", allow(dead_code))]
struct ArgCursor<'a> {
    args: &'a [Cow<'a, OsStr>],
    index: usize,
}

#[cfg_attr(feature = "winnow-args", allow(dead_code))]
impl<'a> ArgCursor<'a> {
    fn current(&self) -> &'a OsStr {
        &self.args[self.index]
    }

    fn text(&self) -> &'a str {
        self.current().to_str().unwrap_or("")
    }

    fn read_arg(&mut self, name: &str) -> Option<&'a OsStr> {
        let rest = match_option(self.current(), name)?;
        let (value, count) = if rest.is_empty() {
            let value = self.args.get(self.index + 1).unwrap_or_else(|| {
                fatal!("option {}: argument missing", self.current().to_string_lossy())
            });
            (value.as_ref(), 2)
        } else if name.len() == 1 {
            (rest, 1)
        } else {
            (os_str(rest.as_encoded_bytes().strip_prefix(b"=")?), 1)
        };
        self.index += count;
        Some(value)
    }

    fn read_eq(&mut self, name: &str) -> Option<&'a OsStr> {
        let rest = match_option(self.current(), name)?;
        let value = os_str(rest.as_encoded_bytes().strip_prefix(b"=")?);
        self.index += 1;
        Some(value)
    }

    fn read_flag(&mut self, name: &str) -> bool {
        if match_option(self.current(), name) != Some(OsStr::new("")) {
            return false;
        }
        self.index += 1;
        true
    }

    fn read_lto_option(&mut self) -> Option<Vec<u8>> {
        // Flags precede argument forms, so that a bare --thinlto-index-only
        // does not take the next argument as its value.
        for (name, value) in [
            ("--lto-cs-profile-generate", "cs-profile-generate"),
            ("--lto-debug-pass-manager", "debug-pass-manager"),
            ("disable-verify", "disable-verify"),
            ("--lto-emit-asm", "emit-asm"),
            ("no-legacy-pass-manager", "legacy-pass-manager"),
            ("no-lto-legacy-pass-manager", "new-pass-manager"),
            ("--opt-remarks-with-hotness", "opt-remarks-with-hotness"),
            ("lto-pseudo-probe-for-profiling", "pseudo-probe-for-profiling"),
            ("save-temps", "save-temps"),
            ("thinlto-emit-imports-files", "thinlto-emit-imports-files"),
            ("thinlto-index-only", "thinlto-index-only"),
        ] {
            if self.read_flag(name) {
                return Some(value.as_bytes().to_vec());
            }
        }
        for (name, prefix) in [
            ("--lto-cs-profile-file", "cs-profile-path="),
            ("--lto-partitions", "lto-partitions="),
            ("--lto-obj-path", "obj-path="),
            ("--opt-remarks-filename", "opt-remarks-filename="),
            ("--opt-remarks-format", "opt-remarks-format="),
            ("--opt-remarks-hotness-threshold", "opt-remarks-hotness-threshold="),
            ("--opt-remarks-passes", "opt-remarks-passes="),
            ("--lto-sample-profile", "sample-profile="),
            ("thinlto-index-only", "thinlto-index-only="),
            ("thinlto-object-suffix-replace", "thinlto-object-suffix-replace="),
            ("thinlto-prefix-replace", "thinlto-prefix-replace="),
            ("thinlto-cache-dir", "cache-dir="),
            ("thinlto-cache-policy", "cache-policy="),
            ("thinlto-jobs", "jobs="),
        ] {
            if let Some(value) = self.read_arg(name) {
                return Some([prefix.as_bytes(), value.as_encoded_bytes()].concat());
            }
        }
        let level = self.current().as_encoded_bytes().strip_prefix(b"--lto-O")?;
        self.index += 1;
        Some([b"O", level].concat())
    }

    fn read_switch(&mut self, positive: &str, negative: &str) -> Option<bool> {
        if self.read_flag(positive) {
            Some(true)
        } else if self.read_flag(negative) {
            Some(false)
        } else {
            None
        }
    }

    fn read_z_switch(&mut self, positive: &str, negative: &str) -> Option<bool> {
        if self.read_z_flag(positive) {
            Some(true)
        } else if self.read_z_flag(negative) {
            Some(false)
        } else {
            None
        }
    }

    fn z_value(&self) -> (Option<&'a str>, usize) {
        if self.text() == "-z" {
            (self.args.get(self.index + 1).and_then(|s| s.to_str()), 2)
        } else {
            (self.text().strip_prefix("-z"), 1)
        }
    }

    fn read_z_flag(&mut self, name: &str) -> bool {
        let (value, count) = self.z_value();
        if value != Some(name) {
            return false;
        }
        self.index += count;
        true
    }

    fn read_z_arg(&mut self, name: &str) -> Option<&'a str> {
        let (value, count) = self.z_value();
        let value = value?.strip_prefix(name)?.strip_prefix('=')?;
        self.index += count;
        Some(value)
    }
}

/// The result of parsing the command line.
pub struct ParsedArgs {
    pub args: Args,
    pub jobs: Vec<ReaderJob>,
}

/// Parses all options. `cmdline` includes the program name.
pub fn parse_args(target: &TargetTraits, raw_cmdline: &[Cow<'_, OsStr>]) -> ParsedArgs {
    // Input file arguments are turned into ReaderJobs for
    // read_input_files(). rctx tracks the reader state options, such as
    // --as-needed, that apply to the files after them; each job gets a
    // snapshot of the state at its position.
    let mut a = Args::default();
    let mut be8 = false;
    let mut directory = PathBuf::new();
    let mut undefined_glob = GlobBuilder::default();
    let mut unique = GlobBuilder::default();
    let mut jobs: Vec<ReaderJob> = Vec::new();
    let mut rctx = ReaderContext::default();
    let mut rctx_stack: Vec<ReaderContext> = Vec::new();
    let mut visited_libs: HashSet<&OsStr> = HashSet::new();

    mold_common::error::set_color(std::io::stderr().is_terminal());
    mold_common::error::set_fatal_warnings(false);
    mold_common::error::set_suppress_warnings(false);
    mold_common::error::set_noinhibit_exec(false);
    mold_common::error::set_demangle(true);
    a.page_size = target.page_size;

    let mut version_shown = false;
    let mut warn_shared_textrel = false;
    let mut error_unresolved_symbols = true;
    let mut z_separate_code: Option<SeparateCodeKind> = None;
    let mut allow_shlib_undefined: Option<bool> = None;
    let mut report_undefined: Option<bool> = None;
    let mut z_relro: Option<bool> = None;
    let mut z_dynamic_undefined_weak: Option<bool> = None;
    #[cfg_attr(feature = "winnow-args", allow(unused_mut))]
    let mut z_bti_report: Option<ReportKind> = None;
    #[cfg_attr(feature = "winnow-args", allow(unused_mut))]
    let mut z_gcs_report: Option<ReportKind> = None;
    #[cfg_attr(feature = "winnow-args", allow(unused_mut))]
    let mut z_gcs_report_dynamic: Option<ReportKind> = None;
    let mut separate_debug_file: Option<PathBuf> = None;
    // An explicit seed survives intervening --reverse-sections options.
    let mut shuffle_sections_seed: Option<u64> = None;
    let mut map_path: Option<PathBuf> = None;
    let mut rpaths: HashSet<&OsStr> = HashSet::new();

    // We generally don't need to write addends to relocated places if the
    // relocation type is RELA because RELA records contain addends.
    // However, there is too much code that wrongly assumes that addends
    // are written to both RELA records and relocated places, so we write
    // addends to relocated places by default. There are a few exceptions:
    //
    // - It looks like SPARC's dynamic linker takes both RELA's r_addend
    // and the value at the relocated place. So we don't want to write
    // values to relocated places.
    //
    // - Static PIE binaries crash on startup in some RISC-V environments if
    // we write addends to relocated places.
    a.apply_dynamic_relocs = !matches!(target.family, Family::Sparc64 | Family::RiscV);

    // The built-in parser: each option tried in turn against the current word.
    #[cfg(not(feature = "winnow-args"))]
    let mut cursor = ArgCursor { args: raw_cmdline, index: 1 };
    #[cfg(not(feature = "winnow-args"))]
    let mut arg = "";
    #[cfg(not(feature = "winnow-args"))]
    let mut raw_arg = OsStr::new("");

    #[cfg(not(feature = "winnow-args"))]
    macro_rules! read_value {
        ($method:ident, $name:expr) => {
            read_value!($method, $name, false)
        };
        ($method:ident, $name:expr, $raw:expr) => {{
            let opt = cursor.current();
            if let Some(value) = cursor.$method($name) {
                raw_arg = value;
                if !$raw {
                    arg = value.to_str().unwrap_or_else(|| {
                        fatal!("option {}: expected a UTF-8 argument", opt.to_string_lossy())
                    });
                }
                true
            } else {
                false
            }
        }};
    }
    #[cfg(not(feature = "winnow-args"))]
    macro_rules! read_arg {
        ($($args:tt)*) => { read_value!(read_arg, $($args)*) };
    }
    #[cfg(not(feature = "winnow-args"))]
    macro_rules! read_eq {
        ($($args:tt)*) => { read_value!(read_eq, $($args)*) };
    }
    #[cfg(not(feature = "winnow-args"))]
    macro_rules! read_z_arg {
        ($name:expr) => {{
            if let Some(value) = cursor.read_z_arg($name) {
                arg = value;
                true
            } else {
                false
            }
        }};
    }

    #[cfg(not(feature = "winnow-args"))]
    while cursor.index < raw_cmdline.len() {
        if !cursor.current().as_encoded_bytes().starts_with(b"-") {
            let mut job = ReaderJob {
                rctx: rctx.clone(),
                name: PathBuf::from(&cursor.current()),
                ..Default::default()
            };
            job.rctx.pos = vec![jobs.len() as u32];
            jobs.push(job);
            cursor.index += 1;
            continue;
        }

        if cursor.read_flag("help") {
            out!("Usage: {} [options] file...\n{}", raw_cmdline[0].to_string_lossy(), HELP);
            std::process::exit(0);
        }

        if read_arg!("o", true) || read_arg!("--output", true) {
            a.output = PathBuf::from(raw_arg);
        } else if read_arg!("dynamic-linker", true) || read_arg!("I", true) {
            a.dynamic_linker = PathBuf::from(raw_arg);
        } else if cursor.read_flag("no-dynamic-linker") {
            a.dynamic_linker.clear();
        } else if cursor.read_flag("v") {
            out!("{}", *VERSION);
            version_shown = true;
        } else if cursor.read_flag("version") {
            out!("{}", *VERSION);
            std::process::exit(0);
        } else if cursor.read_flag("V") {
            out!(
                "{}\n  Supported emulations:\n   elf_x86_64\n   elf_i386\n   aarch64elf\n   \
                 aarch64linux\n   aarch64elfb\n   aarch64linuxb\n   armelf_linux_eabi\n   elf64lriscv\n   \
                 elf64briscv\n   elf32lriscv\n   elf32briscv\n   elf32ppc\n   elf64ppc\n   elf64lppc\n   \
                 elf64_s390\n   elf64_sparc\n   m68kelf\n   shlelf_linux\n   shelf_linux\n   \
                 elf64loongarch\n   elf32loongarch",
                *VERSION
            );
            version_shown = true;
        } else if read_arg!("mllvm", true) {
            a.plugin_opt.push(raw_arg.as_encoded_bytes().to_vec());
        } else if cursor.read_flag("end-lib") {
            rctx.in_lib = false;
        } else if cursor.read_flag("export-dynamic") || cursor.read_flag("E") {
            a.export_dynamic = true;
        } else if cursor.read_flag("no-export-dynamic") {
            a.export_dynamic = false;
        } else if cursor.read_flag("Bsymbolic") {
            a.bsymbolic = BsymbolicKind::All;
        } else if cursor.read_flag("Bsymbolic-functions") {
            a.bsymbolic = BsymbolicKind::Functions;
        } else if cursor.read_flag("Bsymbolic-non-weak") {
            a.bsymbolic = BsymbolicKind::NonWeak;
        } else if cursor.read_flag("Bsymbolic-non-weak-functions") {
            a.bsymbolic = BsymbolicKind::NonWeakFunctions;
        } else if cursor.read_flag("Bno-symbolic") {
            a.bsymbolic = BsymbolicKind::None;
        } else if read_arg!("exclude-libs", true) {
            for lib in raw_arg.as_encoded_bytes().split(|b| matches!(b, b',' | b':')) {
                a.exclude_libs.insert(lib.to_vec());
            }
        } else if cursor.read_flag("q") || cursor.read_flag("emit-relocs") {
            a.emit_relocs = true;
            a.discard_locals = false;
        } else if read_arg!("Map", true) {
            map_path = Some(PathBuf::from(raw_arg));
        } else if cursor.read_flag("print-dependencies") {
            a.print_dependencies = true;
        } else if cursor.read_flag("print-map") || cursor.read_flag("M") {
            map_path.get_or_insert_with(PathBuf::new);
        } else if cursor.read_flag("Bstatic")
            || cursor.read_flag("dn")
            || cursor.read_flag("static")
        {
            rctx.is_static = true;
        } else if cursor.read_flag("Bdynamic") || cursor.read_flag("dy") {
            rctx.is_static = false;
        } else if cursor.read_flag("shared") || cursor.read_flag("Bshareable") {
            a.shared = true;
        } else if read_arg!("spare-dynamic-tags") {
            a.spare_dynamic_tags = parse_number("spare-dynamic-tags", arg);
        } else if read_arg!("spare-program-headers") {
            a.spare_program_headers = parse_number("spare-program-headers", arg);
        } else if cursor.read_flag("start-lib") {
            rctx.in_lib = true;
        } else if cursor.read_flag("start-stop") {
            a.start_stop = true;
        } else if read_arg!("dependency-file", true) {
            a.dependency_file = PathBuf::from(raw_arg);
        } else if read_arg!("defsym", true) {
            let Some((name, value)) =
                raw_arg.as_encoded_bytes().split_once_str(b"=").filter(|(_, v)| !v.is_empty())
            else {
                fatal!("-defsym: syntax error: {}", raw_arg.to_string_lossy());
            };
            a.defsyms.push((name.to_vec(), parse_defsym_value(value)));
        } else if cursor.read_flag(":lto-pass2") {
            a.lto_pass2 = true;
        } else if read_arg!(":ignore-ir-file", true) {
            a.ignore_ir_file.insert(raw_arg.to_os_string());
        } else if cursor.read_flag("demangle") {
            mold_common::error::set_demangle(true);
        } else if cursor.read_flag("no-demangle") {
            mold_common::error::set_demangle(false);
        } else if let Some(value) = cursor.read_switch("detach", "no-detach") {
            a.detach = value;
        } else if cursor.read_flag("default-symver") {
            a.default_symver = true;
        } else if cursor.read_flag("noinhibit-exec") {
            mold_common::error::set_noinhibit_exec(true);
        } else if cursor.read_flag("shuffle-sections") {
            // Resolve the seed after parsing all options.
            a.shuffle_sections = ShuffleSections::Shuffle(0);
        } else if read_eq!("shuffle-sections") {
            let seed = parse_number("shuffle-sections", arg) as u64;
            a.shuffle_sections = ShuffleSections::Shuffle(seed);
            shuffle_sections_seed = Some(seed);
        } else if cursor.read_flag("reverse-sections") {
            a.shuffle_sections = ShuffleSections::Reverse;
        } else if let Some(value) = cursor.read_switch("rosegment", "no-rosegment") {
            a.rosegment = value;
        } else if read_arg!("y", true) || read_arg!("trace-symbol", true) {
            a.trace_symbol.push(raw_arg.as_encoded_bytes().to_vec());
        } else if read_arg!("filler") {
            a.filler = Some(parse_hex("filler", arg) as u8);
        } else if read_arg!("L", true) || read_arg!("library-path", true) {
            a.library_paths.push(PathBuf::from(raw_arg));
        } else if read_arg!("sysroot", true) {
            a.sysroot = PathBuf::from(raw_arg);
        } else if read_arg!("unique", true) {
            if !unique.add(raw_arg.as_encoded_bytes(), 1) {
                fatal!("-unique: invalid glob pattern: {}", raw_arg.to_string_lossy());
            }
        } else if read_arg!("unresolved-symbols") {
            match arg {
                "report-all" | "ignore-in-shared-libs" => report_undefined = Some(true),
                "ignore-all" | "ignore-in-object-files" => report_undefined = Some(false),
                _ => fatal!("unknown --unresolved-symbols argument: {arg}"),
            }
        } else if read_arg!("--undefined-glob", true) {
            if !undefined_glob.add(raw_arg.as_encoded_bytes(), 0) {
                fatal!("--undefined-glob: invalid pattern: {}", raw_arg.to_string_lossy());
            }
        } else if read_arg!("require-defined", true) {
            a.require_defined.push(raw_arg.as_encoded_bytes().to_vec());
        } else if read_arg!("init", true) {
            a.init = raw_arg.as_encoded_bytes().to_vec();
        } else if read_arg!("fini", true) {
            a.fini = raw_arg.as_encoded_bytes().to_vec();
        } else if read_arg!("hash-style") {
            match arg {
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
                _ => fatal!("invalid --hash-style argument: {arg}"),
            }
        } else if read_arg!("soname", true) || read_arg!("h", true) {
            a.soname = raw_arg.to_os_string();
        } else if read_arg!("audit", true) {
            if !a.audit.is_empty() {
                a.audit.push(b':');
            }
            a.audit.extend_from_slice(raw_arg.as_encoded_bytes());
        } else if read_arg!("depaudit", true) || read_arg!("P", true) {
            if !a.depaudit.is_empty() {
                a.depaudit.push(b':');
            }
            a.depaudit.extend_from_slice(raw_arg.as_encoded_bytes());
        } else if cursor.read_flag("allow-multiple-definition") {
            a.allow_multiple_definition = true;
        } else if let Some(value) =
            cursor.read_switch("apply-dynamic-relocs", "no-apply-dynamic-relocs")
        {
            a.apply_dynamic_relocs = value;
        } else if cursor.read_flag("trace") {
            a.trace = true;
        } else if let Some(value) = cursor.read_switch("eh-frame-hdr", "no-eh-frame-hdr") {
            a.eh_frame_hdr = value;
        } else if cursor.read_flag("pie") || cursor.read_flag("pic-executable") {
            a.pic = true;
            a.pie = true;
        } else if cursor.read_flag("no-pie")
            || cursor.read_flag("no-pic-executable")
            || cursor.read_flag("nopie")
        {
            a.pic = false;
            a.pie = false;
        } else if let Some(value) = cursor.read_switch("relax", "no-relax") {
            a.relax = value;
        } else if let Some(value) = cursor.read_switch("gdb-index", "no-gdb-index") {
            a.gdb_index = value;
        } else if cursor.read_flag("r") || cursor.read_flag("relocatable") {
            a.relocatable = true;
            a.emit_relocs = true;
        } else if cursor.read_flag("relocatable-merge-sections") {
            a.relocatable_merge_sections = true;
        } else if cursor.read_flag("perf") {
            a.perf = true;
        } else if cursor.read_flag("pack-dyn-relocs=relr")
            || cursor.read_z_flag("pack-relative-relocs")
        {
            a.pack_dyn_relocs_relr = true;
            a.pack_dyn_relocs_android = false;
        } else if cursor.read_flag("pack-dyn-relocs=android") {
            a.pack_dyn_relocs_android = true;
            a.pack_dyn_relocs_relr = false;
        } else if cursor.read_flag("pack-dyn-relocs=android+relr") {
            a.pack_dyn_relocs_android = true;
            a.pack_dyn_relocs_relr = true;
        } else if cursor.read_flag("pack-dyn-relocs=none")
            || cursor.read_z_flag("nopack-relative-relocs")
        {
            a.pack_dyn_relocs_relr = false;
            a.pack_dyn_relocs_android = false;
        } else if let Some(value) =
            cursor.read_switch("--use-android-relr-tags", "no-use-android-relr-tags")
        {
            a.use_android_relr_tags = value;
        } else if read_eq!("package-metadata", true) {
            a.package_metadata = parse_package_metadata(raw_arg.as_encoded_bytes());
        } else if cursor.read_flag("stats") {
            a.stats = true;
            Counter::enable();
        } else if read_arg!("C", true) || read_arg!("directory", true) {
            directory = PathBuf::from(raw_arg);
        } else if read_arg!("chroot", true) {
            a.chroot = PathBuf::from(raw_arg);
        } else if cursor.read_flag("color-diagnostics")
            || cursor.read_flag("color-diagnostics=auto")
        {
            mold_common::error::set_color(std::io::stderr().is_terminal());
        } else if cursor.read_flag("color-diagnostics=always") {
            mold_common::error::set_color(true);
        } else if cursor.read_flag("color-diagnostics=never")
            || cursor.read_flag("no-color-diagnostics")
        {
            mold_common::error::set_color(false);
        } else if let Some(value) = cursor.read_switch("warn-common", "no-warn-common") {
            a.warn_common = value;
        } else if cursor.read_flag("warn-once") {
            // Ignored for GNU ld compatibility, as in C++ mold.
        } else if cursor.read_flag("warn-shared-textrel") {
            warn_shared_textrel = true;
        } else if cursor.read_flag("warn-textrel") {
            a.warn_textrel = true;
        } else if let Some(value) = cursor.read_switch("enable-new-dtags", "disable-new-dtags") {
            a.enable_new_dtags = value;
        } else if cursor.read_flag("--execute-only") {
            a.execute_only = true;
        } else if cursor.read_flag("zero-to-bss") {
            a.zero_to_bss = true;
        } else if read_arg!("compress-debug-sections") {
            a.compress_debug_sections = match arg {
                "zlib" | "zlib-gabi" => DebugCompression::Zlib(1),
                "zstd" => DebugCompression::Zstd(3),
                "none" => DebugCompression::None,
                s if s.starts_with("zlib:") => {
                    let level = parse_number("compress-debug-sections", &s[5..]);
                    if !(0..=9).contains(&level) {
                        fatal!(
                            "invalid --compress-debug-sections argument: {arg} (zlib level must be between 0 and 9)"
                        );
                    }
                    DebugCompression::Zlib(level as i32)
                }
                s if s.starts_with("zstd:") => {
                    let level = parse_number("compress-debug-sections", &s[5..]);
                    if !(1..=22).contains(&level) {
                        fatal!(
                            "invalid --compress-debug-sections argument: {arg} (zstd level must be between 1 and 22)"
                        );
                    }
                    DebugCompression::Zstd(level as i32)
                }
                _ => fatal!("invalid --compress-debug-sections argument: {arg}"),
            };
        } else if read_arg!("wrap", true) {
            a.wrap.insert(raw_arg.as_encoded_bytes().to_vec());
        } else if cursor.read_flag("--omagic") || cursor.read_flag("N") {
            a.omagic = true;
            rctx.is_static = true;
        } else if cursor.read_flag("--no-omagic") {
            a.omagic = false;
        } else if read_arg!("--oformat") {
            if arg != "binary" {
                fatal!("--oformat: {arg} is not supported");
            }
            a.oformat_binary = true;
        } else if read_arg!("retain-symbols-file", true) {
            a.retain_symbols_file = Some(read_retain_symbols_file(&a.chroot, Path::new(raw_arg)));
        } else if read_arg!("section-align", true) {
            let arg = raw_arg.as_encoded_bytes();
            let Some((name, value)) = arg.split_once_str(b"=").filter(|(_, v)| !v.is_empty())
            else {
                fatal!("--section-align: syntax error: {}", arg.as_bstr());
            };
            let value = std::str::from_utf8(value)
                .unwrap_or_else(|_| fatal!("--section-align: invalid number: {}", value.as_bstr()));
            let value = parse_number("section-align", value);
            if value <= 0 || !(value as u64).is_power_of_two() {
                fatal!("--section-align={}: value must be a power of 2", arg.as_bstr());
            }
            a.section_align.insert(name.to_vec(), value as u64);
        } else if read_arg!("section-start", true) {
            let arg = raw_arg.as_encoded_bytes();
            let Some((name, value)) = arg.split_once_str(b"=").filter(|(_, v)| !v.is_empty())
            else {
                fatal!("--section-start: syntax error: {}", arg.as_bstr());
            };
            let value = std::str::from_utf8(value)
                .unwrap_or_else(|_| fatal!("--section-start: invalid number: {}", value.as_bstr()));
            a.section_start.insert(name.to_vec(), parse_hex("section-start", value));
        } else if read_arg!("section-order", true) {
            a.section_order = parse_section_order(raw_arg.as_encoded_bytes());
        } else if read_arg!("Tbss") {
            a.section_start.insert(b".bss".to_vec(), parse_hex("Tbss", arg));
        } else if read_arg!("Tdata") {
            a.section_start.insert(b".data".to_vec(), parse_hex("Tdata", arg));
        } else if read_arg!("Ttext") {
            a.section_start.insert(b".text".to_vec(), parse_hex("Ttext", arg));
        } else if read_arg!("Ttext-segment") {
            a.ttext_segment = Some(parse_number("Ttext-segment", arg) as u64);
        } else if cursor.read_flag("repro") {
            a.repro = true;
        } else if let Some(value) = cursor.read_z_switch("now", "lazy") {
            a.z_now = value;
        } else if cursor.read_z_flag("cet-report=none") {
            a.z_cet_report = ReportKind::None;
        } else if cursor.read_z_flag("cet-report=warning") {
            a.z_cet_report = ReportKind::Warning;
        } else if cursor.read_z_flag("cet-report=error") {
            a.z_cet_report = ReportKind::Error;
        } else if cursor.read_z_flag("bti-report=none") {
            z_bti_report = Some(ReportKind::None);
        } else if cursor.read_z_flag("bti-report=warning") {
            z_bti_report = Some(ReportKind::Warning);
        } else if cursor.read_z_flag("bti-report=error") {
            z_bti_report = Some(ReportKind::Error);
        } else if cursor.read_z_flag("gcs-report=none") {
            z_gcs_report = Some(ReportKind::None);
        } else if cursor.read_z_flag("gcs-report=warning") {
            z_gcs_report = Some(ReportKind::Warning);
        } else if cursor.read_z_flag("gcs-report=error") {
            z_gcs_report = Some(ReportKind::Error);
        } else if cursor.read_z_flag("gcs-report-dynamic=none") {
            z_gcs_report_dynamic = Some(ReportKind::None);
        } else if cursor.read_z_flag("gcs-report-dynamic=warning") {
            z_gcs_report_dynamic = Some(ReportKind::Warning);
        } else if cursor.read_z_flag("gcs-report-dynamic=error") {
            z_gcs_report_dynamic = Some(ReportKind::Error);
        } else if cursor.read_z_flag("execstack") {
            a.z_execstack = true;
        } else if cursor.read_z_flag("execstack-if-needed") {
            a.z_execstack_if_needed = true;
        } else if read_z_arg!("max-page-size") {
            a.page_size = parse_number("-z max-page-size", arg) as u64;
            if !a.page_size.is_power_of_two() {
                fatal!("-z max-page-size {arg}: value must be a power of 2");
            }
        } else if let Some(value) =
            cursor.read_z_switch("start-stop-visibility=protected", "start-stop-visibility=hidden")
        {
            a.z_start_stop_visibility_protected = value;
        } else if cursor.read_z_flag("noexecstack") {
            a.z_execstack = false;
        } else if cursor.read_z_flag("relro") {
            z_relro = Some(true);
        } else if cursor.read_z_flag("norelro") {
            z_relro = Some(false);
        } else if cursor.read_z_flag("defs") || cursor.read_flag("no-undefined") {
            report_undefined = Some(true);
        } else if cursor.read_z_flag("undefs") {
            report_undefined = Some(false);
        } else if cursor.read_z_flag("nodlopen") {
            a.z_dlopen = false;
        } else if cursor.read_z_flag("nodelete") {
            a.z_delete = false;
        } else if cursor.read_z_flag("nocopyreloc") {
            a.z_copyreloc = false;
        } else if cursor.read_z_flag("nodump") {
            a.z_dump = false;
        } else if cursor.read_z_flag("global") {
            a.z_global = true;
        } else if cursor.read_z_flag("initfirst") {
            a.z_initfirst = true;
        } else if cursor.read_z_flag("interpose") {
            a.z_interpose = true;
        } else if cursor.read_z_flag("ibt") {
            a.z_ibt = true;
        } else if cursor.read_z_flag("ibtplt") {
        } else if cursor.read_z_flag("force-bti") {
            a.z_force_bti = true;
        } else if cursor.read_z_flag("pac-plt") {
            a.z_pac_plt = true;
        } else if cursor.read_z_flag("gcs=implicit") {
            a.z_gcs = GcsKind::Implicit;
        } else if cursor.read_z_flag("gcs=never") {
            a.z_gcs = GcsKind::Never;
        } else if cursor.read_z_flag("gcs=always") {
            a.z_gcs = GcsKind::Always;
        } else if cursor.read_z_flag("muldefs") {
            a.allow_multiple_definition = true;
        } else if let Some(value) =
            cursor.read_z_switch("keep-text-section-prefix", "nokeep-text-section-prefix")
        {
            a.z_keep_text_section_prefix = value;
        } else if cursor.read_z_flag("shstk") {
            a.z_shstk = true;
        } else if cursor.read_z_flag("text") {
            a.z_text = true;
        } else if cursor.read_z_flag("notext") || cursor.read_z_flag("textoff") {
            a.z_text = false;
        } else if cursor.read_z_flag("origin") {
            a.z_origin = true;
        } else if cursor.read_z_flag("nodefaultlib") {
            a.z_nodefaultlib = true;
        } else if read_eq!("separate-debug-file", true) {
            separate_debug_file = Some(PathBuf::from(raw_arg));
        } else if cursor.read_flag("separate-debug-file") {
            separate_debug_file = Some(PathBuf::new());
        } else if cursor.read_flag("no-separate-debug-file") {
            separate_debug_file = None;
        } else if cursor.read_z_flag("separate-loadable-segments") {
            z_separate_code = Some(SeparateCodeKind::SeparateLoadableSegments);
        } else if cursor.read_z_flag("separate-code") {
            z_separate_code = Some(SeparateCodeKind::SeparateCode);
        } else if cursor.read_z_flag("noseparate-code") {
            z_separate_code = Some(SeparateCodeKind::NoSeparateCode);
        } else if read_z_arg!("stack-size") {
            a.z_stack_size = parse_number("-z stack-size", arg) as u64;
        } else if cursor.read_z_flag("dynamic-undefined-weak") {
            z_dynamic_undefined_weak = Some(true);
        } else if cursor.read_z_flag("nodynamic-undefined-weak") {
            z_dynamic_undefined_weak = Some(false);
        } else if let Some(value) = cursor.read_z_switch("sectionheader", "nosectionheader") {
            a.z_sectionheader = value;
        } else if cursor.read_z_flag("rodynamic") {
            a.z_rodynamic = true;
        } else if cursor.read_z_flag("x86-64-v2") {
            a.z_x86_64_isa_level |= GNU_PROPERTY_X86_ISA_1_V2;
        } else if cursor.read_z_flag("x86-64-v3") {
            a.z_x86_64_isa_level |= GNU_PROPERTY_X86_ISA_1_V3;
        } else if cursor.read_z_flag("x86-64-v4") {
            a.z_x86_64_isa_level |= GNU_PROPERTY_X86_ISA_1_V4;
        } else if cursor.read_z_flag("rewrite-endbr") {
            if !matches!(target.family, Family::X86_64 | Family::Arm64) {
                fatal!("-z rewrite-endbr is supported only on x86-64 and arm64");
            }
            a.z_rewrite_endbr = true;
        } else if cursor.read_z_flag("norewrite-endbr") {
            a.z_rewrite_endbr = false;
        } else if let Some(value) = cursor.read_switch("nmagic", "no-nmagic") {
            a.nmagic = value;
        } else if cursor.read_flag("fatal-warnings") {
            mold_common::error::set_fatal_warnings(true);
        } else if cursor.read_flag("no-fatal-warnings") {
            mold_common::error::set_fatal_warnings(false);
        } else if cursor.read_flag("w") || cursor.read_flag("no-warnings") {
            mold_common::error::set_suppress_warnings(true);
        } else if let Some(value) = cursor.read_switch("fork", "no-fork") {
            a.fork = value;
        } else if let Some(value) = cursor.read_switch("--mmap-output-file", "no-mmap-output-file")
        {
            a.mmap_output_file = value;
        } else if let Some(value) = cursor.read_switch("gc-sections", "no-gc-sections") {
            a.gc_sections = value;
        } else if cursor.read_flag("print-gc-sections") {
            a.print_gc_sections = Some(ReportOutput::Stdout);
        } else if read_eq!("print-gc-sections", true) {
            a.print_gc_sections = parse_report_output(raw_arg);
        } else if cursor.read_flag("no-print-gc-sections") {
            a.print_gc_sections = None;
        } else if read_arg!("discard-section", true) {
            a.discard_section.insert(raw_arg.as_encoded_bytes().to_vec());
        } else if read_arg!("no-discard-section", true) {
            a.discard_section.remove(raw_arg.as_encoded_bytes());
        } else if read_arg!("icf") {
            match arg {
                "all" => {
                    a.icf = true;
                    a.icf_all = true;
                }
                "safe" => a.icf = true,
                "none" => a.icf = false,
                _ => fatal!("unknown --icf argument: {arg}"),
            }
        } else if cursor.read_flag("no-icf") {
            a.icf = false;
        } else if cursor.read_flag("ignore-data-address-equality") {
            a.ignore_data_address_equality = true;
        } else if read_arg!("image-base") {
            a.image_base = parse_number("image-base", arg) as u64;
        } else if read_arg!("physical-image-base") {
            a.physical_image_base = Some(parse_number("physical-image-base", arg) as u64);
        } else if cursor.read_flag("print-icf-sections") {
            a.print_icf_sections = Some(ReportOutput::Stdout);
        } else if read_eq!("print-icf-sections", true) {
            a.print_icf_sections = parse_report_output(raw_arg);
        } else if cursor.read_flag("no-print-icf-sections") {
            a.print_icf_sections = None;
        } else if let Some(value) = cursor.read_switch("quick-exit", "no-quick-exit") {
            a.quick_exit = value;
        } else if read_arg!("plugin", true) {
            a.plugin = PathBuf::from(raw_arg);
        } else if read_arg!("plugin-opt", true) {
            a.plugin_opt.push(raw_arg.as_encoded_bytes().to_vec());
        } else if let Some(option) = cursor.read_lto_option() {
            a.plugin_opt.push(option);
        } else if read_arg!("thread-count") {
            a.thread_count = Some(parse_number("thread-count", arg).max(1) as usize);
        } else if cursor.read_flag("threads") {
            a.thread_count = None;
        } else if cursor.read_flag("no-threads") {
            a.thread_count = Some(1);
        } else if read_eq!("threads") {
            a.thread_count = Some(parse_number("threads", arg).max(1) as usize);
        } else if cursor.read_flag("discard-all") || cursor.read_flag("x") {
            a.discard_all = true;
        } else if cursor.read_flag("discard-locals") || cursor.read_flag("X") {
            a.discard_locals = true;
        } else if cursor.read_flag("discard-none") {
            a.discard_all = false;
            a.discard_locals = false;
        } else if cursor.read_flag("strip-all") || cursor.read_flag("s") {
            a.strip_all = true;
        } else if cursor.read_flag("strip-debug") || cursor.read_flag("S") {
            a.strip_debug = true;
        } else if cursor.read_flag("warn-unresolved-symbols") {
            error_unresolved_symbols = false;
        } else if cursor.read_flag("error-unresolved-symbols") {
            error_unresolved_symbols = true;
        } else if read_arg!("rpath", true) {
            add_rpath(&mut a, &mut rpaths, raw_arg);
        } else if read_arg!("R", true) {
            if Path::new(raw_arg).metadata().is_ok_and(|m| !m.is_dir()) {
                fatal!(
                    "-R{}: -R as an alias for --just-symbols is not supported",
                    raw_arg.to_string_lossy()
                );
            }
            add_rpath(&mut a, &mut rpaths, raw_arg);
        } else if let Some(value) =
            cursor.read_switch("--undefined-version", "no-undefined-version")
        {
            a.undefined_version = value;
        } else if read_arg!("undefined", true) || read_arg!("u", true) {
            a.undefined.push(raw_arg.as_encoded_bytes().to_vec());
        } else if cursor.read_flag("build-id") {
            a.build_id = BuildId::Hash(20);
        } else if read_arg!("build-id") {
            a.build_id = match arg {
                "none" => BuildId::None,
                "uuid" => BuildId::Uuid,
                "md5" => BuildId::Hash(16),
                "sha1" => BuildId::Hash(20),
                "sha256" | "fast" => BuildId::Hash(32),
                s if s.starts_with("0x") || s.starts_with("0X") => {
                    BuildId::Hex(parse_hex_build_id(s))
                }
                _ => fatal!("invalid --build-id argument: {arg}"),
            };
        } else if cursor.read_flag("no-build-id") {
            a.build_id = BuildId::None;
        } else if let Some(value) = cursor.read_switch("be8", "be32") {
            be8 = value;
        } else if read_arg!("--format") || read_arg!("b") {
            if arg == "binary" {
                fatal!(
                    "mold does not support `-b binary`. If you want to convert a binary file into an \
                     object file, use `objcopy -I binary -O default <input-file> <output-file.o>` instead."
                );
            }
            fatal!("unknown command line option: -b {arg}");
        } else if read_arg!("fuse-ld", true) {
        } else if cursor.read_flag("allow-shlib-undefined") {
            allow_shlib_undefined = Some(true);
        } else if cursor.read_flag("no-allow-shlib-undefined") {
            allow_shlib_undefined = Some(false);
        } else if read_arg!("O", true)
            || cursor.read_flag("EB")
            || cursor.read_flag("EL")
            || cursor.read_flag("O0")
            || cursor.read_flag("O1")
            || cursor.read_flag("O2")
            || cursor.read_flag("verbose")
            || cursor.read_flag("start-group")
            || cursor.read_flag("end-group")
            || cursor.read_flag("(")
            || cursor.read_flag(")")
            || cursor.read_flag("nostdlib")
            || cursor.read_flag("no-add-needed")
            || cursor.read_flag("no-call-graph-profile-sort")
            || cursor.read_flag("no-copy-dt-needed-entries")
            || read_arg!("sort-section", true)
            || cursor.read_flag("sort-common")
            || cursor.read_flag("dc")
            || cursor.read_flag("dp")
            || cursor.read_flag("fix-cortex-a53-835769")
            || cursor.read_flag("fix-cortex-a53-843419")
            || cursor.read_flag("nodefaultlibs")
            || cursor.read_flag("warn-constructors")
            || cursor.read_flag("warn-execstack")
            || cursor.read_flag("no-warn-execstack")
            || cursor.read_flag("no-error-execstack")
            || cursor.read_flag("no-warn-rwx-segments")
            || cursor.read_flag("no-error-rwx-segments")
            || cursor.read_flag("long-plt")
            || cursor.read_flag("secure-plt")
            || read_arg!("rpath-link", true)
            || cursor.read_z_flag("combreloc")
            || cursor.read_z_flag("nocombreloc")
            || read_z_arg!("common-page-size")
            || cursor.read_flag("no-keep-memory")
            || read_arg!("max-cache-size", true)
        {
            // Ignored for compatibility.
        } else if read_arg!("m") {
            match emulation_to_target(arg) {
                Some(name) => a.emulation = name,
                None => fatal!("unknown -m argument: {arg}"),
            }
        } else if read_arg!("filter", true) || read_arg!("F", true) {
            a.filter.push(raw_arg.as_encoded_bytes().to_vec());
        } else if read_arg!("auxiliary", true) || read_arg!("f", true) {
            a.auxiliary.push(raw_arg.as_encoded_bytes().to_vec());
        } else if read_arg!("version-script", true) {
            a.version_scripts.push(PathBuf::from(raw_arg));
        } else if read_arg!("dynamic-list", true) {
            a.bsymbolic = BsymbolicKind::All;
            let mf = mold_common::mapped_file::must_open_file(&a.chroot, raw_arg);
            a.dynamic_list.push(DynamicListSource::File(mf));
        } else if cursor.read_flag("dynamic-list-data") {
            a.dynamic_list_data = true;
        } else if read_arg!("--export-dynamic-symbol", true) {
            a.dynamic_list.push(DynamicListSource::Pattern(raw_arg.as_encoded_bytes().to_vec()));
        } else if read_arg!("--export-dynamic-symbol-list", true) {
            let mf = mold_common::mapped_file::must_open_file(&a.chroot, raw_arg);
            a.dynamic_list.push(DynamicListSource::File(mf));
        } else if read_arg!("entry", true) || read_arg!("e", true) {
            a.entry = raw_arg.as_encoded_bytes().to_vec();
        } else if let Some(value) = cursor.read_switch("as-needed", "no-as-needed") {
            rctx.as_needed = value;
        } else if let Some(value) = cursor.read_switch("whole-archive", "no-whole-archive") {
            rctx.whole_archive = value;
        } else if read_arg!("--library", true) || read_arg!("l", true) {
            if visited_libs.insert(raw_arg) {
                let mut job =
                    ReaderJob { rctx: rctx.clone(), name: PathBuf::from(raw_arg), is_lib: true };
                job.rctx.pos = vec![jobs.len() as u32];
                jobs.push(job);
            }
        } else if read_arg!("script", true) || read_arg!("T", true) {
            let mut job = ReaderJob {
                rctx: rctx.clone(),
                name: PathBuf::from(raw_arg),
                ..Default::default()
            };
            job.rctx.pos = vec![jobs.len() as u32];
            jobs.push(job);
        } else if cursor.read_flag("push-state") {
            rctx_stack.push(rctx.clone());
        } else if cursor.read_flag("pop-state") {
            rctx = rctx_stack.pop().unwrap_or_else(|| fatal!("no state pushed before popping"));
        } else if cursor.text().starts_with("-z") && cursor.text().len() > 2 {
            warn!("unknown command line option: {}", cursor.text());
            cursor.index += 1;
        } else if cursor.text() == "-z" && cursor.index + 1 < raw_cmdline.len() {
            warn!(
                "unknown command line option: -z {}",
                raw_cmdline[cursor.index + 1].to_str().unwrap_or("")
            );
            cursor.index += 2;
        } else if cursor.text() == "-dynamic" {
            fatal!(
                "unknown command line option: -dynamic; -dynamic is a macOS linker's option. mold does not support macOS."
            );
        } else {
            fatal!("unknown command line option: {}", cursor.current().to_string_lossy());
        }
    }

    // winnow-args lexes the whole command line into options, in order;
    // each is handled as the built-in parser handles it.
    #[cfg(feature = "winnow-args")]
    {
        use crate::cmdline_winnow::{Item, utf8_arg};

        let mut opts = crate::cmdline_winnow::parse(raw_cmdline);
        for opt in &mut opts {
            if let Item::Z(z) = opt
                && let Some(known) = crate::cmdline_winnow::z_opt(&z.value)
            {
                *opt = known;
            }
            match opt {
                Item::Input(value_os) => {
                    let mut job = ReaderJob {
                        rctx: rctx.clone(),
                        name: PathBuf::from(std::mem::take(value_os)),
                        ..Default::default()
                    };
                    job.rctx.pos = vec![jobs.len() as u32];
                    jobs.push(job);
                }
                Item::Help => {
                    out!("Usage: {} [options] file...\n{}", raw_cmdline[0].to_string_lossy(), HELP);
                    std::process::exit(0);
                }
                Item::OutputShort(value_os) | Item::Output(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.output = PathBuf::from(raw_arg);
                }
                Item::DynamicLinker(value_os) | Item::DynamicLinkerShort(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.dynamic_linker = PathBuf::from(raw_arg);
                }
                Item::NoDynamicLinker => {
                    a.dynamic_linker.clear();
                }
                Item::ShortVLower => {
                    out!("{}", *VERSION);
                    version_shown = true;
                }
                Item::Version => {
                    out!("{}", *VERSION);
                    std::process::exit(0);
                }
                Item::ShortV => {
                    out!(
                        "{}\n  Supported emulations:\n   elf_x86_64\n   elf_i386\n   aarch64elf\n   \
                             aarch64linux\n   aarch64elfb\n   aarch64linuxb\n   armelf_linux_eabi\n   elf64lriscv\n   \
                             elf64briscv\n   elf32lriscv\n   elf32briscv\n   elf32ppc\n   elf64ppc\n   elf64lppc\n   \
                             elf64_s390\n   elf64_sparc\n   m68kelf\n   shlelf_linux\n   shelf_linux\n   \
                             elf64loongarch\n   elf32loongarch",
                        *VERSION
                    );
                    version_shown = true;
                }
                Item::Mllvm(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.plugin_opt.push(raw_arg.as_encoded_bytes().to_vec());
                }
                Item::EndLib => {
                    rctx.in_lib = false;
                }
                Item::ExportDynamic | Item::ExportDynamicShort => {
                    a.export_dynamic = true;
                }
                Item::NoExportDynamic => {
                    a.export_dynamic = false;
                }
                Item::Bsymbolic => {
                    a.bsymbolic = BsymbolicKind::All;
                }
                Item::BsymbolicFunctions => {
                    a.bsymbolic = BsymbolicKind::Functions;
                }
                Item::BsymbolicNonWeak => {
                    a.bsymbolic = BsymbolicKind::NonWeak;
                }
                Item::BsymbolicNonWeakFunctions => {
                    a.bsymbolic = BsymbolicKind::NonWeakFunctions;
                }
                Item::BnoSymbolic => {
                    a.bsymbolic = BsymbolicKind::None;
                }
                Item::ExcludeLibs(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    for lib in raw_arg.as_encoded_bytes().split(|b| matches!(b, b',' | b':')) {
                        a.exclude_libs.insert(lib.to_vec());
                    }
                }
                Item::EmitRelocsShort | Item::EmitRelocs => {
                    a.emit_relocs = true;
                    a.discard_locals = false;
                }
                Item::Map(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    map_path = Some(PathBuf::from(raw_arg));
                }
                Item::PrintDependencies => {
                    a.print_dependencies = true;
                }
                Item::PrintMap | Item::PrintMapShort => {
                    map_path.get_or_insert_with(PathBuf::new);
                }
                Item::Bstatic | Item::Dn | Item::Static => {
                    rctx.is_static = true;
                }
                Item::Bdynamic | Item::Dy => {
                    rctx.is_static = false;
                }
                Item::Shared | Item::Bshareable => {
                    a.shared = true;
                }
                Item::SpareDynamicTags(value_os) => {
                    let arg = utf8_arg(value_os, "--spare-dynamic-tags");
                    a.spare_dynamic_tags = parse_number("spare-dynamic-tags", arg);
                }
                Item::SpareProgramHeaders(value_os) => {
                    let arg = utf8_arg(value_os, "--spare-program-headers");
                    a.spare_program_headers = parse_number("spare-program-headers", arg);
                }
                Item::StartLib => {
                    rctx.in_lib = true;
                }
                Item::StartStop => {
                    a.start_stop = true;
                }
                Item::DependencyFile(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.dependency_file = PathBuf::from(raw_arg);
                }
                Item::Defsym(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    let Some((name, value)) = raw_arg
                        .as_encoded_bytes()
                        .split_once_str(b"=")
                        .filter(|(_, v)| !v.is_empty())
                    else {
                        fatal!("-defsym: syntax error: {}", raw_arg.to_string_lossy());
                    };
                    a.defsyms.push((name.to_vec(), parse_defsym_value(value)));
                }
                Item::InternalLtoPass2 => {
                    a.lto_pass2 = true;
                }
                Item::InternalIgnoreIrFile(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.ignore_ir_file.insert(raw_arg.to_os_string());
                }
                Item::Demangle(_) => {
                    mold_common::error::set_demangle(true);
                }
                Item::NoDemangle => {
                    mold_common::error::set_demangle(false);
                }
                Item::Detach => {
                    let value = true;
                    a.detach = value;
                }
                Item::NoDetach => {
                    let value = false;
                    a.detach = value;
                }
                Item::DefaultSymver => {
                    a.default_symver = true;
                }
                Item::NoinhibitExec => {
                    mold_common::error::set_noinhibit_exec(true);
                }
                Item::ShuffleSections(value_os) if value_os.as_encoded_bytes() == b"\0" => {
                    // Resolve the seed after parsing all options.
                    a.shuffle_sections = ShuffleSections::Shuffle(0);
                }
                Item::ShuffleSections(value_os) => {
                    let arg = utf8_arg(value_os, "--shuffle-sections");
                    let seed = parse_number("shuffle-sections", arg) as u64;
                    a.shuffle_sections = ShuffleSections::Shuffle(seed);
                    shuffle_sections_seed = Some(seed);
                }
                Item::ReverseSections => {
                    a.shuffle_sections = ShuffleSections::Reverse;
                }
                Item::Rosegment => {
                    let value = true;
                    a.rosegment = value;
                }
                Item::NoRosegment => {
                    let value = false;
                    a.rosegment = value;
                }
                Item::TraceSymbolShort(value_os) | Item::TraceSymbol(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.trace_symbol.push(raw_arg.as_encoded_bytes().to_vec());
                }
                Item::Filler(value_os) => {
                    let arg = utf8_arg(value_os, "--filler");
                    a.filler = Some(parse_hex("filler", arg) as u8);
                }
                Item::LibraryPathShort(value_os) | Item::LibraryPath(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.library_paths.push(PathBuf::from(raw_arg));
                }
                Item::Sysroot(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.sysroot = PathBuf::from(raw_arg);
                }
                Item::Unique(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    if !unique.add(raw_arg.as_encoded_bytes(), 1) {
                        fatal!("-unique: invalid glob pattern: {}", raw_arg.to_string_lossy());
                    }
                }
                Item::UnresolvedSymbols(value_os) => {
                    let arg = utf8_arg(value_os, "--unresolved-symbols");
                    match arg {
                        "report-all" | "ignore-in-shared-libs" => report_undefined = Some(true),
                        "ignore-all" | "ignore-in-object-files" => report_undefined = Some(false),
                        _ => fatal!("unknown --unresolved-symbols argument: {arg}"),
                    }
                }
                Item::UndefinedGlob(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    if !undefined_glob.add(raw_arg.as_encoded_bytes(), 0) {
                        fatal!("--undefined-glob: invalid pattern: {}", raw_arg.to_string_lossy());
                    }
                }
                Item::RequireDefined(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.require_defined.push(raw_arg.as_encoded_bytes().to_vec());
                }
                Item::Init(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.init = raw_arg.as_encoded_bytes().to_vec();
                }
                Item::Fini(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.fini = raw_arg.as_encoded_bytes().to_vec();
                }
                Item::HashStyle(value_os) => {
                    let arg = utf8_arg(value_os, "--hash-style");
                    match arg {
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
                        _ => fatal!("invalid --hash-style argument: {arg}"),
                    }
                }
                Item::Soname(value_os) | Item::SonameShort(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.soname = raw_arg.to_os_string();
                }
                Item::Audit(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    if !a.audit.is_empty() {
                        a.audit.push(b':');
                    }
                    a.audit.extend_from_slice(raw_arg.as_encoded_bytes());
                }
                Item::Depaudit(value_os) | Item::DepauditShort(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    if !a.depaudit.is_empty() {
                        a.depaudit.push(b':');
                    }
                    a.depaudit.extend_from_slice(raw_arg.as_encoded_bytes());
                }
                Item::AllowMultipleDefinition => {
                    a.allow_multiple_definition = true;
                }
                Item::ApplyDynamicRelocs => {
                    let value = true;
                    a.apply_dynamic_relocs = value;
                }
                Item::NoApplyDynamicRelocs => {
                    let value = false;
                    a.apply_dynamic_relocs = value;
                }
                Item::Trace | Item::TraceShort => {
                    a.trace = true;
                }
                Item::EhFrameHdr => {
                    let value = true;
                    a.eh_frame_hdr = value;
                }
                Item::NoEhFrameHdr => {
                    let value = false;
                    a.eh_frame_hdr = value;
                }
                Item::Pie | Item::PicExecutable => {
                    a.pic = true;
                    a.pie = true;
                }
                Item::NoPie | Item::NoPicExecutable | Item::Nopie => {
                    a.pic = false;
                    a.pie = false;
                }
                Item::Relax => {
                    let value = true;
                    a.relax = value;
                }
                Item::NoRelax => {
                    let value = false;
                    a.relax = value;
                }
                Item::GdbIndex => {
                    let value = true;
                    a.gdb_index = value;
                }
                Item::NoGdbIndex => {
                    let value = false;
                    a.gdb_index = value;
                }
                Item::RelocatableShort | Item::RelocatableShortI | Item::Relocatable => {
                    a.relocatable = true;
                    a.emit_relocs = true;
                }
                Item::RelocatableMergeSections => {
                    a.relocatable_merge_sections = true;
                }
                Item::Perf => {
                    a.perf = true;
                }
                Item::PackDynRelocs(value_os) if value_os.as_encoded_bytes() == b"relr" => {
                    a.pack_dyn_relocs_relr = true;
                    a.pack_dyn_relocs_android = false;
                }
                Item::PackDynRelocs(value_os) if value_os.as_encoded_bytes() == b"android" => {
                    a.pack_dyn_relocs_android = true;
                    a.pack_dyn_relocs_relr = false;
                }
                Item::PackDynRelocs(value_os) if value_os.as_encoded_bytes() == b"android+relr" => {
                    a.pack_dyn_relocs_android = true;
                    a.pack_dyn_relocs_relr = true;
                }
                Item::PackDynRelocs(value_os) if value_os.as_encoded_bytes() == b"none" => {
                    a.pack_dyn_relocs_relr = false;
                    a.pack_dyn_relocs_android = false;
                }
                Item::PackDynRelocs(value_os) => {
                    fatal!(
                        "unknown command line option: --pack-dyn-relocs={}",
                        value_os.to_string_lossy()
                    );
                }
                Item::UseAndroidRelrTags => {
                    let value = true;
                    a.use_android_relr_tags = value;
                }
                Item::NoUseAndroidRelrTags => {
                    let value = false;
                    a.use_android_relr_tags = value;
                }
                Item::PackageMetadata(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.package_metadata = parse_package_metadata(raw_arg.as_encoded_bytes());
                }
                Item::Stats => {
                    a.stats = true;
                    Counter::enable();
                }
                Item::DirectoryShort(value_os) | Item::Directory(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    directory = PathBuf::from(raw_arg);
                }
                Item::Chroot(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.chroot = PathBuf::from(raw_arg);
                }
                Item::ColorDiagnostics(value_os) if value_os.as_encoded_bytes() == b"\0" => {
                    mold_common::error::set_color(std::io::stderr().is_terminal());
                }
                Item::ColorDiagnostics(value_os) if value_os.as_encoded_bytes() == b"auto" => {
                    mold_common::error::set_color(std::io::stderr().is_terminal());
                }
                Item::ColorDiagnostics(value_os) if value_os.as_encoded_bytes() == b"always" => {
                    mold_common::error::set_color(true);
                }
                Item::ColorDiagnostics(value_os) if value_os.as_encoded_bytes() == b"never" => {
                    mold_common::error::set_color(false);
                }
                Item::ColorDiagnostics(value_os) => {
                    fatal!(
                        "unknown command line option: --color-diagnostics={}",
                        value_os.to_string_lossy()
                    );
                }
                Item::NoColorDiagnostics => {
                    mold_common::error::set_color(false);
                }
                Item::WarnCommon => {
                    let value = true;
                    a.warn_common = value;
                }
                Item::NoWarnCommon => {
                    let value = false;
                    a.warn_common = value;
                }
                Item::IgnoredWarnOnce => {
                    // Ignored for GNU ld compatibility, as in C++ mold.
                }
                Item::WarnSharedTextrel => {
                    warn_shared_textrel = true;
                }
                Item::WarnTextrel => {
                    a.warn_textrel = true;
                }
                Item::EnableNewDtags => {
                    let value = true;
                    a.enable_new_dtags = value;
                }
                Item::DisableNewDtags => {
                    let value = false;
                    a.enable_new_dtags = value;
                }
                Item::ExecuteOnly => {
                    a.execute_only = true;
                }
                Item::ZeroToBss => {
                    a.zero_to_bss = true;
                }
                Item::CompressDebugSections(value_os) => {
                    let arg = utf8_arg(value_os, "--compress-debug-sections");
                    a.compress_debug_sections = match arg {
                        "zlib" | "zlib-gabi" => DebugCompression::Zlib(1),
                        "zstd" => DebugCompression::Zstd(3),
                        "none" => DebugCompression::None,
                        s if s.starts_with("zlib:") => {
                            let level = parse_number("compress-debug-sections", &s[5..]);
                            if !(0..=9).contains(&level) {
                                fatal!(
                                    "invalid --compress-debug-sections argument: {arg} (zlib level must be between 0 and 9)"
                                );
                            }
                            DebugCompression::Zlib(level as i32)
                        }
                        s if s.starts_with("zstd:") => {
                            let level = parse_number("compress-debug-sections", &s[5..]);
                            if !(1..=22).contains(&level) {
                                fatal!(
                                    "invalid --compress-debug-sections argument: {arg} (zstd level must be between 1 and 22)"
                                );
                            }
                            DebugCompression::Zstd(level as i32)
                        }
                        _ => fatal!("invalid --compress-debug-sections argument: {arg}"),
                    };
                }
                Item::Wrap(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.wrap.insert(raw_arg.as_encoded_bytes().to_vec());
                }
                Item::Omagic | Item::OmagicShort => {
                    a.omagic = true;
                    rctx.is_static = true;
                }
                Item::NoOmagic => {
                    a.omagic = false;
                }
                Item::Oformat(value_os) => {
                    let arg = utf8_arg(value_os, "--oformat");
                    if arg != "binary" {
                        fatal!("--oformat: {arg} is not supported");
                    }
                    a.oformat_binary = true;
                }
                Item::RetainSymbolsFile(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.retain_symbols_file =
                        Some(read_retain_symbols_file(&a.chroot, Path::new(raw_arg)));
                }
                Item::SectionAlign(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    let arg = raw_arg.as_encoded_bytes();
                    let Some((name, value)) =
                        arg.split_once_str(b"=").filter(|(_, v)| !v.is_empty())
                    else {
                        fatal!("--section-align: syntax error: {}", arg.as_bstr());
                    };
                    let value = std::str::from_utf8(value).unwrap_or_else(|_| {
                        fatal!("--section-align: invalid number: {}", value.as_bstr())
                    });
                    let value = parse_number("section-align", value);
                    if value <= 0 || !(value as u64).is_power_of_two() {
                        fatal!("--section-align={}: value must be a power of 2", arg.as_bstr());
                    }
                    a.section_align.insert(name.to_vec(), value as u64);
                }
                Item::SectionStart(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    let arg = raw_arg.as_encoded_bytes();
                    let Some((name, value)) =
                        arg.split_once_str(b"=").filter(|(_, v)| !v.is_empty())
                    else {
                        fatal!("--section-start: syntax error: {}", arg.as_bstr());
                    };
                    let value = std::str::from_utf8(value).unwrap_or_else(|_| {
                        fatal!("--section-start: invalid number: {}", value.as_bstr())
                    });
                    a.section_start.insert(name.to_vec(), parse_hex("section-start", value));
                }
                Item::SectionOrder(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.section_order = parse_section_order(raw_arg.as_encoded_bytes());
                }
                Item::Tbss(value_os) => {
                    let arg = utf8_arg(value_os, "--Tbss");
                    a.section_start.insert(b".bss".to_vec(), parse_hex("Tbss", arg));
                }
                Item::Tdata(value_os) => {
                    let arg = utf8_arg(value_os, "--Tdata");
                    a.section_start.insert(b".data".to_vec(), parse_hex("Tdata", arg));
                }
                Item::Ttext(value_os) => {
                    let arg = utf8_arg(value_os, "--Ttext");
                    a.section_start.insert(b".text".to_vec(), parse_hex("Ttext", arg));
                }
                Item::TtextSegment(value_os) => {
                    let arg = utf8_arg(value_os, "--Ttext-segment");
                    a.ttext_segment = Some(parse_number("Ttext-segment", arg) as u64);
                }
                Item::Repro => {
                    a.repro = true;
                }
                Item::NoUndefined => {
                    report_undefined = Some(true);
                }
                Item::SeparateDebugFile(value_os) if value_os.as_encoded_bytes() == b"\0" => {
                    separate_debug_file = Some(PathBuf::new());
                }
                Item::SeparateDebugFile(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    separate_debug_file = Some(PathBuf::from(raw_arg));
                }
                Item::NoSeparateDebugFile => {
                    separate_debug_file = None;
                }
                Item::Nmagic | Item::NmagicShort => {
                    let value = true;
                    a.nmagic = value;
                }
                Item::NoNmagic => {
                    let value = false;
                    a.nmagic = value;
                }
                Item::FatalWarnings => {
                    mold_common::error::set_fatal_warnings(true);
                }
                Item::NoFatalWarnings => {
                    mold_common::error::set_fatal_warnings(false);
                }
                Item::NoWarningsShort | Item::NoWarnings => {
                    mold_common::error::set_suppress_warnings(true);
                }
                Item::Fork => {
                    let value = true;
                    a.fork = value;
                }
                Item::NoFork => {
                    let value = false;
                    a.fork = value;
                }
                Item::GcSections => {
                    let value = true;
                    a.gc_sections = value;
                }
                Item::NoGcSections => {
                    let value = false;
                    a.gc_sections = value;
                }
                Item::PrintGcSections(value_os) if value_os.as_encoded_bytes() == b"\0" => {
                    a.print_gc_sections = Some(ReportOutput::Stdout);
                }
                Item::PrintGcSections(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.print_gc_sections = parse_report_output(raw_arg);
                }
                Item::NoPrintGcSections => {
                    a.print_gc_sections = None;
                }
                Item::DiscardSection(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.discard_section.insert(raw_arg.as_encoded_bytes().to_vec());
                }
                Item::NoDiscardSection(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.discard_section.remove(raw_arg.as_encoded_bytes());
                }
                Item::Icf(value_os) => {
                    let arg = utf8_arg(value_os, "--icf");
                    match arg {
                        "all" => {
                            a.icf = true;
                            a.icf_all = true;
                        }
                        "safe" => a.icf = true,
                        "none" => a.icf = false,
                        _ => fatal!("unknown --icf argument: {arg}"),
                    }
                }
                Item::NoIcf => {
                    a.icf = false;
                }
                Item::IgnoreDataAddressEquality => {
                    a.ignore_data_address_equality = true;
                }
                Item::ImageBase(value_os) => {
                    let arg = utf8_arg(value_os, "--image-base");
                    a.image_base = parse_number("image-base", arg) as u64;
                }
                Item::PhysicalImageBase(value_os) => {
                    let arg = utf8_arg(value_os, "--physical-image-base");
                    a.physical_image_base = Some(parse_number("physical-image-base", arg) as u64);
                }
                Item::PrintIcfSections(value_os) if value_os.as_encoded_bytes() == b"\0" => {
                    a.print_icf_sections = Some(ReportOutput::Stdout);
                }
                Item::PrintIcfSections(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.print_icf_sections = parse_report_output(raw_arg);
                }
                Item::NoPrintIcfSections => {
                    a.print_icf_sections = None;
                }
                Item::QuickExit => {
                    let value = true;
                    a.quick_exit = value;
                }
                Item::NoQuickExit => {
                    let value = false;
                    a.quick_exit = value;
                }
                Item::Plugin(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.plugin = PathBuf::from(raw_arg);
                }
                Item::PluginOpt(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.plugin_opt.push(raw_arg.as_encoded_bytes().to_vec());
                }
                Item::LtoCsProfileGenerate => {
                    let option = b"cs-profile-generate".to_vec();
                    a.plugin_opt.push(option);
                }
                Item::LtoDebugPassManager => {
                    let option = b"debug-pass-manager".to_vec();
                    a.plugin_opt.push(option);
                }
                Item::DisableVerify => {
                    let option = b"disable-verify".to_vec();
                    a.plugin_opt.push(option);
                }
                Item::LtoEmitAsm => {
                    let option = b"emit-asm".to_vec();
                    a.plugin_opt.push(option);
                }
                Item::NoLegacyPassManager => {
                    let option = b"legacy-pass-manager".to_vec();
                    a.plugin_opt.push(option);
                }
                Item::NoLtoLegacyPassManager => {
                    let option = b"new-pass-manager".to_vec();
                    a.plugin_opt.push(option);
                }
                Item::OptRemarksWithHotness => {
                    let option = b"opt-remarks-with-hotness".to_vec();
                    a.plugin_opt.push(option);
                }
                Item::LtoPseudoProbeForProfiling => {
                    let option = b"pseudo-probe-for-profiling".to_vec();
                    a.plugin_opt.push(option);
                }
                Item::SaveTemps => {
                    let option = b"save-temps".to_vec();
                    a.plugin_opt.push(option);
                }
                Item::ThinltoEmitImportsFiles => {
                    let option = b"thinlto-emit-imports-files".to_vec();
                    a.plugin_opt.push(option);
                }
                Item::ThinltoIndexOnly(value_os) if value_os.as_encoded_bytes() == b"\0" => {
                    let option = b"thinlto-index-only".to_vec();
                    a.plugin_opt.push(option);
                }
                Item::ThinltoIndexOnly(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    let option =
                        [b"thinlto-index-only=".as_slice(), raw_arg.as_encoded_bytes()].concat();
                    a.plugin_opt.push(option);
                }
                Item::LtoCsProfileFile(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    let option =
                        [b"cs-profile-path=".as_slice(), raw_arg.as_encoded_bytes()].concat();
                    a.plugin_opt.push(option);
                }
                Item::LtoPartitions(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    let option =
                        [b"lto-partitions=".as_slice(), raw_arg.as_encoded_bytes()].concat();
                    a.plugin_opt.push(option);
                }
                Item::LtoObjPath(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    let option = [b"obj-path=".as_slice(), raw_arg.as_encoded_bytes()].concat();
                    a.plugin_opt.push(option);
                }
                Item::OptRemarksFilename(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    let option =
                        [b"opt-remarks-filename=".as_slice(), raw_arg.as_encoded_bytes()].concat();
                    a.plugin_opt.push(option);
                }
                Item::OptRemarksFormat(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    let option =
                        [b"opt-remarks-format=".as_slice(), raw_arg.as_encoded_bytes()].concat();
                    a.plugin_opt.push(option);
                }
                Item::OptRemarksHotnessThreshold(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    let option =
                        [b"opt-remarks-hotness-threshold=".as_slice(), raw_arg.as_encoded_bytes()]
                            .concat();
                    a.plugin_opt.push(option);
                }
                Item::OptRemarksPasses(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    let option =
                        [b"opt-remarks-passes=".as_slice(), raw_arg.as_encoded_bytes()].concat();
                    a.plugin_opt.push(option);
                }
                Item::LtoSampleProfile(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    let option =
                        [b"sample-profile=".as_slice(), raw_arg.as_encoded_bytes()].concat();
                    a.plugin_opt.push(option);
                }
                Item::ThinltoObjectSuffixReplace(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    let option =
                        [b"thinlto-object-suffix-replace=".as_slice(), raw_arg.as_encoded_bytes()]
                            .concat();
                    a.plugin_opt.push(option);
                }
                Item::ThinltoPrefixReplace(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    let option =
                        [b"thinlto-prefix-replace=".as_slice(), raw_arg.as_encoded_bytes()]
                            .concat();
                    a.plugin_opt.push(option);
                }
                Item::ThinltoCacheDir(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    let option = [b"cache-dir=".as_slice(), raw_arg.as_encoded_bytes()].concat();
                    a.plugin_opt.push(option);
                }
                Item::ThinltoCachePolicy(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    let option = [b"cache-policy=".as_slice(), raw_arg.as_encoded_bytes()].concat();
                    a.plugin_opt.push(option);
                }
                Item::ThinltoJobs(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    let option = [b"jobs=".as_slice(), raw_arg.as_encoded_bytes()].concat();
                    a.plugin_opt.push(option);
                }
                Item::ThreadCount(value_os) => {
                    let arg = utf8_arg(value_os, "--thread-count");
                    a.thread_count = Some(parse_number("thread-count", arg).max(1) as usize);
                }
                Item::Threads(value_os) if value_os.as_encoded_bytes() == b"\0" => {
                    a.thread_count = None;
                }
                Item::Threads(value_os) => {
                    let arg = utf8_arg(value_os, "--threads");
                    a.thread_count = Some(parse_number("threads", arg).max(1) as usize);
                }
                Item::NoThreads => {
                    a.thread_count = Some(1);
                }
                Item::DiscardAll | Item::DiscardAllShort => {
                    a.discard_all = true;
                }
                Item::DiscardLocals | Item::DiscardLocalsShort => {
                    a.discard_locals = true;
                }
                Item::DiscardNone => {
                    a.discard_all = false;
                    a.discard_locals = false;
                }
                Item::StripAll | Item::StripAllShort => {
                    a.strip_all = true;
                }
                Item::StripDebug | Item::StripDebugShort => {
                    a.strip_debug = true;
                }
                Item::WarnUnresolvedSymbols => {
                    error_unresolved_symbols = false;
                }
                Item::ErrorUnresolvedSymbols => {
                    error_unresolved_symbols = true;
                }
                Item::Rpath(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    add_rpath(&mut a, &mut rpaths, raw_arg);
                }
                Item::ShortR(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    if Path::new(raw_arg).metadata().is_ok_and(|m| !m.is_dir()) {
                        fatal!(
                            "-R{}: -R as an alias for --just-symbols is not supported",
                            raw_arg.to_string_lossy()
                        );
                    }
                    add_rpath(&mut a, &mut rpaths, raw_arg);
                }
                Item::UndefinedVersion => {
                    let value = true;
                    a.undefined_version = value;
                }
                Item::NoUndefinedVersion => {
                    let value = false;
                    a.undefined_version = value;
                }
                Item::Undefined(value_os) | Item::UndefinedShort(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.undefined.push(raw_arg.as_encoded_bytes().to_vec());
                }
                Item::BuildId(value_os) if value_os.as_encoded_bytes() == b"\0" => {
                    a.build_id = BuildId::Hash(20);
                }
                Item::BuildId(value_os) => {
                    let arg = utf8_arg(value_os, "--build-id");
                    a.build_id = match arg {
                        "none" => BuildId::None,
                        "uuid" => BuildId::Uuid,
                        "md5" => BuildId::Hash(16),
                        "sha1" => BuildId::Hash(20),
                        "sha256" | "fast" => BuildId::Hash(32),
                        s if s.starts_with("0x") || s.starts_with("0X") => {
                            BuildId::Hex(parse_hex_build_id(s))
                        }
                        _ => fatal!("invalid --build-id argument: {arg}"),
                    };
                }
                Item::NoBuildId => {
                    a.build_id = BuildId::None;
                }
                Item::Be8 => {
                    let value = true;
                    be8 = value;
                }
                Item::Be32 => {
                    let value = false;
                    be8 = value;
                }
                Item::Format(value_os) | Item::FormatShort(value_os) => {
                    let arg = utf8_arg(value_os, "--format");
                    if arg == "binary" {
                        fatal!(
                            "mold does not support `-b binary`. If you want to convert a binary file into an \
                                 object file, use `objcopy -I binary -O default <input-file> <output-file.o>` instead."
                        );
                    }
                    fatal!("unknown command line option: -b {arg}");
                }
                Item::IgnoredFuseLd(_) => {}
                Item::AllowShlibUndefined => {
                    allow_shlib_undefined = Some(true);
                }
                Item::NoAllowShlibUndefined => {
                    allow_shlib_undefined = Some(false);
                }
                Item::IgnoredShortO(_) => {
                    // Ignored for compatibility.
                }
                Item::IgnoredEB
                | Item::IgnoredEL
                | Item::IgnoredO0
                | Item::IgnoredO1
                | Item::IgnoredO2
                | Item::IgnoredVerbose(_)
                | Item::IgnoredSplitByFile(_)
                | Item::IgnoredSplitByReloc(_)
                | Item::IgnoredOrphanHandling(_)
                | Item::IgnoredNoStats
                | Item::IgnoredStartGroup
                | Item::IgnoredEndGroup
                | Item::IgnoredOpenParen
                | Item::IgnoredCloseParen
                | Item::IgnoredNostdlib
                | Item::IgnoredNoAddNeeded
                | Item::IgnoredNoCallGraphProfileSort
                | Item::IgnoredNoCopyDtNeededEntries => {
                    // Ignored for compatibility.
                }
                Item::IgnoredSortSection(_) => {
                    // Ignored for compatibility.
                }
                Item::IgnoredSortCommon(_)
                | Item::IgnoredDc
                | Item::IgnoredDp
                | Item::IgnoredFixCortexA53835769
                | Item::IgnoredFixCortexA53843419(_)
                | Item::IgnoredNodefaultlibs
                | Item::IgnoredWarnConstructors
                | Item::IgnoredWarnExecstack
                | Item::IgnoredNoWarnExecstack
                | Item::IgnoredNoErrorExecstack
                | Item::IgnoredNoWarnRwxSegments
                | Item::IgnoredNoErrorRwxSegments
                | Item::IgnoredLongPlt
                | Item::IgnoredSecurePlt => {
                    // Ignored for compatibility.
                }
                Item::IgnoredRpathLink(_) => {
                    // Ignored for compatibility.
                }
                Item::IgnoredNoKeepMemory => {
                    // Ignored for compatibility.
                }
                Item::IgnoredMaxCacheSize(_) => {
                    // Ignored for compatibility.
                }
                Item::IgnoredMmapOutputFile | Item::IgnoredNoMmapOutputFile => {
                    // Ignored for compatibility.
                }
                Item::IgnoredA(value_os) => {
                    // GNU ld accepts only the HP/UX keywords below, and
                    // rejects any other value, including one attached with
                    // '=' ("-a=shared" would abbreviate several long
                    // options, so getopt reports it as unrecognized).
                    let value = utf8_arg(value_os, "-a");
                    if !matches!(value, "archive" | "shared" | "default") {
                        fatal!("unrecognized -a option `{value}'");
                    }
                }
                Item::IgnoredAssert(value_os) => {
                    // Likewise, GNU ld knows only these SunOS keywords.
                    let value = utf8_arg(value_os, "-assert");
                    if !matches!(
                        value,
                        "definitions" | "nodefinitions" | "nosymbolic" | "pure-text"
                    ) {
                        fatal!("unrecognized -assert option `{value}'");
                    }
                }
                Item::IgnoredG
                | Item::IgnoredD
                | Item::IgnoredArchitecture(_)
                | Item::IgnoredGpsize(_)
                | Item::IgnoredQy
                | Item::IgnoredY(_)
                | Item::IgnoredMriScript(_)
                | Item::IgnoredDefaultScript(_) => {
                    // Ignored for compatibility.
                }
                Item::IgnoredPrintMapDiscarded
                | Item::IgnoredNoPrintMapDiscarded
                | Item::IgnoredPrintMapLocals
                | Item::IgnoredNoPrintMapLocals
                | Item::IgnoredStripDiscarded
                | Item::IgnoredNoStripDiscarded
                | Item::IgnoredMapWholeFiles
                | Item::IgnoredNoMapWholeFiles
                | Item::IgnoredCref
                | Item::IgnoredPrintMemoryUsage
                | Item::IgnoredPrintSysroot
                | Item::IgnoredPrintOutputFormat
                | Item::IgnoredTargetHelp
                | Item::IgnoredForceExeSuffix
                | Item::IgnoredTraditionalFormat
                | Item::IgnoredQmagic
                | Item::IgnoredReduceMemoryOverheads
                | Item::IgnoredHashSize(_)
                | Item::IgnoredRemapInputs(_)
                | Item::IgnoredRemapInputsFile(_)
                | Item::IgnoredErrorHandlingScript(_)
                | Item::IgnoredVersionExportsSection(_)
                | Item::IgnoredAcceptUnknownInputArch
                | Item::IgnoredNoAcceptUnknownInputArch
                | Item::IgnoredNoWarnMismatch
                | Item::IgnoredNoWarnSearchMismatch
                | Item::IgnoredForceGroupAllocation
                | Item::IgnoredEnableNonContiguousRegions
                | Item::IgnoredEnableNonContiguousRegionsWarnings
                | Item::IgnoredDisableLinkerVersion
                | Item::IgnoredEnableLinkerVersion
                | Item::IgnoredNoEnumSizeWarning
                | Item::IgnoredNoWcharSizeWarning
                | Item::IgnoredDefaultImportedSymver
                | Item::IgnoredWarnExecstackObjects
                | Item::IgnoredWarnSectionAlign
                | Item::IgnoredWarnMultipleGp
                | Item::IgnoredWarnAlternateEm
                | Item::IgnoredErrorExecstack
                | Item::IgnoredWarnRwxSegments
                | Item::IgnoredErrorRwxSegments
                | Item::IgnoredNoDefineCommon
                | Item::IgnoredDynamicListCppNew
                | Item::IgnoredDynamicListCppTypeinfo
                | Item::IgnoredCheckSections
                | Item::IgnoredNoCheckSections => {
                    // Ignored for compatibility.
                }
                Item::ShortMLower(value_os) => {
                    let arg = utf8_arg(value_os, "-m");
                    match emulation_to_target(arg) {
                        Some(name) => a.emulation = name,
                        None => fatal!("unknown -m argument: {arg}"),
                    }
                }
                Item::Filter(value_os) | Item::FilterShort(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.filter.push(raw_arg.as_encoded_bytes().to_vec());
                }
                Item::Auxiliary(value_os) | Item::AuxiliaryShort(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.auxiliary.push(raw_arg.as_encoded_bytes().to_vec());
                }
                Item::VersionScript(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.version_scripts.push(PathBuf::from(raw_arg));
                }
                Item::DynamicList(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.bsymbolic = BsymbolicKind::All;
                    let mf = mold_common::mapped_file::must_open_file(&a.chroot, raw_arg);
                    a.dynamic_list.push(DynamicListSource::File(mf));
                }
                Item::DynamicListData => {
                    a.dynamic_list_data = true;
                }
                Item::ExportDynamicSymbol(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.dynamic_list
                        .push(DynamicListSource::Pattern(raw_arg.as_encoded_bytes().to_vec()));
                }
                Item::ExportDynamicSymbolList(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    let mf = mold_common::mapped_file::must_open_file(&a.chroot, raw_arg);
                    a.dynamic_list.push(DynamicListSource::File(mf));
                }
                Item::Entry(value_os) | Item::EntryShort(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    a.entry = raw_arg.as_encoded_bytes().to_vec();
                }
                Item::AsNeeded => {
                    let value = true;
                    rctx.as_needed = value;
                }
                Item::NoAsNeeded => {
                    let value = false;
                    rctx.as_needed = value;
                }
                Item::WholeArchive => {
                    let value = true;
                    rctx.whole_archive = value;
                }
                Item::NoWholeArchive => {
                    let value = false;
                    rctx.whole_archive = value;
                }
                Item::Library(value_os) | Item::LibraryShort(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    if visited_libs.insert(raw_arg) {
                        let mut job = ReaderJob {
                            rctx: rctx.clone(),
                            name: PathBuf::from(raw_arg),
                            is_lib: true,
                        };
                        job.rctx.pos = vec![jobs.len() as u32];
                        jobs.push(job);
                    }
                }
                Item::Script(value_os) | Item::ScriptShort(value_os) => {
                    let raw_arg: &OsStr = value_os;
                    let mut job = ReaderJob {
                        rctx: rctx.clone(),
                        name: PathBuf::from(raw_arg),
                        ..Default::default()
                    };
                    job.rctx.pos = vec![jobs.len() as u32];
                    jobs.push(job);
                }
                Item::PushState => {
                    rctx_stack.push(rctx.clone());
                }
                Item::PopState => {
                    rctx = rctx_stack
                        .pop()
                        .unwrap_or_else(|| fatal!("no state pushed before popping"));
                }
                Item::ZNow => {
                    let value = true;
                    a.z_now = value;
                }
                Item::ZLazy => {
                    let value = false;
                    a.z_now = value;
                }
                Item::ZCetReportNone => {
                    a.z_cet_report = ReportKind::None;
                }
                Item::ZCetReportWarning => {
                    a.z_cet_report = ReportKind::Warning;
                }
                Item::ZCetReportError => {
                    a.z_cet_report = ReportKind::Error;
                }
                Item::ZExecstack => {
                    a.z_execstack = true;
                }
                Item::ZExecstackIfNeeded => {
                    a.z_execstack_if_needed = true;
                }
                Item::ZMaxPageSize(value_os) => {
                    let arg = utf8_arg(value_os, "-z max-page-size");
                    a.page_size = parse_number("-z max-page-size", arg) as u64;
                    if !a.page_size.is_power_of_two() {
                        fatal!("-z max-page-size {arg}: value must be a power of 2");
                    }
                }
                Item::ZStartStopVisibilityProtected => {
                    let value = true;
                    a.z_start_stop_visibility_protected = value;
                }
                Item::ZStartStopVisibilityHidden => {
                    let value = false;
                    a.z_start_stop_visibility_protected = value;
                }
                Item::ZNoexecstack => {
                    a.z_execstack = false;
                }
                Item::ZRelro => {
                    z_relro = Some(true);
                }
                Item::ZNorelro => {
                    z_relro = Some(false);
                }
                Item::ZUndefs => {
                    report_undefined = Some(false);
                }
                Item::ZNodlopen => {
                    a.z_dlopen = false;
                }
                Item::ZNodelete => {
                    a.z_delete = false;
                }
                Item::ZNocopyreloc => {
                    a.z_copyreloc = false;
                }
                Item::ZNodump => {
                    a.z_dump = false;
                }
                Item::ZInitfirst => {
                    a.z_initfirst = true;
                }
                Item::ZInterpose => {
                    a.z_interpose = true;
                }
                Item::ZIbt => {
                    a.z_ibt = true;
                }
                Item::ZIbtplt => {}
                Item::ZMuldefs => {
                    a.allow_multiple_definition = true;
                }
                Item::ZKeepTextSectionPrefix => {
                    let value = true;
                    a.z_keep_text_section_prefix = value;
                }
                Item::ZNokeepTextSectionPrefix => {
                    let value = false;
                    a.z_keep_text_section_prefix = value;
                }
                Item::ZShstk => {
                    a.z_shstk = true;
                }
                Item::ZText => {
                    a.z_text = true;
                }
                Item::ZNotext | Item::ZTextoff => {
                    a.z_text = false;
                }
                Item::ZOrigin => {
                    a.z_origin = true;
                }
                Item::ZNodefaultlib => {
                    a.z_nodefaultlib = true;
                }
                Item::ZSeparateLoadableSegments => {
                    z_separate_code = Some(SeparateCodeKind::SeparateLoadableSegments);
                }
                Item::ZSeparateCode => {
                    z_separate_code = Some(SeparateCodeKind::SeparateCode);
                }
                Item::ZNoseparateCode => {
                    z_separate_code = Some(SeparateCodeKind::NoSeparateCode);
                }
                Item::ZStackSize(value_os) => {
                    let arg = utf8_arg(value_os, "-z stack-size");
                    a.z_stack_size = parse_number("-z stack-size", arg) as u64;
                }
                Item::ZDynamicUndefinedWeak => {
                    z_dynamic_undefined_weak = Some(true);
                }
                Item::ZNodynamicUndefinedWeak => {
                    z_dynamic_undefined_weak = Some(false);
                }
                Item::ZSectionheader => {
                    let value = true;
                    a.z_sectionheader = value;
                }
                Item::ZNosectionheader => {
                    let value = false;
                    a.z_sectionheader = value;
                }
                Item::ZRodynamic => {
                    a.z_rodynamic = true;
                }
                Item::ZX8664V2 => {
                    a.z_x86_64_isa_level |= GNU_PROPERTY_X86_ISA_1_V2;
                }
                Item::ZX8664V3 => {
                    a.z_x86_64_isa_level |= GNU_PROPERTY_X86_ISA_1_V3;
                }
                Item::ZX8664V4 => {
                    a.z_x86_64_isa_level |= GNU_PROPERTY_X86_ISA_1_V4;
                }
                Item::ZRewriteEndbr => {
                    if !matches!(target.family, Family::X86_64 | Family::Arm64) {
                        fatal!("-z rewrite-endbr is supported only on x86-64 and arm64");
                    }
                    a.z_rewrite_endbr = true;
                }
                Item::ZNorewriteEndbr => {
                    a.z_rewrite_endbr = false;
                }
                Item::ZGlobal
                | Item::ZGlobalaudit
                | Item::ZLoadfltr
                | Item::ZStartStopGc
                | Item::ZNoStartStopGc
                | Item::ZUnique
                | Item::ZNounique
                | Item::ZUniqueSymbol
                | Item::ZNouniqueSymbol => {
                    // Ignored for compatibility.
                }
                Item::Grouped(value_os) => {
                    // A letter that takes a value ends the bundle, and GNU ld
                    // rejects the word, so it stays unknown.
                    if crate::cmdline_winnow::bundle_takes_value(value_os) {
                        fatal!("unknown command line option: {}", value_os.to_string_lossy());
                    }
                    warn!(
                        "grouped short command line options are deprecated: {}",
                        value_os.to_string_lossy()
                    );
                    // GNU ld reads -r and -i as a word of their own: in a
                    // group, with letters after them, it rejects the word.
                    let letters =
                        value_os.as_encoded_bytes().strip_prefix(b"-").unwrap_or_default();
                    if let Some((_, init)) = letters.split_last()
                        && (init.contains(&b'r') || init.contains(&b'i'))
                    {
                        fatal!("unrecognised option: {}", value_os.to_string_lossy());
                    }
                }
                Item::Unknown(value_os) => {
                    if let Some(level) = value_os.as_encoded_bytes().strip_prefix(b"--lto-O") {
                        a.plugin_opt.push([b"O", level].concat());
                    } else if value_os.as_os_str() == "-dynamic" {
                        fatal!(
                            "unknown command line option: -dynamic; -dynamic is a macOS linker's option. mold does not support macOS."
                        );
                    } else {
                        fatal!("unknown command line option: {}", value_os.to_string_lossy());
                    }
                }
                Item::Z(z) => {
                    if z.attached {
                        warn!("unknown command line option: -z{}", z.value.to_string_lossy());
                    } else {
                        warn!("unknown command line option: -z {}", z.value.to_string_lossy());
                    }
                }
            }
        }
    }

    if !a.chroot.as_os_str().is_empty() && !a.dependency_file.as_os_str().is_empty() {
        a.dependency_file =
            a.chroot.join(a.dependency_file.strip_prefix("/").unwrap_or(&a.dependency_file));
    }

    a.map = map_path.map(|mut path| {
        // Keep the raw spelling until after chroot: "-" names a file there,
        // while an empty path (including plain -M) still selects stdout.
        if !a.chroot.as_os_str().is_empty() && !path.as_os_str().is_empty() {
            path = a.chroot.join(path.strip_prefix("/").unwrap_or(&path));
        }
        parse_report_output(path.as_os_str()).unwrap_or(ReportOutput::Stdout)
    });

    if !directory.as_os_str().is_empty() {
        a.orig_cwd = std::env::current_dir().ok();
        if let Err(e) = std::env::set_current_dir(&directory) {
            fatal!("chdir failed: {}: {}", directory.display(), strerror(&e));
        }
    }

    if !a.sysroot.as_os_str().is_empty() {
        for path in &mut a.library_paths {
            let bytes = path.as_os_str().as_encoded_bytes();
            if let Some(rest) = bytes.strip_prefix(b"=").or_else(|| bytes.strip_prefix(b"$SYSROOT"))
            {
                let mut full = a.sysroot.as_os_str().to_os_string();
                full.push(os_str(rest));
                *path = PathBuf::from(full);
            }
        }
    }

    // Clean library paths by removing redundant `/..` and `/.` so that
    // they are easier to read in log messages.
    for path in &mut a.library_paths {
        *path = clean_path(path);
    }

    if a.shared {
        a.pic = true;
    }

    if let Some(val) = a.ttext_segment {
        if val % a.page_size != 0 {
            warn!("-Ttext-segment is not a multiple of page size: {val}");
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
        if error_unresolved_symbols { UnresolvedKind::Error } else { UnresolvedKind::Warn }
    } else {
        UnresolvedKind::Ignore
    };

    if a.retain_symbols_file.is_some() {
        a.strip_all = false;
        a.discard_all = false;
    }

    if let ShuffleSections::Shuffle(seed) = &mut a.shuffle_sections {
        *seed = shuffle_sections_seed.unwrap_or_else(|| {
            let mut buf = [0u8; 8];
            getrandom::fill(&mut buf)
                .unwrap_or_else(|err| fatal!("cannot get random bytes: {err}"));
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

    // `-z force-bti` implies `-z bti-report=warning`, and `-z gcs=always`
    // implies `-z gcs-report=warning` and `-z gcs-report-dynamic=warning`.
    a.z_bti_report =
        z_bti_report.unwrap_or(if a.z_force_bti { ReportKind::Warning } else { ReportKind::None });
    let gcs_report =
        if a.z_gcs == GcsKind::Always { ReportKind::Warning } else { ReportKind::None };
    a.z_gcs_report = z_gcs_report.unwrap_or(gcs_report);
    a.z_gcs_report_dynamic = z_gcs_report_dynamic.unwrap_or(gcs_report);

    // --section-order implies `-z norelro`
    a.z_relro = z_relro.unwrap_or(a.section_order.is_empty());
    if a.nmagic || a.omagic {
        a.z_relro = false;
    }

    if !a.shared {
        if !a.filter.is_empty() {
            fatal!("-filter may not be used without -shared");
        }
        if !a.auxiliary.is_empty() {
            fatal!("-auxiliary may not be used without -shared");
        }
    }

    // Even though SH4 is RELA, addends in its relocation records are always
    // zero, and actual addends are written to relocated places. So we need
    // to handle it as an exception.
    if (!target.is_rela || target.family == Family::Sh4) && !a.apply_dynamic_relocs {
        fatal!("--no-apply-dynamic-relocs may not be used on {}", target.name);
    }
    if target.family == Family::Sparc64 && a.apply_dynamic_relocs {
        fatal!("--apply-dynamic-relocs may not be used on SPARC64");
    }

    if !a.section_start.is_empty() && !a.section_order.is_empty() {
        fatal!("--section-start may not be used with --section-order");
    }
    if a.strip_all && a.emit_relocs && !a.relocatable {
        fatal!("--strip-all may not be used with --emit-relocs");
    }
    if a.image_base % a.page_size != 0 {
        fatal!("-image-base must be a multiple of -max-page-size");
    }
    if a.emulation == "arm32be" && !be8 && !a.relocatable {
        fatal!("--be32 is not supported");
    }

    if std::env::var_os("MOLD_REPRO").is_some_and(|v| !v.is_empty()) {
        a.repro = true;
    }

    if a.default_symver {
        let ver = if a.soname.is_empty() {
            a.output.file_name().unwrap_or_default().as_encoded_bytes().to_vec()
        } else {
            a.soname.as_encoded_bytes().to_vec()
        };
        a.version_definitions.push(ver.into());
    }

    if let Some(file) = separate_debug_file {
        a.separate_debug_file = if file.as_os_str().is_empty() {
            let mut name = a.output.as_os_str().to_os_string();
            name.push(".dbg");
            PathBuf::from(name)
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
    // header, there's no way to identify the location of a symbol
    // table in an output file in the first place.
    if a.oformat_binary {
        a.strip_all = true;
    }

    // By default, mold tries to overwrite an output file if it exists
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

    // We don't garbage-collect sections or remove symbols when creating a
    // relocatable output. -s strips only debug info there.
    if a.relocatable {
        a.gc_sections = false;
        a.discard_all = false;
        a.discard_locals = false;
        a.strip_debug |= a.strip_all;
        a.strip_all = false;
    }

    a.undefined_glob = undefined_glob.build();
    a.unique = unique.build();
    ParsedArgs { args: a, jobs }
}

fn add_rpath<'a>(a: &mut Args, seen: &mut HashSet<&'a OsStr>, path: &'a OsStr) {
    if seen.insert(path) {
        if !a.rpaths.is_empty() {
            a.rpaths.push(":");
        }
        a.rpaths.push(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_preserves_gnu_option_boundaries_and_values() {
        let args: Vec<_> = [
            "mold",
            "-output",
            "--output=next",
            "-z",
            "now",
            "-zmax-page-size=4096",
            "--gc-sections",
            "--no-gc-sections",
            "--lto-cs-profile-file=data",
            "--save-temps",
            "--lto-O2",
        ]
        .into_iter()
        .map(|s| Cow::Borrowed(OsStr::new(s)))
        .collect();
        let mut cursor = ArgCursor { args: &args, index: 1 };
        assert_eq!(cursor.read_arg("--output"), None);
        assert_eq!(cursor.index, 1);
        assert_eq!(cursor.read_arg("o"), Some(OsStr::new("utput")));
        assert_eq!(cursor.read_eq("--output"), Some(OsStr::new("next")));
        assert!(!cursor.read_z_flag("lazy"));
        assert!(cursor.read_z_flag("now"));
        assert_eq!(cursor.read_z_arg("max-page-size"), Some("4096"));
        assert_eq!(cursor.read_switch("gc-sections", "no-gc-sections"), Some(true));
        assert_eq!(cursor.read_switch("gc-sections", "no-gc-sections"), Some(false));
        assert_eq!(cursor.read_lto_option(), Some(b"cs-profile-path=data".to_vec()));
        assert_eq!(cursor.read_lto_option(), Some(b"save-temps".to_vec()));
        assert_eq!(cursor.read_lto_option(), Some(b"O2".to_vec()));
        assert_eq!(cursor.index, args.len());
    }

    #[cfg(unix)]
    #[test]
    fn cursor_borrows_non_utf8_separate_and_attached_values() {
        let args: Vec<_> = [b"mold".as_slice(), b"-o", b"out-\xff", b"--plugin-opt=arg-\xfe"]
            .into_iter()
            .map(|s| Cow::Borrowed(os_str(s)))
            .collect();
        let mut cursor = ArgCursor { args: &args, index: 1 };
        assert_eq!(cursor.read_arg("o").unwrap().as_encoded_bytes(), b"out-\xff");
        assert_eq!(cursor.read_arg("plugin-opt").unwrap().as_encoded_bytes(), b"arg-\xfe");
        assert_eq!(cursor.index, args.len());
    }

    #[test]
    fn numeric_options_use_c_integer_syntax() {
        for (text, expected) in [
            ("0", 0),
            ("32", 32),
            ("040", 32),
            ("0x20", 32),
            ("+0X20", 32),
            ("+040", 32),
            (" \t\n\r\x0b\x0c+040", 32),
            (" -0x20", -32),
            ("- 040", -32),
            ("--1", 1),
            ("18446744073709551615", -1),
            ("-9223372036854775808", i64::MIN),
        ] {
            assert_eq!(parse_number("test", text), expected, "{text:?}");
        }
        for text in ["", " ", "+", "08", "0x", "0x+1", "1 ", "1x", "18446744073709551616"] {
            assert_eq!(parse_c_number(text), None, "{text:?}");
        }
    }
}
