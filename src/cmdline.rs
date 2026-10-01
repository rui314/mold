//! Command line parsing.
//!
//! The command line is compatible with Apple's ld64: options are single-dash
//! long names, and input files and `-l` options are position-dependent.

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::IsTerminal;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use crate::error::{Held, raw};
use crate::fatal;
use crate::filetype::{FileType, get_file_type};
use crate::input_files::PlatformVersion;
use crate::macho::*;
use crate::mapped_file::MappedFile;
use crate::util::glob::{Glob, GlobBuilder};
use crate::util::{display, os_str, page_align};

/// The Apple ld64 version whose command line this linker implements,
/// reported by -version_details. Xcode passes flags according to this
/// number (Xcode 26.6 ships ld-1267).
pub const LD64_COMPAT_VERSION: &str = "1267";

/// The -v banner, which ld-prime also writes as its version in the
/// -dependency_info file.
pub const VERSION_BANNER: &str =
    concat!("mold-macho ", env!("CARGO_PKG_VERSION"), " (compatible with Apple ld64)");

/// Prints the -v banner on stderr, where ld-prime prints its own.
pub fn print_version() {
    eprintln!("{VERSION_BANNER}");
}

/// Prints the -version_details JSON on stdout, as ld-prime does.
pub fn print_version_details() {
    println!(
        "{{\n\t\"version\": \"{LD64_COMPAT_VERSION}\",\n\t\"architectures\": [\n\t\t\"arm64\",\n\t\t\"x86_64\"\n\t]\n}}"
    );
}

/// An input in command line order. Paths keep the bytes they were given
/// in; library and framework names are OS strings, since they become
/// path components.
#[derive(Clone, Debug)]
pub enum InputArg {
    /// A file path.
    File(PathBuf),
    /// A file path a -filelist gives, which ld-prime takes as it is,
    /// an archive's too (see passes::find_input).
    Listed(PathBuf),
    /// A library option: what it makes of the library, and how it
    /// names it.
    Library(LibraryKind, LibraryName),
    /// `-bundle_loader path`: the executable a bundle's undefined
    /// symbols may resolve to, bound at run time as the main executable.
    /// A file of another kind is an input like any other.
    BundleLoader(PathBuf),
}

/// How a library option names its library: `-lfoo` and the like, by a
/// name to look up in the library paths; `-framework Foo` and the
/// like, in the framework paths; `-weak_library path` and the like, by
/// its path.
#[derive(Clone, Debug)]
pub enum LibraryName {
    Lib(OsString),
    Framework(OsString),
    Path(PathBuf),
}

/// What a library option makes of the library it names. The options
/// of each kind name a library in each of the three ways (see
/// LibraryName), but for those that say otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibraryKind {
    /// -l, -framework: a library like any other. ld-prime also takes
    /// a path ending in .a on the command line for one, looked up as a
    /// -force_load path is, under a -syslibroot first, and not found as
    /// a library.
    Plain,
    /// -weak-l, -weak_framework, -weak_library: a dylib whose absence
    /// is tolerated at load time.
    Weak,
    /// -reexport-l, -reexport_framework, -reexport_library: a dylib
    /// whose exports this dylib re-exports as its own. -reexport-l looks
    /// for a dylib only.
    Reexport,
    /// -hidden-l, -hidden_framework, -load_hidden: an archive whose
    /// external symbols are demoted to private externals.
    Hidden,
    /// -needed-l, -needed_framework, -needed_library: always keep the
    /// dylib's load command.
    Needed,
    /// -upward-l, -upward_framework, -upward_library: a dylib that
    /// depends on this one in turn (LC_LOAD_UPWARD_DYLIB). -upward-l
    /// looks for a dylib only.
    Upward,
    /// -lazy-l, -lazy_framework, -lazy_library: a dylib dyld loads at
    /// the first use of one of its symbols, from macOS 27 on (see
    /// Args::lazy_load); before, a library like any other.
    Lazy,
    /// -no_merge-l, -no_merge_framework, -no_merge_library: a mergeable
    /// library this image re-exports, as Xcode's debug builds link what
    /// its release builds merge (-merge_*). They are -reexport-l,
    /// -reexport_framework and -reexport_library but for the hook
    /// ld-prime adds for the classes of such libraries (see
    /// Args::merged_libraries_hook), and as ld-prime spells them.
    NoMerge,
    /// -merge-l, -merge_framework, -merge_library: a library whose
    /// content goes into this image, a dylib that -make_mergeable made
    /// mergeable (LC_ATOM_INFO) linked as its objects would be, in place
    /// of a load command. Only a dylib can be found for one, never a
    /// stub; an object or archive the path names links as ever.
    Merge,
    /// -force-l, -force_load: an archive all of whose members are
    /// linked.
    Force,
    /// -possible-l, -possible_framework, -possible_library: a library
    /// as an auto-link option names one, a hint (see
    /// passes::load_autolink_deps).
    Possible,
    /// -assert-weak-l, -assert_weak_framework, -assert_weak_library: a
    /// dylib that loads weakly, all of whose imports must be weak
    /// already (see passes::check_weak_assertions).
    AssertWeak,
    /// -delay-l, -delay_framework, -delay_library: a dylib whose
    /// initializers run at the first use of one of its symbols (see
    /// delay_init::create_delay_init). -delay-l looks for a dylib only.
    Delay,
}

impl LibraryKind {
    /// The option of this kind that names a library as `name` does, as
    /// ld-prime spells it in diagnostics: with the name joined to it,
    /// or followed by a space.
    pub fn option(self, name: &LibraryName) -> &'static str {
        use LibraryKind::*;
        match name {
            LibraryName::Lib(_) => match self {
                Plain => "-l",
                Weak => "-weak-l",
                Reexport => "-reexport-l",
                Hidden => "-hidden-l",
                Needed => "-needed-l",
                Upward => "-upward-l",
                Lazy => "-lazy-l",
                NoMerge => "-no_merge-l",
                Merge => "-merge-l",
                Force => "-force-l",
                Possible => "-possible-l",
                AssertWeak => "-assert-weak-l",
                Delay => "-delay-l",
            },
            LibraryName::Framework(_) => match self {
                Plain => "-framework ",
                Weak => "-weak_framework ",
                Reexport => "-reexport_framework ",
                Needed => "-needed_framework ",
                Upward => "-upward_framework ",
                Lazy => "-lazy_framework ",
                NoMerge => "-no_merge_framework ",
                Merge => "-merge_framework ",
                Hidden => "-hidden_framework ",
                Possible => "-possible_framework ",
                AssertWeak => "-assert_weak_framework ",
                Delay => "-delay_framework ",
                Force => unreachable!(),
            },
            LibraryName::Path(_) => match self {
                Weak => "-weak_library ",
                Reexport => "-reexport_library ",
                Needed => "-needed_library ",
                Upward => "-upward_library ",
                Lazy => "-lazy_library ",
                NoMerge => "-no_merge_library ",
                Merge => "-merge_library ",
                Hidden => "-load_hidden ",
                Force => "-force_load ",
                Possible => "-possible_library ",
                AssertWeak => "-assert_weak_library ",
                Delay => "-delay_library ",
                Plain => "",
            },
        }
    }
}

impl LibraryName {
    /// The name or path as the option gives it.
    pub fn as_os_str(&self) -> &OsStr {
        match self {
            Self::Lib(name) | Self::Framework(name) => name,
            Self::Path(path) => path.as_os_str(),
        }
    }
}

/// -weak_reference_mismatches: an import referenced both weakly and not
/// is a strong one (the default), a weak one, or an error.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WeakRefMismatches {
    NonWeak,
    Weak,
    Error,
}

/// How an option says to treat what it is about (text relocations,
/// unaligned pointers, ...).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Treatment {
    Warning,
    Error,
    Suppress,
}

/// Reads a treatment as ld-prime reads them all: warning (or warn),
/// error, and where the option takes it, suppress.
fn parse_treatment(opt: &str, arg: &OsStr, suppress: bool) -> Treatment {
    match arg.as_bytes() {
        b"warning" | b"warn" => Treatment::Warning,
        b"error" => Treatment::Error,
        b"suppress" if suppress => Treatment::Suppress,
        _ if suppress => fatal!("{opt} invalid option (warning | error | suppress)"),
        _ => fatal!("{opt} invalid option (warning | error)"),
    }
}

/// -commons: what becomes of a tentative definition (a common symbol)
/// some dylib of the link defines: it wins (ignore_dylibs, the
/// default), the dylib's does (use_dylibs), or the link fails.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CommonsMode {
    IgnoreDylibs,
    UseDylibs,
    Error,
}

/// A list of symbols whose subsections move to another segment: one of
/// -move_to_rw_segment, -move_to_ro_segment or -dirty_data_list (see
/// symbol_moves).
#[derive(Debug)]
pub struct SymbolMove {
    /// The segment, as given: ld-prime's warnings name it so, and the
    /// output cuts it to 16 bytes.
    pub segment: Vec<u8>,
    /// The names listed, each found with value 1, and the patterns,
    /// with 0: ld-prime warns about a symbol it cannot move only if the
    /// list names it.
    pub symbols: Glob,
}

/// A section -sectcreate makes from a file, or -add_empty_section
/// empty. ld-prime makes each a file of its own, of one section, which
/// takes its place among the inputs.
#[derive(Debug)]
pub struct SectCreate {
    pub segname: Vec<u8>,
    pub sectname: Vec<u8>,
    /// The file of the contents; None for an empty section.
    pub path: Option<PathBuf>,
    /// How many inputs come before the option on the command line.
    pub position: usize,
}

/// A -rename_section: (old_seg, old_sect, new_seg, new_sect).
pub type SectionRename = (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>);

/// Parsed command line arguments.
#[derive(Debug)]
pub struct Args {
    pub output: PathBuf,
    /// The output file type: MH_EXECUTE, MH_DYLIB, MH_BUNDLE,
    /// MH_KEXT_BUNDLE or MH_DYLINKER (MH_EXECUTE for a relocatable
    /// object, which the relocatable flag makes).
    pub output_type: u32,
    /// -install_name: the LC_ID_DYLIB string, kept as the bytes given.
    pub install_name: Option<Vec<u8>>,
    /// -final_output: the install name a dylib gets when -install_name
    /// is absent (the compiler driver passes it with several -arch).
    pub final_output: Option<Vec<u8>>,
    /// -keep_private_externs: a -r output keeps private externals as
    /// such instead of making them non-external.
    pub keep_private_externs: bool,
    /// -arch, canonicalized to the target's own spelling of its name.
    pub arch: Option<&'static str>,
    /// -e: the entry point, "_main" unless given ("start" for an image
    /// no dyld loads, and for dyld itself).
    pub entry: String,
    /// The deployment target: -platform_version's platform (PLATFORM_*),
    /// minimum OS and SDK versions, else those of the first object
    /// file (see infer_platform). The platform is 0 (none) only in a
    /// -r or -preload link that nothing names one for.
    pub platform: u32,
    pub platform_minos: u32,
    pub platform_sdk: u32,
    pub syslibroot: Vec<PathBuf>,
    /// The -L and -F directories, and once passes::set_search_paths has
    /// settled them, the library and framework search paths.
    pub library_paths: Vec<PathBuf>,
    pub framework_paths: Vec<PathBuf>,
    pub inputs: Vec<InputArg>,
    /// -rpath: LC_RPATH strings, as given.
    pub rpaths: Vec<Vec<u8>>,
    /// Whether the output is ad-hoc code signed: -adhoc_codesign /
    /// -no_adhoc_codesign, resolved for the target at the end of
    /// parsing.
    pub adhoc_codesign: bool,
    pub dead_strip: bool,
    /// -S: do not emit debug stab symbols.
    pub strip_debug: bool,
    pub all_load: bool,
    pub load_objc: bool,
    /// Symbols to treat as undefined from the start, forcing archive
    /// members that define them to be linked: those -u and -init name,
    /// and those an export list names without wildcards (ld64's "initial
    /// undefines"). One that stays undefined is an error even under
    /// -undefined dynamic_lookup.
    pub forced_undefined: Vec<String>,
    /// If set, only these symbols are exported (-exported_symbols_list
    /// or -exported_symbol).
    pub exported_symbols: Option<Glob>,
    /// -no_exported_symbols: hide every definition.
    pub no_exported_symbols: bool,
    /// Symbols to remove from the exported set.
    pub unexported_symbols: Glob,
    /// -reexported_symbols_list: publish selected imports as exports.
    pub reexported_symbols: Glob,
    pub current_version: u32,
    pub compatibility_version: u32,
    /// -map: write a map file describing the output layout.
    pub map: Option<PathBuf>,
    /// -dependency_info: write Xcode's binary dependency listing.
    pub dependency_info: Option<PathBuf>,
    /// The -filelist files, which the listing names among the inputs.
    pub filelists: Vec<PathBuf>,
    /// -sdk_imports: Xcode's JSON report of imported APIs.
    pub sdk_imports: Option<PathBuf>,
    /// -sdk_imports_api_list: the APIs the report lists, of all the
    /// imports, and the list's version, which it records.
    pub sdk_imports_api_list: Option<crate::api_list::ApiList>,
    /// Whether the image is laid out for chained fixups rather than
    /// classic dyld info (its imports bound by no lazy pointer):
    /// -fixup_chains / -no_fixup_chains, resolved for the deployment
    /// target at the end of parsing (see resolve_fixup_chains). An
    /// x86-64 image still falls back to classic dyld info at an
    /// unaligned pointer (see Context::use_chained_fixups).
    pub fixup_chains: bool,
    /// -no_fixup_chains: besides turning chained fixups off, it gives an
    /// image no dyld loads (which has no fixups unless asked for them)
    /// classic rebase and weak-bind opcodes.
    pub no_fixup_chains: bool,
    /// The libLTO to load for bitcode inputs (-lto_library).
    pub lto_library: Option<PathBuf>,
    /// -mcpu: the CPU libLTO compiles the bitcode for.
    pub lto_cpu: Option<String>,
    /// -lto_softload_runtime_symbols / -no_lto_softload_runtime_symbols,
    /// the last one given, or else whether the image is -static or
    /// -preload (see passes::LTO_RUNTIME_ROUTINES).
    pub lto_softload: bool,
    /// -save-temps: keep LTO's intermediate bitcode and objects beside
    /// the output.
    pub save_temps: bool,
    /// -flto-codegen-only: have libLTO compile each bitcode module as
    /// it is, ThinLTO or not, without optimizing it.
    pub lto_codegen_only: bool,
    /// -mllvm: options for LLVM's optimizer and code generator, which
    /// libLTO parses as its own command line.
    pub mllvm: Vec<Vec<u8>>,
    /// -cache_path_lto: the ThinLTO cache directory.
    pub lto_cache_dir: Option<PathBuf>,
    /// -prune_interval_lto: the seconds between prunings of the ThinLTO
    /// cache (-1 for never), if given; libLTO's default otherwise.
    pub lto_cache_prune_interval: Option<i32>,
    /// -prune_after_lto: the seconds an unused ThinLTO cache entry
    /// lasts; 0 for libLTO's default.
    pub lto_cache_expiration: u32,
    /// -max_relative_cache_size_lto: the percentage of the free space
    /// the ThinLTO cache may take; 0 for libLTO's default.
    pub lto_cache_max_size: u32,
    /// -stack_size: the main thread's stack size, recorded in LC_MAIN,
    /// or reserved as the __UNIXSTACK segment of an executable that
    /// starts from LC_UNIXTHREAD.
    pub stack_size: u64,
    /// -sectcreate and -add_empty_section, in command-line order: the
    /// sections to synthesize.
    pub sectcreate: Vec<SectCreate>,
    /// -r: produce a relocatable object instead of a final image.
    pub relocatable: bool,
    /// -flat_namespace: bind imports by name across all loaded images
    /// instead of to specific dylibs.
    pub flat_namespace: bool,
    /// -Z: do not search the standard library and framework
    /// directories.
    pub no_standard_dirs: bool,
    /// -v: print the version and the search paths.
    pub verbose: bool,
    /// -version_details: print the linker's description in JSON and,
    /// as -v does, the search paths.
    pub version_details: bool,
    /// -x: strip non-global symbols from the output symbol table.
    pub strip_locals: bool,
    /// Fold identical functions (on by default; -no_deduplicate turns
    /// it off and a later -deduplicate back on).
    pub deduplicate: bool,
    /// Describe the image's DTrace probe sites in DOF sections (on by
    /// default; -no_dtrace_dof turns it off, and with it what makes the
    /// sites links: see dtrace).
    pub dtrace_dof: bool,
    /// -verbose_deduplicate: report what folding saved.
    pub verbose_deduplicate: bool,
    /// Emit LC_FUNCTION_STARTS (on by default but in a -static image).
    pub function_starts: bool,
    /// Emit LC_DATA_IN_CODE (on by default but in a -static image).
    pub data_in_code_info: bool,
    /// -version_load_command: give a -static image LC_BUILD_VERSION.
    pub version_load_command: bool,
    /// LC_SOURCE_VERSION's version, -source_version's or 0; None for no
    /// command (before macOS 10.8 unless -add_source_version or
    /// -source_version asks for one, or under -no_source_version).
    pub source_version: Option<u64>,
    /// The image starts from LC_UNIXTHREAD's thread state rather than
    /// from LC_MAIN, through which dyld calls main (see parse_args).
    pub unixthread: bool,
    /// -add_split_seg_info: emit LC_SEGMENT_SPLIT_INFO, which lets a
    /// dyld shared cache or kernel collection builder slide the
    /// segments apart. ld64 and ld-prime have no negative form.
    pub add_split_seg_info: bool,
    /// Whether initializers are 32-bit image offsets (__init_offsets)
    /// rather than absolute pointers (__mod_init_func): -init_offsets,
    /// or implied by chained fixups (see parse_args).
    pub init_offsets: bool,
    /// -init: the function the image runs before its other
    /// initializers (the last one given).
    pub init: Option<String>,
    /// -data_const / -no_data_const: whether read-only-after-fixup data
    /// sections (__const, __cfstring, the ObjC lists, __got ...) go in
    /// a __DATA_CONST segment. ld64's default is on but for a -static
    /// image.
    pub data_const: bool,
    /// -no_implicit_dylibs: do not bind through a re-export to the
    /// defining dylib (and add a load command for it); bind to the
    /// re-exporting dylib named on the command line instead.
    pub no_implicit_dylibs: bool,
    /// Whether Objective-C method lists are rewritten in the relative
    /// form: -objc_relative_method_lists /
    /// -no_objc_relative_method_lists, resolved for the target at the
    /// end of parsing.
    pub objc_relative_method_lists: bool,
    /// Merge categories into the classes defined in the same image, as
    /// ld64 does unless -no_objc_category_merging is given (there is no
    /// option to turn it on).
    pub objc_category_merging: bool,
    /// Compute a content-hash LC_UUID (on by default; -no_uuid leaves
    /// it zeroed - dyld refuses executables without the load command).
    pub uuid: bool,
    /// -random_uuid: a random LC_UUID in place of the content hash,
    /// which saves hashing a large output.
    pub random_uuid: bool,
    /// $RC_UUID_SALT: what Apple's build system has hashed into the
    /// content-hash LC_UUID with the output, so that one build's image
    /// differs in UUID from another build's of the same contents; empty
    /// for none.
    pub uuid_salt: Vec<u8>,
    /// -no_dynamic_access: dyld may neither dlopen() the image nor find
    /// its symbols with dlsym() (MH_NOFIXPREBINDING), for a dynamic main
    /// executable or a dylib (see check_output_kind).
    pub no_dynamic_access: bool,
    /// -warn_weak_exports / -no_weak_exports: warn of, or refuse, the
    /// weak definitions a final image exports (and the definitions that
    /// override a dylib's weak one), which dyld coalesces at launch.
    pub warn_weak_exports: bool,
    pub no_weak_exports: bool,
    /// -no_weak_imports: a final image may not import a symbol any
    /// object references weakly (weak_import).
    pub no_weak_imports: bool,
    /// -weak_reference_mismatches: what a final image makes of a symbol
    /// it imports that some objects reference weakly and others not.
    pub weak_reference_mismatches: WeakRefMismatches,
    /// -commons, and -warn_commons (or $LD_WARN_COMMONS): warn of each
    /// tentative definition that wins over a dylib's definition.
    pub commons: CommonsMode,
    pub warn_commons: bool,
    /// $LD_WARN_ON_SWIFT_ABI_VERSION_MISMATCHES: objects built for
    /// different Swift ABI versions draw a warning rather than an error.
    pub warn_swift_abi_mismatches: bool,
    /// $LD_PREFER_TAPI_FILE: a library search takes a stub over the
    /// library next to it in an SDK too (see passes::Prober::library).
    pub prefer_stubs: bool,
    /// -max_default_common_align, as a power of two: the most a common
    /// symbol with no alignment of its own is aligned to (its size
    /// rounded up to a power of two). 2^15 unless given, 2^8 in a
    /// -preload image.
    pub max_default_common_align: u8,
    /// -force_symbols_weak_list / -force_symbols_not_weak_list: the
    /// exported definitions a final image makes weak, or not weak,
    /// whatever their objects say (the weak list winning).
    pub force_weak: Glob,
    pub force_not_weak: Glob,
    /// -keep_duplicate / -keep_duplicates_list: the functions (local
    /// ones too) function deduplication leaves alone.
    pub keep_duplicates: Glob,
    /// -allow_dead_duplicates: a symbol defined more than once is fine
    /// if -dead_strip removes all its definitions but the one kept.
    pub allow_dead_duplicates: bool,
    /// -poison_symbol / -poison_symbols_list: symbols the output may
    /// not refer to.
    pub poisoned: Glob,
    /// -deployment_target_mismatches: what to make of an object built
    /// for a newer OS version than the link's (a warning unless given).
    pub deployment_target_mismatches: Treatment,
    /// What to make of a pointer dyld fixes up that is not 8-aligned:
    /// -unaligned_pointers, resolved for the image at the end of
    /// parsing (see resolve_unaligned_pointers).
    pub unaligned_pointers: Treatment,
    /// The exports -interposable (all of them) or -interposable_list
    /// (those it names, whatever -interposable says) make interposable:
    /// the image refers to them through binds to itself.
    pub interposable: Option<Glob>,
    /// -sub_library / -sub_umbrella: the libraries (by file name, less
    /// the extension) and the frameworks (by -framework name) to
    /// re-export, which no other naming of them may make weak.
    pub sub_libraries: Vec<Vec<u8>>,
    pub sub_umbrellas: Vec<Vec<u8>>,
    /// -image_suffix: suffixes (_debug, _profile) of the library and
    /// framework variants -l and -framework look for before the plain
    /// one, in order.
    pub image_suffixes: Vec<OsString>,
    /// -encryptable (the last of it and -no_encryption): the image's
    /// code may be encrypted after the link, as the App Store does iOS
    /// apps': __TEXT's sections but __oslogstring start on a page of
    /// their own, which LC_ENCRYPTION_INFO_64 names (see
    /// resolve_encryptable).
    pub encryptable: bool,
    /// -w: suppress warnings.
    pub suppress_warnings: bool,
    /// -fatal_warnings, or $LD_TREAT_WARNINGS_AS_ERRORS set to anything
    /// but 0: a warning fails the link.
    pub fatal_warnings: bool,
    pub demangle: bool,
    /// -undefined dynamic_lookup (or suppress): leave unresolved symbols
    /// to be looked up in any loaded image at run time.
    pub undefined_dynamic_lookup: bool,
    /// -U: individual symbols allowed to stay undefined.
    pub allowed_undefined: Vec<String>,
    /// -dead_strip_dylibs: drop load commands for dylibs nothing binds
    /// to.
    pub dead_strip_dylibs: bool,
    /// Whether the dylibs -lazy-l, -lazy_library and -lazy_framework
    /// name load lazily: when the output is for macOS 27 or later.
    pub lazy_load: bool,
    /// Whether to warn about linked dylibs nothing binds to:
    /// -warn_unused_dylibs / -no_warn_unused_dylibs, by default only
    /// for a dylib bound for the dyld shared cache, where each needless
    /// load costs every process (resolved at the end of parsing).
    pub warn_unused_dylibs: bool,
    /// -not_for_dyld_shared_cache: a dylib installed in /usr/lib or
    /// /System/Library that won't go into the dyld shared cache.
    pub not_for_dyld_shared_cache: bool,
    /// -no_shared_cache_eligible: -not_for_dyld_shared_cache, and an
    /// empty LC_SEGMENT_SPLIT_INFO in a final image (but a -preload
    /// one) to mark it so for the cache builder.
    pub shared_cache_marker: bool,
    /// -debug_variant: a debug build, which ld64 keeps out of the dyld
    /// shared cache (and spares warnings that only matter for binaries
    /// shipped to customers; there are none here).
    pub debug_variant: bool,
    /// Whether the image is bound for the dyld shared cache (ld64's
    /// fSharedRegionEligible): see resolve_shared_region.
    pub shared_region: bool,
    /// -no_inits / -no_warn_inits: static initializers are an error, or
    /// go unmentioned in a dylib bound for the shared cache.
    pub no_inits: bool,
    pub no_warn_inits: bool,
    /// -no_compact_unwind: no __unwind_info; the image unwinds by its
    /// __eh_frame alone (GCC's driver passes it on every link).
    pub no_compact_unwind: bool,
    /// Warn about an FDE beyond the 16 MiB of __eh_frame an
    /// __unwind_info entry can point into (-no_warn_eh_frame_too_large
    /// silences it).
    pub warn_eh_frame_too_large: bool,
    /// -bind_at_load: ask dyld to resolve all bindings at load time.
    pub bind_at_load: bool,
    /// Whether imported functions are called through lazy pointers
    /// bound on first use (classic dyld info's __la_symbol_ptr and
    /// __stub_helper): resolved at the end of parsing.
    pub lazy_binding: bool,
    /// Whether dyld learns what to bind and slide from relocations and
    /// the indirect symbol table, as before LC_DYLD_INFO came with
    /// macOS 10.6 (ld64's legacy LINKEDIT): resolved at the end of
    /// parsing (see resolve_legacy_linkedit).
    pub legacy_linkedit: bool,
    /// -application_extension: mark a dylib safe for app extensions. Set
    /// $LD_APPLICATION_EXTENSION_SAFE or $LD_NO_ENCRYPT (ld64's iOS
    /// variable, which marks it too) makes that the default, which
    /// -no_application_extension undoes.
    pub application_extension: bool,
    /// -simulator_support: a dylib simulator processes may load too
    /// (MH_SIM_SUPPORT).
    pub simulator_support: bool,
    /// -add_ast_path: Swift AST paths recorded as N_AST stabs for the
    /// debugger.
    pub add_ast_paths: Vec<PathBuf>,
    pub dynamic: bool,
    /// -headerpad: the space left free after the load commands (32
    /// unless given, 128 in firmware dyld loads; an image dyld loads
    /// never gets less than 32).
    pub headerpad: u64,
    /// -headerpad_max_install_names: room for every dylib load command
    /// to grow to MAXPATHLEN.
    pub headerpad_max_install_names: bool,
    /// -search_dylibs_first: search every path for a dylib before
    /// falling back to archives.
    pub search_dylibs_first: bool,
    /// -search_in_sparse_frameworks: look for a framework not found
    /// in the search path in its Versions/Current too.
    pub search_in_sparse_frameworks: bool,
    /// -dylib_file install_name:file: (install name, file) pairs, the
    /// files to load a dylib re-exports under those install names from,
    /// before looking anywhere else.
    pub dylib_files: Vec<(Vec<u8>, PathBuf)>,
    /// -umbrella: declare this dylib a subframework of the named
    /// umbrella framework (LC_SUB_FRAMEWORK).
    pub umbrella: Option<Vec<u8>>,
    /// -oso_prefix: prefix to strip from N_OSO stab paths ("."  means
    /// the current directory).
    pub oso_prefix: Option<Vec<u8>>,
    /// -export_dynamic: keep all global symbols through LTO even in an
    /// executable, for dlsym or plugin use.
    pub export_dynamic: bool,
    /// -order_file: files of symbol names; the subsections they name are
    /// placed first in their output sections, in file order.
    pub order_files: Vec<PathBuf>,
    /// -order_file_statistics (or LD_PRINT_ORDER_FILE_STATISTICS in the
    /// environment): report the -order_file lines that order nothing.
    pub order_file_statistics: bool,
    /// An order file's file:symbol line names a symbol of the object
    /// LTO compiled by the bitcode file it came from, not the object
    /// (-use_lto_filenames_in_order_file_matching, the default).
    pub lto_filenames_in_order_file: bool,
    /// -object_path_lto: keep the LTO-compiled object at this path for
    /// the debugger.
    pub object_path_lto: Option<PathBuf>,
    /// --print-dependencies: report which file satisfied each
    /// undefined symbol.
    pub print_dependencies: bool,
    /// -why_load: report which symbol caused each archive member to
    /// load.
    pub why_load: bool,
    /// -why_live: for each matching symbol, print the reference chain
    /// that kept it alive through -dead_strip ("*" wildcards allowed).
    pub why_live: Glob,
    /// Whether the link notes the files of the private libraries a
    /// dylib re-exports and merges (DylibFile::merged_files), which
    /// gathering costs: -map and -why_live list them, and the
    /// diagnostics of tentative definitions a dylib defines too, and of
    /// re-exports a library the image re-exports whole makes redundant,
    /// name them.
    pub merged_files: bool,
    /// -alias/-alias_list: (existing, new) symbol aliases to define.
    pub aliases: Vec<(String, String)>,
    /// -sectalign: (segment, section, p2align), the alignment of an
    /// output section whatever its members ask for.
    pub sectalign: Vec<(Vec<u8>, Vec<u8>, u8)>,
    /// -allowable_client: clients that may link this subframework
    /// (LC_SUB_CLIENT).
    pub allowable_clients: Vec<Vec<u8>>,
    /// -client_name: the name this link presents when checking
    /// subframework restrictions.
    pub client_name: Option<Vec<u8>>,
    /// -t: print each file that takes part in the link.
    pub trace: bool,
    /// -trace_symbol_layout and -trace_symbol_layout_file: report where
    /// the symbol moves put symbols, on stdout or into this file.
    pub trace_symbol_layout: bool,
    pub trace_symbol_layout_file: Option<PathBuf>,
    /// -trace_file, -trace_file_shared_cache, -trace_symbols_file: the
    /// files Apple's build system has the link append a JSON record of
    /// what it linked to (see mapfile::write_trace_files).
    pub trace_file: Option<PathBuf>,
    pub trace_file_shared_cache: Option<PathBuf>,
    pub trace_symbols_file: Option<PathBuf>,
    /// $LD_TRACE_SYMBOLS_DIR: the directory -trace_symbols_file's record
    /// goes to, in a file of its own, where no -trace_symbols_file names
    /// one (see trace_env).
    pub trace_symbols_dir: Option<PathBuf>,
    /// -trace_implicit_libraries: print the libraries auto-link options
    /// and re-exports bring in, or with -trace_implicit_library only
    /// those whose names hold one of these.
    pub trace_implicit_libraries: bool,
    pub trace_implicit_library: Vec<Vec<u8>>,
    /// -arch_errors_fatal: an input file without the link's
    /// architecture is an error rather than ignored with a warning.
    pub arch_errors_fatal: bool,
    /// -allow_sub_type_mismatches: an object of another subtype of the
    /// link's CPU type is linked, with a warning, rather than ignored.
    pub allow_sub_type_mismatches: bool,
    /// The link takes a fat dylib's slice of its own CPU subtype only,
    /// not one of another subtype of its CPU type (an arm64e slice in
    /// an arm64 link): -no_allow_dylib_sub_type_mismatches, or else
    /// $LD_DYLIB_CPU_SUBTYPES_MUST_MATCH, names an architecture of its
    /// CPU type.
    pub dylib_subtypes_must_match: bool,
    /// The architecture whose dylibs stand in for those of the link's
    /// that are missing, as $LD_DYLIB_ARCH_FALLBACK names it (see
    /// dylib_arch_fallback).
    pub dylib_arch_fallback: Option<String>,
    /// -ignore_optimization_hints: skip LC_LINKER_OPTIMIZATION_HINT
    /// processing.
    pub ignore_optimization_hints: bool,
    /// -print_statistics: report pass timings and sizes to stderr;
    /// mold's --perf.
    pub perf: bool,
    /// -warn_duplicate_libraries (default): warn when one library is
    /// named more than once.
    pub warn_duplicate_libraries: bool,
    /// -non_global_symbols_strip_list: local symbols to drop from the
    /// output symbol table (glob patterns).
    pub local_strip_list: Glob,
    /// -non_global_symbols_no_strip_list: if set, only matching local
    /// symbols stay.
    pub local_keep_list: Option<Glob>,
    pub pagezero_size: u64,
    /// True when -pagezero_size was given explicitly (a non-zero size
    /// is an error anywhere but a main executable).
    pub explicit_pagezero: bool,
    /// -image_base (or -seg1addr): __TEXT's address, and so the mach
    /// header's; a -segaddr for __TEXT sets it too. resolve_image_base
    /// drops it for an image dyld slides wherever it likes.
    pub image_base: Option<u64>,
    /// -segaddr: (segment, address) pins, one per segment (the last
    /// one given wins).
    pub segaddrs: Vec<(Vec<u8>, u64)>,
    /// -segprot: (segment, max, init) protections.
    pub segprots: Vec<(Vec<u8>, u8, u8)>,
    /// -segment_order: segment names in output order.
    pub segment_order: Vec<Vec<u8>>,
    /// -seg_page_size: (segment, size), the boundary the segment after
    /// the named one starts on, in memory and in the file.
    pub seg_page_sizes: Vec<(Vec<u8>, u64)>,
    /// -segalign: the boundary segments start and end on, in memory and
    /// in the file, and the most a section may be aligned to. The
    /// target's page size unless given, but 4 KiB for a -preload image
    /// on any target (ld64's default segment alignment, which it raises
    /// to the 16 KiB arm64 page for every other kind of image).
    pub segment_align: u64,
    /// -no_zero_fill_sections: zero-fill sections take their space in
    /// the file, as regular sections.
    pub no_zero_fill_sections: bool,
    /// Whether to warn about a section aligned beyond its segment, as
    /// it is unless -no_warn_reduced_section_align.
    pub warn_reduced_section_align: bool,
    /// -section_order: (segment, section names), the sections that
    /// lead their segment, in this order.
    pub section_order: Vec<(Vec<u8>, Vec<Vec<u8>>)>,
    /// -rename_section, in command-line order.
    pub rename_sections: Vec<SectionRename>,
    /// -rename_segment: (old, new).
    pub rename_segments: Vec<(Vec<u8>, Vec<u8>)>,
    /// -move_to_rw_segment and -move_to_ro_segment: the lists of the
    /// data and of the code to move to other segments, in command-line
    /// order (the first list naming a symbol decides where it goes).
    pub move_to_rw: Vec<SymbolMove>,
    pub move_to_ro: Vec<SymbolMove>,
    /// -dirty_data_list: the lists of the data to move to __DATA_DIRTY.
    pub dirty_data: Vec<SymbolMove>,
    /// ZERO_AR_DATE is set, or -reproducible given: the stabs record no
    /// modification times.
    pub zero_ar_date: bool,
    /// A static executable (-static, -preload): an image no dyld loads
    /// (the XNU kernel), with no LC_MAIN or imports, and fixups only if
    /// -fixup_chains or -no_fixup_chains asks.
    pub static_link: bool,
    /// -preload: a -static executable (static_link is set too) whose
    /// mach header, load commands and symbol table lie outside its
    /// segments, for firmware whose segments are copied out into ROM;
    /// its header says MH_PRELOAD.
    pub preload: bool,
    /// -kernel: the -static image is a kernel (XNU), which kmutil slides
    /// into a kernel collection: position independent and, like a
    /// shared-cache dylib, with split info.
    pub kernel: bool,
    /// -kexts_use_stubs: an x86-64 kext calls its imports through
    /// stubs and GOT slots, not by external relocations on the calls.
    pub kexts_use_stubs: bool,
    /// Code goes in its own __TEXT_EXEC segment, and __TEXT is
    /// read-only (ld64's -text_exec, implied by an arm64 -kext).
    pub text_exec: bool,
    /// -no_branch_islands: make no range-extension thunks, so that a
    /// branch out of reach is an error.
    pub no_branch_islands: bool,
    /// Whether an executable is position independent (MH_PIE):
    /// -pie / -no_pie, resolved for the target at the end of parsing.
    pub pie: bool,
    /// Whether a pointer may need a fixup in a segment mapped without
    /// write permission (a text relocation): resolved from the kind of
    /// output and -read_only_relocs at the end of parsing.
    pub text_relocs: bool,
    /// Whether the image gets the hook for the classes of the mergeable
    /// libraries it merges or re-exports (-merge_*, -no_merge_*): code
    /// that has objc_setHook_getImageName place each such class in its
    /// framework's directory within the app bundle, where the
    /// framework's resources stay, rather than in this image (see
    /// bundle_hook). On unless -no_merged_libraries_hook.
    pub merged_libraries_hook: bool,
    /// -make_mergeable: a dylib that a later link may merge (-merge_*),
    /// for which ld-prime writes the mergeable record (LC_ATOM_INFO), as
    /// Xcode builds a MERGEABLE_LIBRARY.
    pub make_mergeable: bool,
    /// -add_mergeable_debug_hook: a debug build of a mergeable dylib
    /// gets the hook of merged libraries itself, for its classes that
    /// it doesn't export, which no image re-exporting it can name.
    pub add_mergeable_debug_hook: bool,
    /// -dyld_env: DYLD_xxx=value settings dyld applies as it launches a
    /// main executable (LC_DYLD_ENVIRONMENT), as given.
    pub dyld_envs: Vec<Vec<u8>>,
    /// -objc_stubs_small: an arm64 _objc_msgSend$<selector> stub loads
    /// its selector and branches to _objc_msgSend (through its __stubs
    /// entry, if imported) instead of loading _objc_msgSend from a GOT
    /// slot of its own (-objc_stubs_fast, the default). ld-prime makes
    /// x86-64's stubs the same either way.
    pub objc_stubs_small: bool,
    /// Whether __objc_selrefs goes in __DATA_CONST (with -data_const):
    /// -const_selrefs / -no_const_selrefs, the last one given, or by
    /// default only in an image bound for the shared region, whatever
    /// the deployment target (unlike ld64's, from macOS 13 on).
    pub const_selrefs: bool,
    /// -no_dwarf_unwind: leave the inputs' __eh_frame out of the output
    /// (see input_files::KeptFdes).
    pub no_dwarf_unwind: bool,
    /// -ignore_auto_link: neither act on the objects' auto-link options
    /// (LC_LINKER_OPTION) nor -add_linker_option's, nor carry any into
    /// a -r output.
    pub ignore_auto_link: bool,
    /// -add_linker_option: auto-link options as if an object gave them,
    /// the words of every one in a row (see passes::read_linker_options).
    pub linker_options: Vec<Vec<u8>>,
    /// -force_load_swift_libs: load every member of an archive an
    /// auto-link option finds whose file name starts with "libswift".
    pub force_load_swift_libs: bool,
    /// -merge_zero_fill_sections: each segment's zero-fill sections, the
    /// commons too, form one __zerofill section (see
    /// output_sections::SectionMap::zero_fill_name).
    pub merge_zero_fill_sections: bool,
    /// -fixup_chains_section (or -fixup_chains_section_vm): a -static
    /// image's fixup chains start where __TEXT,__chain_starts says, for
    /// its own loader, not where an LC_DYLD_CHAINED_FIXUPS would.
    pub fixup_chains_section: bool,
    /// What the section's reserved1 says its starts are: 1 for file
    /// offsets (-fixup_chains_section), 2 for VM offsets
    /// (-fixup_chains_section_vm). ld-prime writes VM offsets either way.
    pub chain_starts_kind: u32,
    /// -remove_swift_reflection_metadata_sections: drop the Swift
    /// reflection metadata (see passes::remove_swift_reflection_metadata).
    pub remove_swift_reflection_metadata_sections: bool,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            output: PathBuf::from("a.out"),
            output_type: MH_EXECUTE,
            install_name: None,
            final_output: None,
            keep_private_externs: false,
            arch: None,
            entry: "_main".to_string(),
            platform: 0,
            platform_minos: encode_version(0, 0, 0),
            platform_sdk: encode_version(0, 0, 0),
            syslibroot: Vec::new(),
            library_paths: Vec::new(),
            framework_paths: Vec::new(),
            inputs: Vec::new(),
            rpaths: Vec::new(),
            adhoc_codesign: false,
            dead_strip: false,
            strip_debug: false,
            all_load: false,
            load_objc: false,
            forced_undefined: Vec::new(),
            exported_symbols: None,
            no_exported_symbols: false,
            unexported_symbols: Glob::new(),
            reexported_symbols: Glob::new(),
            // ld64 leaves both at 0.0.0 unless -current_version /
            // -compatibility_version say otherwise.
            current_version: encode_version(0, 0, 0),
            compatibility_version: encode_version(0, 0, 0),
            map: None,
            dependency_info: None,
            filelists: Vec::new(),
            sdk_imports: None,
            sdk_imports_api_list: None,
            fixup_chains: false,
            no_fixup_chains: false,
            lto_library: None,
            lto_cpu: None,
            lto_softload: false,
            save_temps: false,
            lto_codegen_only: false,
            mllvm: Vec::new(),
            lto_cache_dir: None,
            lto_cache_prune_interval: None,
            lto_cache_expiration: 0,
            lto_cache_max_size: 0,
            stack_size: 0,
            sectcreate: Vec::new(),
            relocatable: false,
            flat_namespace: false,
            no_standard_dirs: false,
            verbose: false,
            version_details: false,
            strip_locals: false,
            deduplicate: true,
            dtrace_dof: true,
            verbose_deduplicate: false,
            function_starts: true,
            data_in_code_info: true,
            version_load_command: false,
            source_version: Some(0),
            unixthread: false,
            add_split_seg_info: false,
            init_offsets: false,
            init: None,
            data_const: true,
            no_implicit_dylibs: false,
            objc_relative_method_lists: false,
            objc_category_merging: true,
            uuid: true,
            random_uuid: false,
            uuid_salt: Vec::new(),
            no_dynamic_access: false,
            warn_weak_exports: false,
            no_weak_exports: false,
            no_weak_imports: false,
            weak_reference_mismatches: WeakRefMismatches::NonWeak,
            commons: CommonsMode::IgnoreDylibs,
            warn_commons: false,
            warn_swift_abi_mismatches: false,
            prefer_stubs: false,
            max_default_common_align: 15,
            force_weak: Glob::new(),
            force_not_weak: Glob::new(),
            keep_duplicates: Glob::new(),
            allow_dead_duplicates: false,
            poisoned: Glob::new(),
            deployment_target_mismatches: Treatment::Warning,
            unaligned_pointers: Treatment::Suppress,
            interposable: None,
            sub_libraries: Vec::new(),
            sub_umbrellas: Vec::new(),
            image_suffixes: Vec::new(),
            encryptable: false,
            suppress_warnings: false,
            fatal_warnings: false,
            demangle: false,
            undefined_dynamic_lookup: false,
            allowed_undefined: Vec::new(),
            dead_strip_dylibs: false,
            lazy_load: false,
            warn_unused_dylibs: false,
            not_for_dyld_shared_cache: false,
            shared_cache_marker: false,
            debug_variant: false,
            shared_region: false,
            no_inits: false,
            no_warn_inits: false,
            no_compact_unwind: false,
            warn_eh_frame_too_large: true,
            bind_at_load: false,
            lazy_binding: false,
            legacy_linkedit: false,
            application_extension: false,
            simulator_support: false,
            add_ast_paths: Vec::new(),
            dynamic: true,
            headerpad: 32,
            headerpad_max_install_names: false,
            search_dylibs_first: false,
            search_in_sparse_frameworks: false,
            dylib_files: Vec::new(),
            umbrella: None,
            oso_prefix: None,
            export_dynamic: false,
            order_files: Vec::new(),
            order_file_statistics: false,
            lto_filenames_in_order_file: true,
            object_path_lto: None,
            print_dependencies: false,
            why_load: false,
            why_live: Glob::new(),
            merged_files: false,
            aliases: Vec::new(),
            sectalign: Vec::new(),
            allowable_clients: Vec::new(),
            client_name: None,
            trace: false,
            trace_symbol_layout: false,
            trace_symbol_layout_file: None,
            trace_file: None,
            trace_file_shared_cache: None,
            trace_symbols_file: None,
            trace_symbols_dir: None,
            trace_implicit_libraries: false,
            trace_implicit_library: Vec::new(),
            arch_errors_fatal: false,
            allow_sub_type_mismatches: false,
            dylib_subtypes_must_match: false,
            dylib_arch_fallback: None,
            ignore_optimization_hints: false,
            perf: false,
            warn_duplicate_libraries: true,
            local_strip_list: Glob::new(),
            local_keep_list: None,
            pagezero_size: 0x1_0000_0000,
            explicit_pagezero: false,
            image_base: None,
            segaddrs: Vec::new(),
            segprots: Vec::new(),
            segment_order: Vec::new(),
            seg_page_sizes: Vec::new(),
            segment_align: 0,
            no_zero_fill_sections: false,
            warn_reduced_section_align: true,
            section_order: Vec::new(),
            rename_sections: Vec::new(),
            rename_segments: Vec::new(),
            move_to_rw: Vec::new(),
            move_to_ro: Vec::new(),
            dirty_data: Vec::new(),
            zero_ar_date: false,
            static_link: false,
            preload: false,
            kernel: false,
            kexts_use_stubs: false,
            text_exec: false,
            no_branch_islands: false,
            pie: true,
            text_relocs: false,
            merged_libraries_hook: true,
            make_mergeable: false,
            add_mergeable_debug_hook: false,
            dyld_envs: Vec::new(),
            objc_stubs_small: false,
            const_selrefs: false,
            no_dwarf_unwind: false,
            ignore_auto_link: false,
            linker_options: Vec::new(),
            force_load_swift_libs: false,
            merge_zero_fill_sections: false,
            fixup_chains_section: false,
            chain_starts_kind: 0,
            remove_swift_reflection_metadata_sections: false,
        }
    }
}

impl Args {
    /// The address -segaddr pins a segment to.
    pub fn segaddr(&self, segname: &[u8]) -> Option<u64> {
        self.segaddrs.iter().find(|(name, _)| name == segname).map(|&(_, addr)| addr)
    }
}

/// The numbers of an X.Y.Z version, however many there are (an empty
/// one is 0). ld-prime's complaint about anything else names the
/// option, if there is one to name (a -target triple has none).
fn version_numbers(opt: &str, arg: &str) -> Vec<u64> {
    let nums = if arg.is_empty() || arg.ends_with('.') {
        None
    } else {
        arg.split('.')
            .map(|s| match s {
                "" => Some(0),
                _ if s.bytes().all(|c| c.is_ascii_digit()) => s.parse().ok(),
                _ => None,
            })
            .collect()
    };
    nums.unwrap_or_else(|| {
        fatal!("{}malformed 32-bit xxxx.yy.zz version number: '{arg}'", option_prefix(opt))
    })
}

fn option_prefix(opt: &str) -> String {
    if opt.is_empty() { String::new() } else { format!("{opt}: ") }
}

/// The most each number of a version may be: LC_BUILD_VERSION and
/// LC_ID_DYLIB pack X.Y.Z into 16, 8 and 8 bits.
const VERSION_LIMITS: [u64; 3] = [0xffff, 0xff, 0xff];

fn fits_version(nums: &[u64]) -> bool {
    nums.len() <= 3 && nums.iter().zip(VERSION_LIMITS).all(|(&num, max)| num <= max)
}

/// Parses an OS version, which ld-prime refuses if it doesn't fit.
fn parse_version(opt: &str, arg: &str) -> u32 {
    let nums = version_numbers(opt, arg);
    if !fits_version(&nums) {
        fatal!(
            "{}malformed version number '{arg}' cannot fit in 32-bit xxxx.yy.zz",
            option_prefix(opt)
        );
    }
    let num = |i: usize| nums.get(i).map_or(0, |&num| num as u32);
    encode_version(num(0), num(1), num(2))
}

/// Parses a dylib's current or compatibility version, which ld-prime
/// truncates to fit, with a warning: each number to its most, and the
/// numbers past the third dropped (ld64 took five for the current
/// version). An empty one is 0.
fn parse_dylib_version(opt: &str, arg: &str, warnings: &mut OptionWarnings) -> u32 {
    if arg.is_empty() {
        return encode_version(0, 0, 0);
    }
    let nums = version_numbers(opt, arg);
    if !fits_version(&nums) {
        warnings.warn(format!("truncating {opt} to fit in 32-bit space used by old mach-o format"));
    }
    let num = |i: usize| nums.get(i).map_or(0, |&num| num.min(VERSION_LIMITS[i]) as u32);
    encode_version(num(0), num(1), num(2))
}

/// Parses -source_version's a.b.c.d.e into LC_SOURCE_VERSION's 64 bits,
/// 24 for a and 10 for each other number. ld-prime reads up to five
/// numbers of decimal digits apart by dots, an empty one 0 and each
/// taken modulo 2^32, and ignores what follows the fifth; a dot ending
/// the string before it, another character or a number too large for
/// its bits make the string malformed (None).
fn parse_source_version(arg: &str) -> Option<u64> {
    let mut nums = [0u32; 5];
    let mut s = arg.as_bytes();
    for (i, num) in nums.iter_mut().enumerate() {
        let len = s.iter().take_while(|c| c.is_ascii_digit()).count();
        *num = s[..len]
            .iter()
            .fold(0, |n: u32, &c| n.wrapping_mul(10).wrapping_add((c - b'0') as u32));
        s = &s[len..];
        if i == 4 || s.is_empty() {
            break;
        }
        match s {
            [b'.', rest @ ..] if !rest.is_empty() => s = rest,
            _ => return None,
        }
    }
    let [a, b, c, d, e] = nums.map(u64::from);
    if a > 0xff_ffff || [b, c, d, e].iter().any(|&n| n > 0x3ff) {
        return None;
    }
    Some((a << 40) | (b << 30) | (c << 20) | (d << 10) | e)
}

/// The traces Apple's build system asks for in the environment, where
/// no option names a file: with $LD_TRACE_DEPENDENTS set, whatever its
/// value, -trace_file's record goes to $LD_TRACE_FILE, if that names a
/// file; and with a -trace_file either way, -trace_symbols_file's goes
/// to a file of its own in $LD_TRACE_SYMBOLS_DIR.
fn trace_env(args: &mut Args) {
    if args.trace_file.is_none() && std::env::var_os("LD_TRACE_DEPENDENTS").is_some() {
        let file = std::env::var_os("LD_TRACE_FILE").filter(|file| !file.is_empty());
        args.trace_file = file.map(PathBuf::from);
    }
    if args.trace_file.is_some() && args.trace_symbols_file.is_none() {
        args.trace_symbols_dir = std::env::var_os("LD_TRACE_SYMBOLS_DIR").map(PathBuf::from);
    }
}

/// The source version the build system gives in
/// $RC_ProjectSourceVersion, which ld-prime takes when -source_version
/// gives none, and takes for 0 with a warning when malformed.
fn env_source_version() -> u64 {
    let Some(env) = std::env::var_os("RC_ProjectSourceVersion") else {
        return 0;
    };
    let env = env.to_string_lossy();
    parse_source_version(&env).unwrap_or_else(|| {
        crate::warn!("$RC_ProjectSourceVersion: malformed 64-bit a.b.c.d.e version number: {env}");
        0
    })
}

/// ld64 takes the platform by name, in any case, or by its PLATFORM_*
/// number; Xcode passes the number for some prelink steps
/// (`-platform_version 1 11.0`). A platform ld-prime knows but mold
/// does not link for is unsupported, and any other name unknown.
fn parse_platform(arg: &str) -> u32 {
    let name = arg.to_ascii_lowercase();
    let number = match name.bytes().all(|c| c.is_ascii_digit()) {
        true => name.parse::<u32>().ok(),
        false => None,
    };
    match (name.as_str(), number) {
        ("macos" | "macosx", _) | (_, Some(PLATFORM_MACOS)) => PLATFORM_MACOS,
        ("firmware", _) | (_, Some(PLATFORM_FIRMWARE)) => PLATFORM_FIRMWARE,
        // ld-prime numbers its platforms up to 30.
        (_, Some(1..=30)) => fatal!("unsupported platform: {arg}"),
        (name, _) if is_other_platform(name) => fatal!("unsupported platform: {arg}"),
        _ => fatal!("-platform_version unknown platform: {arg}"),
    }
}

/// Whether ld-prime knows a platform name (in lower case) mold does not
/// link for: Apple's other OSes, their simulators, exclaves and kernel
/// kits.
fn is_other_platform(name: &str) -> bool {
    let (os, variant) = name.split_once('-').unwrap_or((name, ""));
    let apple_os = matches!(os, "macos" | "ios" | "tvos" | "watchos" | "visionos" | "xros");
    match variant {
        "" => {
            apple_os && os != "macos"
                || matches!(os, "bridgeos" | "driverkit" | "sepos" | "maccatalyst")
        }
        "simulator" => apple_os && os != "macos",
        "exclavecore" | "exclavekit" => apple_os,
        "kernelkit" => apple_os || os == "bridgeos",
        "catalyst" => os == "mac",
        _ => false,
    }
}

/// Makes the name a library option gives of its argument.
type Naming = fn(&OsStr) -> LibraryName;

/// The library options with the library's name or path in the next
/// argument: the kind of library each names, and how it names it.
fn library_option(opt: &[u8]) -> Option<(LibraryKind, Naming)> {
    use LibraryKind::*;
    let lib: Naming = |name| LibraryName::Lib(name.to_owned());
    let framework: Naming = |name| LibraryName::Framework(name.to_owned());
    let path: Naming = |path| LibraryName::Path(PathBuf::from(path));
    Some(match opt {
        b"-l" => (Plain, lib),
        b"-framework" => (Plain, framework),
        b"-weak_framework" => (Weak, framework),
        b"-reexport_framework" => (Reexport, framework),
        b"-needed_framework" => (Needed, framework),
        b"-upward_framework" => (Upward, framework),
        b"-lazy_framework" => (Lazy, framework),
        b"-no_merge_framework" => (NoMerge, framework),
        b"-merge_framework" => (Merge, framework),
        b"-hidden_framework" => (Hidden, framework),
        b"-possible_framework" => (Possible, framework),
        b"-assert_weak_framework" => (AssertWeak, framework),
        b"-delay_framework" => (Delay, framework),
        b"-weak_library" => (Weak, path),
        b"-reexport_library" => (Reexport, path),
        b"-needed_library" => (Needed, path),
        b"-upward_library" => (Upward, path),
        b"-lazy_library" => (Lazy, path),
        b"-no_merge_library" => (NoMerge, path),
        b"-merge_library" => (Merge, path),
        b"-load_hidden" => (Hidden, path),
        b"-possible_library" => (Possible, path),
        b"-assert_weak_library" => (AssertWeak, path),
        b"-delay_library" => (Delay, path),
        b"-force_load" => (Force, path),
        _ => return None,
    })
}

/// The library options with the library's name joined to them
/// (-weak-lfoo), and the kind of library each names; -l, the others'
/// prefix, last.
const JOINED_LIBRARY_OPTIONS: [(&str, LibraryKind); 13] = [
    ("-reexport-l", LibraryKind::Reexport),
    ("-no_merge-l", LibraryKind::NoMerge),
    ("-merge-l", LibraryKind::Merge),
    ("-hidden-l", LibraryKind::Hidden),
    ("-needed-l", LibraryKind::Needed),
    ("-upward-l", LibraryKind::Upward),
    ("-lazy-l", LibraryKind::Lazy),
    ("-weak-l", LibraryKind::Weak),
    ("-force-l", LibraryKind::Force),
    ("-possible-l", LibraryKind::Possible),
    ("-assert-weak-l", LibraryKind::AssertWeak),
    ("-delay-l", LibraryKind::Delay),
    ("-l", LibraryKind::Plain),
];

/// The frameworks and the other libraries the options of a kind name,
/// each once, in command line order: ld-prime keeps the two apart.
fn libraries_of_kind(inputs: &[InputArg], kind: LibraryKind) -> (Vec<&[u8]>, Vec<&[u8]>) {
    let mut frameworks: Vec<&[u8]> = Vec::new();
    let mut libraries: Vec<&[u8]> = Vec::new();
    for input in inputs {
        if let InputArg::Library(k, name) = input
            && *k == kind
        {
            let list = match name {
                LibraryName::Framework(_) => &mut frameworks,
                _ => &mut libraries,
            };
            let name = name.as_os_str().as_bytes();
            if !list.contains(&name) {
                list.push(name);
            }
        }
    }
    (frameworks, libraries)
}

/// Decides whether the dylibs -lazy-l and the like name load lazily:
/// dyld loads one when __dyld_lazy_load says so, which ld-prime keeps
/// as an import of any final image that names one, used or not, for
/// macOS 27 on. Firmware, a -preload image included, has no dyld:
/// ld-prime links the library as usual there, with a second warning.
fn resolve_lazy_load(args: &mut Args) {
    let (frameworks, libraries) = libraries_of_kind(&args.inputs, LibraryKind::Lazy);
    if frameworks.is_empty() && libraries.is_empty() {
        return;
    }
    let lazy_load = args.platform == PLATFORM_MACOS
        && args.platform_minos >= encode_version(27, 0, 0)
        && !args.preload;
    for lib in frameworks.iter().chain(&libraries).filter(|_| !lazy_load) {
        crate::warn!(
            "lazy-load will be ignored for '{}' because deployment target version is too low",
            display(lib)
        );
    }
    if args.platform == PLATFORM_FIRMWARE || args.preload {
        for _ in &frameworks {
            crate::warn!(
                "-lazy_framework cannot be used on firmware, changing to regular -framework"
            );
        }
        for _ in &libraries {
            crate::warn!("-lazy_library cannot be used on firmware, changing to regular link");
        }
    }
    args.lazy_load = lazy_load;
    if lazy_load && !args.relocatable {
        args.forced_undefined.push("__dyld_lazy_load".to_string());
    }
}

/// A dylib -delay-l and the like name keeps its initializers until the
/// image dlopen()s it, which dyld supports from macOS 15 on: ld-prime
/// warns of an older or another target (a -preload image included),
/// but delays the dylib all the same. It wants _dlopen as one of the
/// command line's initial undefines in any link that names one, -r
/// included.
fn resolve_delay_init(args: &mut Args) {
    let (frameworks, libraries) = libraries_of_kind(&args.inputs, LibraryKind::Delay);
    if frameworks.is_empty() && libraries.is_empty() {
        return;
    }
    let supported = args.platform == PLATFORM_MACOS
        && args.platform_minos >= encode_version(15, 0, 0)
        && !args.preload;
    for lib in frameworks.iter().chain(&libraries).filter(|_| !supported) {
        crate::warn!(
            "delay-init will be ignored for '{}' because deployment target version is too low",
            display(lib)
        );
    }
    args.forced_undefined.push("_dlopen".to_string());
}

/// Takes the platform and minimum OS version an option names. The last
/// option wins; ld-prime warns about another minimum version for the
/// same platform, and about firmware replacing macOS, but refuses
/// macOS (or another platform) replacing firmware once it has read
/// every option.
fn set_platform(args: &mut Args, st: &mut ParseState, platform: u32, minos: u32) {
    if args.platform == platform && args.platform_minos != minos {
        let (old, new) = (format_version(args.platform_minos), format_version(minos));
        let name = platform_name(platform);
        st.warnings.warn(format!(
            "passed two min versions ({old}, {new}) for platform {name}. Using {new}."
        ));
    } else if args.platform == PLATFORM_MACOS && platform == PLATFORM_FIRMWARE {
        st.warnings.warn("conflicting -platform_version platform: macOS, using: firmware");
    } else if args.platform != 0 && args.platform != platform {
        st.incompatible_platforms.get_or_insert((args.platform, platform));
    }
    args.platform = platform;
    args.platform_minos = minos;
}

/// Applies -target <arch>-<vendor>-<os><version>, which clang passes in
/// place of -arch and -platform_version for firmware (for instance
/// arm64-apple-firmware1.0.0). ld-prime lets the triple override both,
/// before or after it, and records no SDK version.
fn apply_target_triple(args: &mut Args, triple: &str) {
    let (arch, platform, minos) = parse_triple(triple);
    args.platform = platform;
    args.platform_minos = minos;
    args.platform_sdk = encode_version(0, 0, 0);
    args.arch = Some(
        target_arch(arch)
            .unwrap_or_else(|| fatal!("unknown architecture in target triple '{triple}'")),
    );
}

/// The CPU family - "arm64", "x86_64" or another - of an architecture
/// name ld-prime knows, as -arch_variant and
/// $LD_DYLIB_CPU_SUBTYPES_MUST_MATCH take them (and -arch, for the
/// targets mold links for); None for one it doesn't.
fn arch_cpu_family(name: &[u8]) -> Option<&'static str> {
    const ARM64: [&str; 14] = [
        "arm64",
        "arm64e",
        "arm64.x1",
        "arm64.x2",
        "arm64e.x1",
        "arm64e.x2",
        "arm64e.v1",
        "arm64e.old",
        "arm64e.x1.old",
        "arm64e.kernel",
        "arm64e.kernel.v1",
        "arm64e.kernel.v2",
        "arm64e.x1.kernel",
        "arm64e.x2.kernel",
    ];
    const OTHER: [&str; 18] = [
        "armv4t",
        "armv6",
        "armv7",
        "armv7k",
        "armv7s",
        "armv6m",
        "armv7m",
        "armv7em",
        "armv8m.main",
        "armv8.1m.main",
        "thumbv6m",
        "thumbv7",
        "thumbv7k",
        "thumbv7s",
        "thumbv7m",
        "thumbv7em",
        "thumbv8m.main",
        "thumbv8.1m.main",
    ];
    let is = |names: &[&str]| names.iter().any(|n| n.as_bytes() == name);
    match name {
        _ if is(&ARM64) => Some("arm64"),
        b"x86_64" | b"x86_64h" => Some("x86_64"),
        b"arm64_32" | b"i386" | b"ppc" => Some("other"),
        _ if is(&OTHER) => Some("other"),
        _ => None,
    }
}

/// The architecture $LD_DYLIB_ARCH_FALLBACK names, as "<arch>:<other>",
/// for a link for <arch>: a dylib of <other> - thin, or a fat file's
/// slice where none of the link's is - stands in for a dylib of the
/// link's own (ld64's means to take armv7k slices for arm64_32). A
/// value for another architecture, or one naming none ld-prime knows,
/// names none.
fn dylib_arch_fallback(arch: &str) -> Option<String> {
    let env = std::env::var_os("LD_DYLIB_ARCH_FALLBACK")?;
    let (from, to) = env.to_str()?.split_once(':')?;
    (from == arch && arch_cpu_family(to.as_bytes()).is_some()).then(|| to.to_string())
}

/// Whether a ':'-separated list of architecture names, as
/// -no_allow_dylib_sub_type_mismatches and
/// $LD_DYLIB_CPU_SUBTYPES_MUST_MATCH give one, names one of CPU family
/// `family`. ld-prime warns of each name it doesn't know (and passes
/// over a list of just "1").
fn names_cpu_family(list: &[u8], family: &str) -> bool {
    if list == b"1" {
        return false;
    }
    let mut found = false;
    for name in list.split(|&c| c == b':').filter(|name| !name.is_empty()) {
        match arch_cpu_family(name) {
            Some(f) => found |= f == family,
            None => crate::warn!(
                "unknown architecture name '{}' in LD_DYLIB_CPU_SUBTYPES_MUST_MATCH",
                display(name)
            ),
        }
    }
    found
}

/// The target an architecture name ld-prime knows stands for, None for
/// a name it does not know. mold links for arm64 and x86_64 of them,
/// and ld-prime no longer for i386.
fn target_arch(arch: &str) -> Option<&'static str> {
    // As `ld -v` lists them.
    const KNOWN: [&str; 15] = [
        "armv6",
        "armv7",
        "armv7s",
        "arm64",
        "arm64e",
        "arm64_32",
        "i386",
        "x86_64",
        "x86_64h",
        "armv6m",
        "armv7k",
        "armv7m",
        "armv7em",
        "armv8m.main",
        "armv8.1m.main",
    ];
    if arch == "i386" {
        fatal!("linking for i386 is no longer supported");
    }
    if !KNOWN.contains(&arch) {
        return None;
    }
    Some(
        crate::target::canonical_name(arch).unwrap_or_else(|| fatal!("unsupported target: {arch}")),
    )
}

/// Splits a target triple, <arch>-<vendor>-<os><version>, into its
/// architecture, platform and OS version.
fn parse_triple(triple: &str) -> (&str, u32, u32) {
    let mut parts = triple.splitn(3, '-');
    let (Some(arch), Some(_vendor), Some(os)) = (parts.next(), parts.next(), parts.next()) else {
        fatal!("missing dashes in target triple '{triple}'");
    };
    let (os_name, version) = os.split_at(os.find(|c: char| c.is_ascii_digit()).unwrap_or(os.len()));
    let platform = match os_name.to_ascii_lowercase().as_str() {
        "macos" | "macosx" => PLATFORM_MACOS,
        "firmware" => PLATFORM_FIRMWARE,
        name if is_other_platform(name) => fatal!("unsupported platform: {os_name}"),
        _ => 0,
    };
    // An environment after the version (clang makes x86-64 firmware
    // x86_64-apple-firmware1.0.0-simulator) names no OS either.
    if platform == 0 || version.contains('-') {
        fatal!("unknown OS in target triple '{triple}'");
    }
    // Firmware tracks no OS versions; macOS must say which.
    let minos = match version {
        "" if platform == PLATFORM_FIRMWARE => encode_version(0, 0, 0),
        "" => fatal!("missing OS version in target triple '{triple}'"),
        _ => parse_version("", version),
    };
    (arch, platform, minos)
}

/// The three ways to choose the exports: an export list, an unexport
/// list, or -no_exported_symbols.
#[derive(Clone, Copy, PartialEq)]
enum ExportChoice {
    Exported,
    Unexported,
    None,
}

/// ld64 takes one way to choose the exports, and rejects an option of
/// another with a message named after the option that comes second.
fn check_export_choice(seen: &mut Option<ExportChoice>, choice: ExportChoice, opt: &str) {
    if seen.is_some_and(|seen| seen != choice) {
        match opt {
            "-exported_symbol" => {
                fatal!("-exported_symbol cannot be used with -unexported_symbol*")
            }
            "-unexported_symbol" => {
                fatal!("-unexported_symbol cannot be used with -exported_symbol*")
            }
            "-no_exported_symbols" => {
                fatal!("-no_exported_symbols cannot be used with -[un]exported_symbol*")
            }
            _ => fatal!(
                "{opt}: -exported_symbol*, -unexported_symbol* and -no_exported_symbols cannot be used together"
            ),
        }
    }
    *seen = Some(choice);
}

/// The name a symbol list entry spells, as ld-prime reads one: None for
/// a pattern, which has a wildcard (`*`, `?` or `[`) no backslash
/// escapes; otherwise the entry with a backslash taking the character
/// after it as itself (`_a\*` names `_a*`, `_a\b` `_ab`).
fn exact_name(entry: &str) -> Option<String> {
    let mut name = Vec::with_capacity(entry.len());
    let mut bytes = entry.bytes();
    while let Some(c) = bytes.next() {
        match c {
            b'*' | b'?' | b'[' => return None,
            b'\\' => name.push(bytes.next().unwrap_or(c)),
            _ => name.push(c),
        }
    }
    // A backslash dropped from UTF-8 leaves UTF-8.
    Some(String::from_utf8(name).unwrap())
}

/// Makes the names among a symbol list's entries initial undefines: an
/// object need not mention them for them to pull in an archive member,
/// and each must resolve. Patterns only match symbols already there.
fn add_initial_undefines(undefs: &mut Vec<String>, entries: impl IntoIterator<Item: AsRef<str>>) {
    undefs.extend(entries.into_iter().filter_map(|entry| exact_name(entry.as_ref())));
}

/// Adds a symbol list's entries to `glob` with `value`: the names they
/// spell (see exact_name), and the patterns - a malformed one, such as
/// `_a[`, matching nothing, as ld-prime takes it without a word.
fn add_patterns(glob: &mut GlobBuilder, entries: impl IntoIterator<Item: AsRef<str>>, value: i64) {
    for entry in entries {
        let entry = entry.as_ref();
        match exact_name(entry) {
            Some(name) => glob.add_literal(name.as_bytes(), value),
            None => {
                glob.add(entry.as_bytes(), value);
            }
        }
    }
}

/// Reads a symbol list file. ld-prime ends its error about one it can't
/// open, as about a -filelist file, with a blank line.
fn read_symbol_list(opt: &str, path: &Path) -> Vec<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => symbol_list(&text),
        Err(e) => fatal!(
            "{opt} file '{}' could not be opened, {}\n",
            path.display(),
            crate::error::errno_text(&e)
        ),
    }
}

/// Reads the list of a symbol move to `segment` (see SymbolMove), whose
/// lines are those of an export list: names, patterns, and either
/// qualified as file:name to match the symbol of an object of that
/// leaf name ("foo.o", "libfoo.a(foo.o)") alone.
fn symbol_move(opt: &str, segment: &[u8], path: &Path) -> SymbolMove {
    let mut symbols = GlobBuilder::default();
    for entry in read_symbol_list(opt, path) {
        match exact_name(&entry) {
            Some(name) => symbols.add_literal(name.as_bytes(), 1),
            None => add_patterns(&mut symbols, [entry.as_str()], 0),
        }
    }
    SymbolMove { segment: segment.to_vec(), symbols: symbols.build() }
}

/// The entries of a symbol list: one per line, '#' starting a comment.
fn symbol_list(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect()
}

/// ld64 numeric option arguments are hexadecimal, read as strtoull()
/// reads them (see parse_unsigned), with or without a 0x prefix.
fn hex_number(val: &str) -> Option<u64> {
    parse_unsigned(val, 16)
}

/// The decimal arguments of the options ld64 reads with strtoul().
/// A ThinLTO cache option's number, truncated to the 32 bits libLTO
/// takes.
fn lto_cache_number(name: &str, arg: &OsStr) -> i32 {
    let value = arg.to_str().and_then(decimal_number);
    value.unwrap_or_else(|| fatal!("invalid argument for {name}")) as i32
}

fn decimal_number(val: &str) -> Option<u64> {
    parse_unsigned(val, 10)
}

/// A number as strtoull() reads it in base 10 or 16: after white space
/// and a sign, in hexadecimal with or without a 0x prefix. One too big
/// is the largest there is, and a negative one wraps around; anything
/// left over makes it no number.
fn parse_unsigned(val: &str, radix: u32) -> Option<u64> {
    let val = val.trim_start_matches(|c: char| c.is_ascii() && is_space(c as u8));
    let (negative, val) = match val.as_bytes().first() {
        Some(b'-') => (true, &val[1..]),
        Some(b'+') => (false, &val[1..]),
        _ => (false, val),
    };
    // "0x" is a prefix only before a digit: alone, it is a 0 and an x.
    let digits = match val.strip_prefix("0x").or_else(|| val.strip_prefix("0X")) {
        Some(rest) if radix == 16 && rest.starts_with(|c: char| c.is_ascii_hexdigit()) => rest,
        _ => val,
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    let num = u64::from_str_radix(digits, radix).unwrap_or(u64::MAX);
    Some(if negative && num != u64::MAX { num.wrapping_neg() } else { num })
}

fn parse_hex(opt: &str, val: &str) -> u64 {
    hex_number(val).unwrap_or_else(|| fatal!("{opt}: not a hexadecimal number: {val}"))
}

/// Parses a -segprot protection: the letters r, w and x in either
/// case, and '-' for none. ld-prime warns about any other byte and
/// ignores it, so a non-ASCII letter draws a warning for each of its
/// bytes, which it prints as they are.
fn parse_prot(val: &[u8], warnings: &mut OptionWarnings) -> u8 {
    let mut prot = 0u8;
    for &c in val {
        match c.to_ascii_lowercase() {
            b'r' => prot |= 1,
            b'w' => prot |= 2,
            b'x' => prot |= 4,
            b'-' => {}
            _ => warnings.warn(format_args!("unknown -segprot letter '{}'", raw(&[c]))),
        }
    }
    prot
}

/// A section or segment name an option gives, cut to the 16 bytes of
/// a Mach-O header's name field, as ld-prime silently does for the new
/// names of -rename_section and -rename_segment. (The names they
/// rename from are matched as given, so a longer one matches nothing.)
fn section_name(name: &[u8]) -> Vec<u8> {
    cut_name(name).to_vec()
}

/// A -sectcreate segment or section name, cut to 16 bytes with
/// ld-prime's warning. (-add_empty_section's are cut silently: ld-prime
/// fails an assertion on them.)
fn sectcreate_name(kind: &str, name: &[u8], warnings: &mut OptionWarnings) -> Vec<u8> {
    let cut = section_name(name);
    if cut.len() < name.len() {
        warnings.warn(format_args!(
            "-sectcreate {kind} name too long ('{}'), will be truncated to '{}'",
            raw(name),
            raw(&cut)
        ));
    }
    cut
}

fn is_space(c: u8) -> bool {
    // Same as isspace() in the C locale, without the function call.
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// Splits a response file into arguments, as ld-prime does: at runs of
/// spaces, tabs, newlines and carriage returns (vertical tabs and form
/// feeds are argument bytes). A backslash takes the next byte as it is,
/// and single or double quotes take the bytes up to the matching one,
/// backslashes still escaping - so '' is an empty argument. The text
/// ends at the first NUL byte, ending an open quote or a trailing
/// backslash with it.
fn split_response_file(data: &'static [u8]) -> Vec<Cow<'static, OsStr>> {
    let data = &data[..memchr::memchr(0, data).unwrap_or(data.len())];
    let is_sep = |c: u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r');
    let mut args = Vec::new();
    let mut i = 0;
    loop {
        while i < data.len() && is_sep(data[i]) {
            i += 1;
        }
        if i >= data.len() {
            return args;
        }
        // Plain arguments borrow the file's bytes, which live for the
        // whole link. Copy only when removing quotes or backslashes.
        let start = i;
        while i < data.len() && !is_sep(data[i]) && !matches!(data[i], b'\\' | b'\'' | b'"') {
            i += 1;
        }
        let mut arg = Cow::Borrowed(&data[start..i]);
        while i < data.len() && !is_sep(data[i]) {
            match data[i] {
                b'\\' => {
                    arg.to_mut().extend(data.get(i + 1));
                    i += 2;
                }
                quote @ (b'\'' | b'"') => {
                    i += 1;
                    while i < data.len() && data[i] != quote {
                        if data[i] == b'\\' {
                            i += 1;
                        }
                        arg.to_mut().extend(data.get(i));
                        i += 1;
                    }
                    i += 1;
                }
                c => {
                    arg.to_mut().push(c);
                    i += 1;
                }
            }
        }
        args.push(match arg {
            Cow::Borrowed(bytes) => Cow::Borrowed(os_str(bytes)),
            Cow::Owned(bytes) => Cow::Owned(OsString::from_vec(bytes)),
        });
    }
}

/// Replaces each "@file" argument with the arguments the file holds
/// (see split_response_file), those of a "@file" among them in turn, as
/// ld-prime does. Build systems pass thousands of input files this way,
/// past the kernel's limit on a command line's length. A dylib path
/// starting "@rpath", "@loader_path" or "@executable_path" is no
/// response file; any other "@" argument is, wherever it is, even an
/// option's argument. ld-prime names a file by its real path where it
/// has one, and reads none twice: a second "@" naming one, nested or
/// not, is an error. A file it can't open draws a warning, the argument
/// staying as it is (a file to link, to fail as one); one it can't
/// read, such as a directory, is an error.
pub fn expand_response_files(argv: Vec<OsString>) -> Vec<Cow<'static, OsStr>> {
    let mut args = Vec::new();
    let mut loaded = hashbrown::HashSet::new();
    for arg in argv {
        expand_response_file(Cow::Owned(arg), &mut loaded, &mut args);
    }
    args
}

/// Appends `arg` to `args`, or the arguments of the response file it
/// names (see expand_response_files).
fn expand_response_file(
    arg: Cow<'static, OsStr>,
    loaded: &mut hashbrown::HashSet<PathBuf>,
    args: &mut Vec<Cow<'static, OsStr>>,
) {
    const DYLIB_PATHS: [&[u8]; 3] = [b"@rpath", b"@loader_path", b"@executable_path"];
    let bytes = arg.as_bytes();
    let Some(path) = bytes.strip_prefix(b"@") else {
        args.push(arg);
        return;
    };
    if DYLIB_PATHS.iter().any(|prefix| bytes.starts_with(prefix)) {
        args.push(arg);
        return;
    }
    let path = Path::new(os_str(path));
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if !loaded.insert(path.clone()) {
        fatal!("recursively loading {}", path.display());
    }
    let mut file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(e) => {
            let errno = crate::error::errno_text(&e);
            crate::warn!("response file '{}' could not be opened, {errno}", path.display());
            args.push(arg);
            return;
        }
    };
    let mut data = Vec::new();
    if let Err(e) = std::io::Read::read_to_end(&mut file, &mut data) {
        let errno = crate::error::errno_text(&e);
        fatal!("response file '{}' could not be read, {errno}", path.display());
    }
    for arg in split_response_file(Vec::leak(data)) {
        expand_response_file(arg, loaded, args);
    }
}

/// Reads a -filelist file: one input path per line, in whatever bytes
/// the file system uses, optionally under a directory given after a
/// comma in the option's argument. Returns the file's path and the
/// paths.
fn read_filelist(arg: &OsStr) -> (PathBuf, Vec<PathBuf>) {
    let (path, dir) = match memchr::memchr(b',', arg.as_bytes()) {
        Some(comma) => (
            Path::new(os_str(&arg.as_bytes()[..comma])),
            Some(Path::new(os_str(&arg.as_bytes()[comma + 1..]))),
        ),
        None => (Path::new(arg), None),
    };
    let text = std::fs::read(path).unwrap_or_else(|e| {
        let errno = crate::error::errno_text(&e);
        fatal!("-filelist file '{}' could not be opened, {errno}\n", path.display())
    });
    let files = text
        .split(|&b| b == b'\n')
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
        .filter(|line| !line.is_empty())
        .map(|line| match dir {
            Some(dir) => dir.join(os_str(line)),
            None => PathBuf::from(os_str(line)),
        })
        .collect();
    (path.to_path_buf(), files)
}

/// What option parsing needs to know about the target: the driver
/// parses once per speculated target, as mold does.
pub struct TargetTraits {
    pub name: &'static str,
    pub page_size: u64,
}

/// What the command line links: ld64's output kinds, which ld-prime
/// chooses the same way. The last of -execute, -dylib, -bundle, -r,
/// -preload, -kext and -dylinker names the kind, but -static is a
/// modifier as much as a kind: it makes a static executable of anything
/// but a relocatable object or a kext, which it leaves as they are, and
/// a later -execute leaves a static executable static.
#[derive(Clone, Copy, Default, PartialEq)]
enum OutputKind {
    #[default]
    DynamicExecutable,
    StaticExecutable,
    Dylib,
    Bundle,
    Object,
    Preload,
    Kext,
    Dylinker,
}

/// The warnings ld-prime gives as it reads an option, which only a -w
/// before the option silences (but -fatal_warnings still counts), and
/// its notices, which it prints bare whatever -w and -fatal_warnings
/// say. They are held back until the parse is known to be for the
/// target, so that they are given once, but come before the error a
/// later option runs into (see error::hold).
#[derive(Default)]
struct OptionWarnings {
    quiet: bool,
    hidden: bool,
}

impl OptionWarnings {
    fn warn(&mut self, msg: impl fmt::Display) {
        if self.quiet {
            self.hidden = true;
        } else {
            crate::error::hold(Held::Warning(crate::error::render(format_args!("{msg}"))));
        }
    }

    fn notice(&mut self, msg: impl fmt::Display) {
        crate::error::hold(Held::Notice(crate::error::render(format_args!("{msg}"))));
    }

    /// Gives the messages, once the parse is known to be the last.
    fn print(&self) {
        crate::error::release_held();
        if self.hidden {
            crate::error::hidden_warning();
        }
    }
}

/// What parse_args gathers from the options for the phases after them:
/// the options it resolves into Args only once it knows the target and
/// the kind of output (None for one not given), the symbol lists, and
/// the diagnostics ld-prime gives once it has read every option.
#[derive(Default)]
struct ParseState<'a> {
    kind: OutputKind,
    /// -target's triple, which overrides -arch and -platform_version.
    target_triple: Option<&'a str>,
    /// The first two platforms the options name that cannot go
    /// together (see set_platform).
    incompatible_platforms: Option<(u32, u32)>,
    arch_variant: bool,
    /// -no_allow_dylib_sub_type_mismatches's architectures.
    dylib_subtype_list: Option<&'a [u8]>,
    /// Whether -e names the entry point.
    explicit_entry: bool,
    pie: Option<bool>,
    fixup_chains: Option<bool>,
    /// -fixup_chains_section's kind (see Args::chain_starts_kind), unless
    /// a later -fixup_chains or -no_fixup_chains turned it off.
    chain_starts: Option<u32>,
    rebase_section: bool,
    threaded_starts: bool,
    /// -read_only_relocs: whether its treatment allows text relocations.
    read_only_relocs: Option<bool>,
    unaligned_pointers: Option<Treatment>,
    function_starts: Option<bool>,
    data_in_code_info: Option<bool>,
    /// -add_source_version or -no_source_version, and -source_version's
    /// number.
    source_version: Option<bool>,
    source_version_number: Option<u64>,
    adhoc_codesign: Option<bool>,
    data_const: Option<bool>,
    objc_relative_method_lists: Option<bool>,
    objc_stubs_small: Option<bool>,
    const_selrefs: Option<bool>,
    warn_unused_dylibs: Option<bool>,
    max_default_common_align: Option<u8>,
    headerpad: Option<u64>,
    segalign: Option<u64>,
    segprots: Vec<(Vec<u8>, u8, u8)>,
    seg_page_sizes: Vec<(Vec<u8>, u64)>,
    stack_size: Option<u64>,
    stack_addr: Option<u64>,
    x86_64_layout_emulation: bool,
    lto_libraries: Vec<PathBuf>,
    lto_softload: Option<bool>,
    lists: SymbolLists,
    export_choice: Option<ExportChoice>,
    reexports_listed: bool,
    force_weakness_listed: bool,
    warnings: OptionWarnings,
    /// The warnings about the obsolete options given, which ld-prime
    /// ignores with a warning once it has read them all.
    obsolete: Vec<String>,
    /// The options ld-prime doesn't know, each followed by a space.
    unknown: String,
}

/// The symbol name patterns the options give, which parse_args compiles
/// into Args's matchers once the whole command line is known.
#[derive(Default)]
struct SymbolLists {
    exported_symbols: Option<GlobBuilder>,
    unexported_symbols: GlobBuilder,
    reexported_symbols: GlobBuilder,
    why_live: GlobBuilder,
    local_strip_list: GlobBuilder,
    local_keep_list: Option<GlobBuilder>,
    force_weak: GlobBuilder,
    force_not_weak: GlobBuilder,
    keep_duplicates: GlobBuilder,
    poisoned: GlobBuilder,
    /// -interposable: every symbol, unless an -interposable_list names
    /// some.
    interposable_all: bool,
    interposable_list: Option<GlobBuilder>,
}

/// The error for an option the command line ends before the argument
/// of, as ld-prime words it: what the option needs, in the usage its
/// manual page gives (which -executable_path, obsolete, has not).
pub(crate) fn missing_argument(opt: &str) -> String {
    let usage = match opt {
        "-arch" | "-arch_variant" => "missing <arch>",
        "-no_allow_dylib_sub_type_mismatches" => "missing <arch_list>",
        "-e"
        | "-init"
        | "-u"
        | "-U"
        | "-install_name"
        | "-dylib_install_name"
        | "-dylinker_install_name"
        | "-final_output"
        | "-exported_symbol"
        | "-unexported_symbol"
        | "-sub_library"
        | "-sub_umbrella"
        | "-umbrella"
        | "-allowable_client"
        | "-client_name"
        | "-why_live"
        | "-keep_duplicate"
        | "-poison_symbol" => "missing <name>",
        "-headerpad"
        | "-pagezero_size"
        | "-stack_size"
        | "-segalign"
        | "-branch_island_region_size" => "missing <size>",
        "-image_base" | "-seg1addr" => "missing <address>",
        "-current_version"
        | "-dylib_current_version"
        | "-compatibility_version"
        | "-dylib_compatibility_version"
        | "-macos_version_min"
        | "-source_version"
        | "-ios_version_min"
        | "-maccatalyst_version_min"
        | "-objc_abi_version" => "missing <version>",
        "-mllvm"
        | "-max_code_deduplicate_passes"
        | "-prune_interval_lto"
        | "-prune_after_lto"
        | "-max_relative_cache_size_lto" => "missing <value>",
        "-mcpu" => "missing <cpu>",
        "-trace_implicit_library" => return "-trace_implicit_library_name missing <name>".into(),
        "-add_linker_option" => "missing <options>",
        "-undefined" => "missing <dynamic_lookup>",
        "-dyld_env" => "missing <arg>",
        "-image_suffix" => "missing <suffix>",
        "-weak_reference_mismatches" => "missing [ error | weak | non-weak ]",
        "-max_default_common_align" => "missing <align-value>",
        // (ld-prime names the other list.)
        "-force_symbols_not_weak_list" => {
            return "-force_symbols_weak_list missing <path>".to_string();
        }
        "-read_only_relocs"
        | "-arch_variant_lto_cache_mismatch"
        | "-duplicate_symbols"
        | "-deployment_target_mismatches"
        | "-unaligned_pointers"
        | "-objc_class_ro_signing_mismatch" => "missing <option>",
        "-target" => "missing <target-triple>",
        "-alias" => "missing <real-name> <alias-name>",
        "-dylib_file" => "missing <path:path>",
        "-platform_version" => "missing arguments <platform> <min_version> <sdk_version>",
        "-sectcreate" => "missing arguments <segname> <sectname> <file>",
        "-add_empty_section" => "missing arguments <segname> <sectname>",
        "-segaddr" => "needs <segname> <addr>",
        "-stack_addr" => "requires <address>",
        "-segment_order" => "needs <segment-list>",
        "-sectalign" => "needs <segname> <sectname> <align>",
        "-executable_path" | "-kext_objects_dir" | "-multiply_defined" | "-sdk_version"
        | "-seg_addr_table" | "-Y" => return format!("obsolete option {opt} requires 1 arguments"),
        // Files and directories: inputs, lists, outputs and search paths.
        _ => "missing <path>",
    };
    format!("{opt} {usage}")
}

/// ld-prime's checks of the options that put an image's fixups in a
/// section of its own, for its own loader: -fixup_chains_section, which
/// rules out -rebase_section as -fixup_chains does, and -rebase_section.
/// Only an image no dyld loads can have them (a kext or a -kernel
/// image, which have neither, ignores them), and only a 32-bit one
/// -rebase_section's.
fn check_fixup_sections(
    args: &Args,
    fixup_chains: Option<bool>,
    fixup_chains_section: bool,
    rebase_section: bool,
) {
    if rebase_section && fixup_chains == Some(true) {
        fatal!(
            "-fixup_chains*, -rebase_section and -threaded_starts_section can't be used together"
        );
    }
    let dynamic = !args.static_link && !args.relocatable && !args.is_kext();
    if rebase_section && dynamic {
        fatal!("-rebase_section can't be used with dynamic binaries");
    }
    if fixup_chains_section && dynamic {
        fatal!("-fixup_chains_section* can't be used with dynamic binaries");
    }
    if rebase_section && !args.is_kext() && !args.kernel {
        fatal!("-rebase_section can only be used on 32-bit architectures");
    }
}

/// Adds an -add_linker_option's words to `words`. ld-prime splits the
/// option at its first space only for an option that names a framework
/// (any word with "framework" in it), and takes the rest for its
/// argument, spaces and all; it ignores any other with a space, and
/// passes one without on as a word of its own.
fn add_linker_option(words: &mut Vec<Vec<u8>>, opt: &[u8], warnings: &mut OptionWarnings) {
    let Some(space) = memchr::memchr(b' ', opt) else {
        words.push(opt.to_vec());
        return;
    };
    let (head, arg) = (&opt[..space], &opt[space + 1..]);
    if memchr::memmem::find(head, b"framework").is_some() {
        words.push(head.to_vec());
        words.push(arg.to_vec());
    } else {
        warnings.warn(format!(
            "unknown linker option from -add_linker_option ignored, starting with: '{}'",
            display(head)
        ));
    }
}

/// The command line as parse_args reads it: `index` is the argument it
/// is on, an option or the last argument of one.
struct ArgCursor<'a> {
    args: &'a [Cow<'a, OsStr>],
    index: usize,
}

impl<'a> ArgCursor<'a> {
    /// Moves on to the next argument and returns it, None past the end.
    fn advance(&mut self) -> Option<&'a OsStr> {
        self.index += 1;
        self.args.get(self.index).map(|arg| arg.as_ref())
    }

    /// An option's argument. ld-prime takes an empty one for none, and
    /// the command line ending before it is an error in the words it
    /// has for the option.
    fn next_arg(&mut self, opt: &str) -> &'a OsStr {
        match self.advance() {
            Some(arg) if !arg.is_empty() => arg,
            _ => fatal!("{}", missing_argument(opt)),
        }
    }

    /// An argument that may be empty, as ld-prime takes a few.
    fn arg_or_empty(&mut self, opt: &str) -> &'a OsStr {
        self.advance().unwrap_or_else(|| fatal!("{}", missing_argument(opt)))
    }

    /// An argument that is text by nature.
    fn next_text(&mut self, opt: &str) -> &'a str {
        text(opt, self.next_arg(opt))
    }

    fn next_path(&mut self, opt: &str) -> PathBuf {
        PathBuf::from(self.next_arg(opt))
    }

    fn next_bytes(&mut self, opt: &str) -> Vec<u8> {
        self.next_arg(opt).as_bytes().to_vec()
    }

    /// The entries of the symbol list file an option names.
    fn next_symbol_list(&mut self, opt: &str) -> Vec<String> {
        read_symbol_list(opt, &self.next_path(opt))
    }

    /// An operand of -rename_section or -rename_segment: ld-prime
    /// reports a missing or empty one with the option's usage.
    fn rename_operand(&mut self, opt: &str, usage: &str) -> &'a [u8] {
        match self.advance() {
            Some(arg) if !arg.is_empty() => arg.as_bytes(),
            _ => fatal!("{opt} missing {usage}"),
        }
    }

    /// An operand of -move_to_rw_segment or -move_to_ro_segment:
    /// ld-prime reports a missing or empty one with the option's usage
    /// alone.
    fn move_operand(&mut self, opt: &str) -> &'a OsStr {
        match self.advance() {
            Some(arg) if !arg.is_empty() => arg,
            _ => fatal!("{opt} <segname> <path>"),
        }
    }
}

/// An argument that is text by nature.
fn text<'a>(opt: &str, arg: &'a OsStr) -> &'a str {
    arg.to_str().unwrap_or_else(|| {
        fatal!("option {opt}: expected a UTF-8 argument: {}", display(arg.as_bytes()))
    })
}

/// -platform_version <platform> <min_version> <sdk_version>: ld-prime
/// takes all three before it reads any.
fn read_platform_version(cur: &mut ArgCursor, args: &mut Args, st: &mut ParseState, opt: &str) {
    let platform = cur.next_text(opt);
    let minos = cur.next_text(opt);
    let sdk = cur.next_text(opt);
    let platform = parse_platform(platform);
    let minos = parse_version(opt, minos);
    let sdk = parse_version(opt, sdk);
    set_platform(args, st, platform, minos);
    args.platform_sdk = sdk;
}

/// -macos_version_min <version>, the old pre-LC_BUILD_VERSION way of
/// stating the deployment target, still emitted by clang for older
/// -mmacosx-version-min targets. It fixes the platform to macOS; ld64
/// records the SDK as the same version (the flag carries no separate
/// SDK). ld-prime notes each use of the old spelling,
/// -macosx_version_min, and reports on the option as the new.
fn read_macos_version_min(cur: &mut ArgCursor, args: &mut Args, st: &mut ParseState, name: &str) {
    if name == "-macosx_version_min" {
        st.warnings.notice("-macosx_version_min has been renamed to -macos_version_min");
    }
    let opt = "-macos_version_min";
    let minos = parse_version(opt, cur.next_text(opt));
    set_platform(args, st, PLATFORM_MACOS, minos);
    args.platform_sdk = minos;
}

/// -ios_version_min and -maccatalyst_version_min, which ld-prime still
/// takes, under their old names too, as it does -macosx_version_min.
/// mold links for neither, as -platform_version ios says.
fn read_other_version_min(cur: &mut ArgCursor, st: &mut ParseState, name: &str) -> ! {
    let (opt, platform) = match name {
        "-ios_version_min" | "-iphoneos_version_min" => ("-ios_version_min", PLATFORM_IOS),
        _ => ("-maccatalyst_version_min", PLATFORM_MACCATALYST),
    };
    if name != opt {
        st.warnings.notice(format!("{name} has been renamed to {opt}"));
    }
    parse_version(opt, cur.next_text(opt));
    fatal!("unsupported platform: {}", platform_name(platform));
}

/// -bundle_loader <executable>: the last one counts; ld-prime reads no
/// other.
fn read_bundle_loader(
    cur: &mut ArgCursor,
    args: &mut Args,
    warnings: &mut OptionWarnings,
    opt: &str,
) {
    let loader = |arg: &InputArg| matches!(arg, InputArg::BundleLoader(_));
    if let Some(pos) = args.inputs.iter().position(loader)
        && let InputArg::BundleLoader(old) = args.inputs.remove(pos)
    {
        let old = old.display();
        warnings.warn(format!("duplicate -bundle_loader option, '{old}' ignored"));
    }
    args.inputs.push(InputArg::BundleLoader(cur.next_path(opt)))
}

/// -dylib_file <install_name>:<path>, which ld-prime deprecates, once,
/// as it reads it.
fn add_dylib_file(args: &mut Args, warnings: &mut OptionWarnings, arg: &[u8]) {
    let Some(colon) = memchr::memchr(b':', arg) else {
        fatal!("-dylib_file malformed <path:path>");
    };
    if args.dylib_files.is_empty() {
        warnings.warn(
            "-dylib_file is deprecated. Use -F or -L to control where indirect dylibs are found",
        );
    }
    let file = PathBuf::from(os_str(&arg[colon + 1..]));
    args.dylib_files.push((arg[..colon].to_vec(), file));
}

/// -segprot <segment> <max-prot> <init-prot>.
fn read_segprot(cur: &mut ArgCursor, st: &mut ParseState) {
    // ld-prime takes a missing argument for an empty one.
    let mut arg = || cur.advance().map_or(&b""[..], |arg| arg.as_bytes());
    let (seg, max, init) = (arg(), arg(), arg());
    if seg.is_empty() || max.is_empty() || init.is_empty() {
        fatal!("-segprot missing <seg> <max-prot> <init-prot>");
    }
    let seg = seg.to_vec();
    // __LINKEDIT, which dyld reads, keeps its own.
    if seg == b"__LINKEDIT" {
        st.warnings.warn("-segprot cannot be used to modify __LINKEDIT protections");
    } else {
        let max = parse_prot(max, &mut st.warnings);
        let init = parse_prot(init, &mut st.warnings);
        st.segprots.push((seg, max, init));
    }
}

/// -segment_order <segment>:<segment>..., which ld-prime refuses after
/// one that names any segment.
fn read_segment_order(cur: &mut ArgCursor, args: &mut Args, opt: &str) {
    if !args.segment_order.is_empty() {
        fatal!("-segment_order used more than once");
    }
    args.segment_order = cur
        .next_arg(opt)
        .as_bytes()
        .split(|&c| c == b':')
        .filter(|s| !s.is_empty())
        .map(<[u8]>::to_vec)
        .collect();
}

/// -seg_page_size <segment> <size>.
fn read_seg_page_size(cur: &mut ArgCursor, st: &mut ParseState, opt: &str) {
    let (Some(seg), Some(size)) = (cur.advance(), cur.advance()) else {
        fatal!("-seg_page_size needs <segname> <size>");
    };
    if seg.is_empty() || size.is_empty() {
        fatal!("-seg_page_size needs <segname> <size>");
    }
    let size = parse_hex(opt, text(opt, size));
    if size > u32::MAX as u64 {
        fatal!("-seg_page_size {size}: size too big");
    }
    st.seg_page_sizes.push((seg.as_bytes().to_vec(), size));
}

/// -section_order <segment> <section>:<section>..., once per segment.
fn read_section_order(cur: &mut ArgCursor, args: &mut Args) {
    let (Some(seg), Some(list)) = (cur.advance(), cur.advance()) else {
        fatal!("-section_order needs <segname> <section-list>");
    };
    if seg.is_empty() || list.is_empty() {
        fatal!("-section_order needs <segname> <section-list>");
    }
    let seg = seg.as_bytes().to_vec();
    let list: Vec<Vec<u8>> = (list.as_bytes().split(|&c| c == b':'))
        .filter(|s| !s.is_empty())
        .map(<[u8]>::to_vec)
        .collect();
    if list.is_empty() {
        fatal!("-section_order should specifify at least one section");
    }
    if args.section_order.iter().any(|(s, _)| *s == seg) {
        fatal!("-section_order {} used more than once", raw(&seg));
    }
    args.section_order.push((seg, list));
}

/// -sectcreate <segment> <section> <file>.
fn read_sectcreate(cur: &mut ArgCursor, args: &mut Args, warnings: &mut OptionWarnings, opt: &str) {
    let seg = cur.next_arg(opt).as_bytes();
    let seg = sectcreate_name("segment", seg, warnings);
    let sect = cur.next_arg(opt).as_bytes();
    let sect = sectcreate_name("section", sect, warnings);
    let file = cur.next_path(opt);
    args.sectcreate.push(SectCreate {
        segname: seg,
        sectname: sect,
        path: Some(file),
        position: args.inputs.len(),
    });
}

/// -add_empty_section <segment> <section>.
fn read_add_empty_section(cur: &mut ArgCursor, args: &mut Args, opt: &str) {
    let seg = section_name(cur.next_arg(opt).as_bytes());
    let sect = section_name(cur.next_arg(opt).as_bytes());
    args.sectcreate.push(SectCreate {
        segname: seg,
        sectname: sect,
        path: None,
        position: args.inputs.len(),
    });
}

/// -sectalign <segment> <section> <align>. ld64 takes the largest power
/// of two that divides the alignment (1 for 0), and the first
/// -sectalign given for a section.
fn read_sectalign(cur: &mut ArgCursor, args: &mut Args, warnings: &mut OptionWarnings, opt: &str) {
    let seg = cur.next_bytes(opt);
    let sect = cur.next_bytes(opt);
    let align = parse_hex(opt, cur.next_text(opt));
    if align > u32::MAX as u64 {
        fatal!("-sectalign {align}: alignment too big");
    }
    let p2align = if align == 0 { 0 } else { align.trailing_zeros() as u8 };
    if !align.is_power_of_two() {
        warnings.warn(format_args!(
            "alignment for -sectalign {} {} is not a power of two, using 0x{:X}",
            raw(&seg),
            raw(&sect),
            1u64 << p2align
        ));
    }
    if !args.sectalign.iter().any(|(s1, s2, _)| *s1 == seg && *s2 == sect) {
        args.sectalign.push((seg, sect, p2align));
    }
}

/// -max_default_common_align's alignment, as a power of two: a
/// hexadecimal power of two up to 0x8000. ld-prime takes 0 for 1 and
/// anything else for the power of two below it, with a warning.
fn parse_common_align(arg: &str, warnings: &mut OptionWarnings) -> u8 {
    let Some(align) = hex_number(arg) else {
        fatal!("-max_default_common_align must specify an integer size");
    };
    if align > 0x8000 {
        fatal!(
            "argument for -max_default_common_align ({align:#x}) must be less than or equal to 0x8000"
        );
    }
    if align == 0 {
        warnings.warn("zero is not a valid -max_default_common_align");
    } else if !align.is_power_of_two() {
        warnings.warn(format!(
            "alignment for -max_default_common_align is not a power of two, using {:#x}",
            1u64 << align.ilog2()
        ));
    }
    align.max(1).ilog2() as u8
}

/// Reads an -alias_list file: an existing symbol's name and its alias's
/// on each line, '#' starting a comment. ld64 links on without the
/// aliases of a file it can't read, warning in the words it uses for
/// an order file.
fn read_alias_list(
    list: &Path,
    aliases: &mut Vec<(String, String)>,
    warnings: &mut OptionWarnings,
) {
    let contents = match std::fs::read_to_string(list) {
        Ok(contents) => contents,
        Err(e) => {
            let errno = crate::error::errno_text(&e);
            warnings.warn(format!("order file '{}' could not be opened, {errno}", list.display()));
            String::new()
        }
    };
    for line in contents.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut it = line.split_whitespace();
        match (it.next(), it.next()) {
            (Some(existing), Some(new)) => aliases.push((existing.to_string(), new.to_string())),
            _ => fatal!("malformed -alias_list line: {line}"),
        }
    }
}

/// An option no other arm of parse_args names: one with its argument
/// joined to its name (-lfoo, -weak-lfoo, -L<dir>, -F<dir>),
/// -debug_snapshot with its mode, an optimization level, or one ld-prime
/// doesn't know, which it reports with the others once it has read them
/// all (see finish_options).
fn read_joined_option(
    cur: &mut ArgCursor,
    args: &mut Args,
    st: &mut ParseState,
    raw: &[u8],
    name: &str,
) {
    if let Some(&(prefix, kind)) =
        JOINED_LIBRARY_OPTIONS.iter().find(|(prefix, _)| raw.starts_with(prefix.as_bytes()))
    {
        // An option with no name joined to it takes the next argument
        // for one, as ld-prime does: -weak-l foo is -weak-lfoo.
        let lib = match &raw[prefix.len()..] {
            [] => cur.next_arg(prefix),
            lib => os_str(lib),
        };
        let lib = LibraryName::Lib(lib.to_owned());
        args.inputs.push(InputArg::Library(kind, lib));
    } else if let Some(dir) = raw.strip_prefix(b"-L") {
        args.library_paths.push(PathBuf::from(os_str(dir)));
    } else if let Some(dir) = raw.strip_prefix(b"-F") {
        args.framework_paths.push(PathBuf::from(os_str(dir)));
    } else if let Some(mode) = raw.strip_prefix(b"-debug_snapshot") {
        // A link snapshot (see -snapshot_dir) in a mode after the name,
        // or after a '='.
        let mode = mode.strip_prefix(b"=").unwrap_or(mode);
        if !matches!(mode, b"" | b"minimal") {
            fatal!("unknown debug snapshot mode: {}", display(mode));
        }
    } else if raw.starts_with(b"-O") {
        // An optimization level, which clang passes on from its own
        // command line (-O2, -Ofast, -Og, ...). ld-prime takes -O
        // followed by anything; it only switches function
        // deduplication, which here is on unless -no_deduplicate,
        // whatever the level.
    } else {
        st.unknown.push_str(name);
        st.unknown.push(' ');
    }
}

/// A file the command line names to link. ld-prime takes a path to an
/// archive on the command line, though not in a -filelist, for a
/// library option's (see LibraryKind::Plain).
fn input_file(path: &OsStr) -> InputArg {
    if path.as_bytes().ends_with(b".a") {
        InputArg::Library(LibraryKind::Plain, LibraryName::Path(PathBuf::from(path)))
    } else {
        InputArg::File(PathBuf::from(path))
    }
}

/// Parses all options. `cmdline` includes the program name.
///
/// Options are matched as bytes and their arguments keep the bytes they
/// were given in: paths, install names and rpaths pass through to the
/// file system and the load commands unchanged. Arguments that are text
/// by nature (symbol and section names, versions, the -undefined
/// treatment) must be UTF-8.
pub fn parse_args(target: &TargetTraits, cmdline: &[Cow<'_, OsStr>]) -> Args {
    let mut args = Args {
        zero_ar_date: std::env::var_os("ZERO_AR_DATE").is_some(),
        warn_commons: std::env::var_os("LD_WARN_COMMONS").is_some(),
        warn_swift_abi_mismatches: std::env::var_os("LD_WARN_ON_SWIFT_ABI_VERSION_MISMATCHES")
            .is_some(),
        prefer_stubs: std::env::var_os("LD_PREFER_TAPI_FILE").is_some(),
        order_file_statistics: std::env::var_os("LD_PRINT_ORDER_FILE_STATISTICS").is_some(),
        fatal_warnings: std::env::var_os("LD_TREAT_WARNINGS_AS_ERRORS").is_some_and(|v| v != "0"),
        application_extension: ["LD_APPLICATION_EXTENSION_SAFE", "LD_NO_ENCRYPT"]
            .iter()
            .any(|var| std::env::var_os(var).is_some()),
        uuid_salt: std::env::var_os("RC_UUID_SALT").map_or(Vec::new(), |s| s.as_bytes().to_vec()),
        ..Default::default()
    };
    let mut st = ParseState::default();

    crate::error::set_color(std::io::stderr().is_terminal());

    let mut cur = ArgCursor { args: cmdline, index: 0 };
    while let Some(opt) = cur.advance() {
        // Every option name is ASCII; an unknown one is reported lossily.
        let name = opt.to_string_lossy();
        let name: &str = &name;
        match opt.as_bytes() {
            b"-o" => args.output = cur.next_path(name),
            b"-arch" => {
                let arch = cur.next_text(name);
                args.arch =
                    Some(target_arch(arch).unwrap_or_else(|| fatal!("unknown -arch name: {arch}")));
            }
            b"-target" => st.target_triple = Some(cur.next_text(name)),
            b"-e" => {
                args.entry = cur.next_text(name).to_string();
                st.explicit_entry = true;
            }
            b"-platform_version" => read_platform_version(&mut cur, &mut args, &mut st, name),
            b"-syslibroot" => args.syslibroot.push(cur.next_path(name)),
            b"-L" => args.library_paths.push(cur.next_path(name)),
            raw if let Some((kind, naming)) = library_option(raw) => {
                args.inputs.push(InputArg::Library(kind, naming(cur.next_arg(name))));
            }
            b"-no_merged_libraries_hook" => args.merged_libraries_hook = false,
            b"-make_mergeable" => args.make_mergeable = true,
            b"-add_mergeable_debug_hook" => args.add_mergeable_debug_hook = true,
            b"-filelist" => {
                let (list, files) = read_filelist(cur.next_arg(name));
                args.inputs.extend(files.into_iter().map(InputArg::Listed));
                args.filelists.push(list);
            }
            b"-F" => args.framework_paths.push(cur.next_path(name)),
            b"-execute" => {
                if st.kind != OutputKind::StaticExecutable {
                    st.kind = OutputKind::DynamicExecutable;
                }
            }
            b"-dylib" => st.kind = OutputKind::Dylib,
            b"-bundle" => st.kind = OutputKind::Bundle,
            b"-kext" => st.kind = OutputKind::Kext,
            b"-dylinker" => st.kind = OutputKind::Dylinker,
            b"-bundle_loader" => read_bundle_loader(&mut cur, &mut args, &mut st.warnings, name),
            b"-final_output" => args.final_output = Some(cur.next_bytes(name)),
            b"-keep_private_externs" => args.keep_private_externs = true,
            // ld-prime only warns about a missing path, or an empty
            // one or an option's name, which it takes all the same.
            b"-rpath" => match cur.advance() {
                Some(arg) if !arg.is_empty() && !arg.as_bytes().starts_with(b"-") => {
                    args.rpaths.push(arg.as_bytes().to_vec());
                }
                _ => st.warnings.warn("-rpath missing <path>"),
            },
            // (dyld's own LC_ID_DYLINKER names /usr/lib/dyld, whatever
            // -dylinker_install_name says.)
            b"-install_name" | b"-dylib_install_name" | b"-dylinker_install_name" => {
                args.install_name = Some(cur.next_bytes(name))
            }
            b"-map" => args.map = Some(cur.next_path(name)),
            b"-sdk_imports" => args.sdk_imports = Some(cur.next_path(name)),
            // ld-prime reads the list as it reads the option.
            b"-sdk_imports_api_list" => {
                let list = crate::api_list::read(&cur.next_path(name));
                args.sdk_imports_api_list = Some(list);
            }
            b"-fixup_chains" | b"-no_fixup_chains" => {
                st.fixup_chains = Some(name == "-fixup_chains");
                st.chain_starts = None;
            }
            // The last of these and -fixup_chains or -no_fixup_chains
            // counts, but ld-prime refuses to switch from one kind of
            // chain starts to the other.
            b"-fixup_chains_section" | b"-fixup_chains_section_vm" => {
                let kind = if name == "-fixup_chains_section" { 1 } else { 2 };
                if st.chain_starts.is_some_and(|k| k != kind) {
                    fatal!(
                        "{name} can't be used together with other -fixup_chains_section* options"
                    );
                }
                st.fixup_chains = Some(true);
                st.chain_starts = Some(kind);
            }
            // A 32-bit image's rebases in a section, which no image of
            // the 64-bit targets mold has can have.
            b"-rebase_section" => st.rebase_section = true,
            b"-threaded_starts_section" => st.threaded_starts = true,
            b"-adhoc_codesign" => st.adhoc_codesign = Some(true),
            b"-no_adhoc_codesign" => st.adhoc_codesign = Some(false),
            b"-dynamic" => args.dynamic = true,
            b"-static" => {
                if !matches!(st.kind, OutputKind::Object | OutputKind::Kext) {
                    st.kind = OutputKind::StaticExecutable;
                }
            }
            b"-preload" => st.kind = OutputKind::Preload,
            b"-kernel" => args.kernel = true,
            b"-version_load_command" => args.version_load_command = true,
            // The last one wins; ld-prime warns as it reads one that
            // turns the other around.
            b"-pie" | b"-no_pie" => {
                let on = name == "-pie";
                if st.pie == Some(!on) {
                    let other = if on { "-no_pie" } else { "-pie" };
                    st.warnings.warn(format!("{name} overriding previous {other}"));
                }
                st.pie = Some(on);
            }
            // Given with -dead_strip, this once kept initializers and
            // terminators nothing referenced. -dead_strip always keeps
            // them now, and ld64 takes this for -dead_strip alone.
            b"-no_dead_strip_inits_and_terms" => {
                args.dead_strip = true;
                st.warnings.warn(
                    "option '-no_dead_strip_inits_and_terms' is obsolete, use '-dead_strip' instead",
                );
            }
            b"-headerpad" => {
                let size = parse_hex(name, cur.next_text(name));
                if size > u32::MAX as u64 {
                    fatal!("-headerpad size too large");
                }
                st.headerpad = Some(size);
            }
            // ld-prime spaces its branch island clusters by this size;
            // mold places range-extension thunks by each branch's reach
            // instead (see thunks.rs), so the size is checked and unused.
            b"-branch_island_region_size" => {
                if hex_number(cur.next_text(name)).is_none() {
                    fatal!("{name} must specify a hexadecimal size");
                }
            }
            b"-pagezero_size" => {
                args.pagezero_size = parse_hex(name, cur.next_text(name));
                args.explicit_pagezero = true;
            }
            b"-image_base" | b"-seg1addr" => {
                args.image_base = Some(parse_hex(name, cur.next_text(name)));
            }
            b"-segaddr" => {
                let seg = cur.next_bytes(name);
                let addr = parse_hex(name, cur.next_text(name));
                args.segaddrs.push((seg, addr));
            }
            b"-segprot" => read_segprot(&mut cur, &mut st),
            b"-segment_order" => read_segment_order(&mut cur, &mut args, name),
            b"-seg_page_size" => read_seg_page_size(&mut cur, &mut st, name),
            b"-segalign" => {
                let align = parse_hex(name, cur.next_text(name));
                if align > u32::MAX as u64 {
                    fatal!("-segalign {align}: alignemnt too big");
                }
                st.segalign = Some(align);
            }
            b"-no_zero_fill_sections" => args.no_zero_fill_sections = true,
            b"-no_warn_reduced_section_align" => args.warn_reduced_section_align = false,
            b"-section_order" => read_section_order(&mut cur, &mut args),
            b"-rename_section" => {
                let usage = "<from-segment> <from-section> <to-segment> <to-section>";
                let old_seg = cur.rename_operand(name, usage).to_vec();
                let old_sect = cur.rename_operand(name, usage).to_vec();
                let new_seg = section_name(cur.rename_operand(name, usage));
                let new_sect = section_name(cur.rename_operand(name, usage));
                args.rename_sections.push((old_seg, old_sect, new_seg, new_sect));
            }
            b"-rename_segment" => {
                let usage = "<from-segment> <to-segment>";
                let old = cur.rename_operand(name, usage).to_vec();
                let new = section_name(cur.rename_operand(name, usage));
                args.rename_segments.push((old, new));
            }
            b"-move_to_rw_segment" | b"-move_to_ro_segment" => {
                let segment = cur.move_operand(name).as_bytes();
                let list = symbol_move(name, segment, Path::new(cur.move_operand(name)));
                match name {
                    "-move_to_rw_segment" => args.move_to_rw.push(list),
                    _ => args.move_to_ro.push(list),
                }
            }
            // ld-prime's error about a list it can't open names no option.
            b"-dirty_data_list" => {
                let list = symbol_move("", b"__DATA_DIRTY", &cur.next_path(name));
                args.dirty_data.push(list);
            }
            b"-stack_size" => {
                let size = hex_number(cur.next_text(name));
                st.stack_size = Some(
                    size.unwrap_or_else(|| fatal!("-stack_size must specify an integer size")),
                );
            }
            b"-stack_addr" => {
                let addr = hex_number(cur.next_text(name));
                st.stack_addr = Some(
                    addr.unwrap_or_else(|| fatal!("-stack_addr must specify an integer address")),
                );
            }
            b"-sectcreate" => read_sectcreate(&mut cur, &mut args, &mut st.warnings, name),
            b"-add_empty_section" => read_add_empty_section(&mut cur, &mut args, name),
            b"-x" => args.strip_locals = true,
            b"-Z" => args.no_standard_dirs = true,
            b"-r" => st.kind = OutputKind::Object,
            b"-flat_namespace" => args.flat_namespace = true,
            b"-twolevel_namespace" => args.flat_namespace = false,
            // ld64 made an executable bind its dylibs' imports flat too
            // (MH_FORCE_FLAT); ld-prime takes it for -flat_namespace.
            b"-force_flat_namespace" => {
                st.warnings.warn(
                    "-force_flat_namespace is no longer supported, using -flat_namespace instead",
                );
                args.flat_namespace = true;
            }
            // How relocations in read-only segments are treated:
            // warning and suppress allow them (ld-prime prints no
            // warning either way), error refuses them. See
            // resolve_text_relocs.
            b"-read_only_relocs" => {
                let treatment = parse_treatment(name, cur.next_arg(name), true);
                st.read_only_relocs = Some(treatment != Treatment::Error);
            }
            // ld-prime knows one treatment besides the default error:
            // dynamic_lookup, which suppress selects too. It deprecates
            // every other one (error, warning or anything else) and
            // ignores it, so none undoes an earlier dynamic_lookup.
            b"-undefined" => {
                let treatment = cur.next_text(name);
                if matches!(treatment, "dynamic_lookup" | "suppress") {
                    args.undefined_dynamic_lookup = true;
                }
                if treatment != "dynamic_lookup" {
                    st.warnings.warn(format!("-undefined {treatment} is deprecated"));
                }
            }
            b"-U" => args.allowed_undefined.push(cur.next_text(name).to_string()),
            b"-w" => {
                args.suppress_warnings = true;
                st.warnings.quiet = true;
            }
            b"-fatal_warnings" => args.fatal_warnings = true,
            b"-demangle" => args.demangle = true,
            b"-help" => {
                println!("Usage: ld64.mold [options] file...");
                crate::error::exit_after_cleanup(0);
            }

            b"-dead_strip" => args.dead_strip = true,
            b"-dead_strip_dylibs" => args.dead_strip_dylibs = true,
            b"-warn_unused_dylibs" => st.warn_unused_dylibs = Some(true),
            b"-no_warn_unused_dylibs" => st.warn_unused_dylibs = Some(false),
            b"-not_for_dyld_shared_cache" => args.not_for_dyld_shared_cache = true,
            b"-debug_variant" => args.debug_variant = true,
            b"-no_inits" => args.no_inits = true,
            b"-no_warn_inits" => args.no_warn_inits = true,
            b"-no_compact_unwind" => args.no_compact_unwind = true,
            b"-bind_at_load" => args.bind_at_load = true,
            b"-application_extension" => args.application_extension = true,
            b"-no_application_extension" => args.application_extension = false,
            b"-simulator_support" => args.simulator_support = true,
            b"-add_ast_path" => args.add_ast_paths.push(cur.next_path(name)),
            b"-S" => args.strip_debug = true,
            b"-all_load" => args.all_load = true,
            b"-u" => args.forced_undefined.push(cur.next_text(name).to_string()),
            b"-exported_symbol" => {
                check_export_choice(&mut st.export_choice, ExportChoice::Exported, name);
                let pat = cur.next_text(name);
                add_initial_undefines(&mut args.forced_undefined, [pat]);
                add_patterns(st.lists.exported_symbols.get_or_insert_default(), [pat], 0);
            }
            b"-no_exported_symbols" => {
                check_export_choice(&mut st.export_choice, ExportChoice::None, name);
                args.no_exported_symbols = true;
            }
            b"-exported_symbols_list" => {
                check_export_choice(&mut st.export_choice, ExportChoice::Exported, name);
                let names = cur.next_symbol_list(name);
                add_initial_undefines(&mut args.forced_undefined, &names);
                add_patterns(st.lists.exported_symbols.get_or_insert_default(), &names, 0);
            }
            b"-unexported_symbol" => {
                check_export_choice(&mut st.export_choice, ExportChoice::Unexported, name);
                add_patterns(&mut st.lists.unexported_symbols, [cur.next_text(name)], 0)
            }
            b"-unexported_symbols_list" => {
                check_export_choice(&mut st.export_choice, ExportChoice::Unexported, name);
                add_patterns(&mut st.lists.unexported_symbols, cur.next_symbol_list(name), 0);
            }
            b"-reexported_symbols_list" => {
                st.reexports_listed = true;
                let names = cur.next_symbol_list(name);
                // Exact names force a reference even if no object
                // mentions them, so one nothing defines is reported as
                // wanted by ld-prime's "<initial-undefines>", as a -u
                // name is. Patterns only match existing symbols.
                add_initial_undefines(&mut args.forced_undefined, &names);
                add_patterns(&mut st.lists.reexported_symbols, &names, 0);
            }
            // The -dylib_ spellings are the older names ld64 still
            // accepts; Xcode passes -dylib_compatibility_version.
            b"-current_version" | b"-dylib_current_version" => {
                let version = text(name, cur.arg_or_empty(name));
                args.current_version = parse_dylib_version(name, version, &mut st.warnings);
            }
            b"-compatibility_version" | b"-dylib_compatibility_version" => {
                let version = text(name, cur.arg_or_empty(name));
                args.compatibility_version = parse_dylib_version(name, version, &mut st.warnings);
            }
            b"-v" => args.verbose = true,
            // Xcode's build system runs `ld -version_details` before the
            // first link and refuses to build if the output is not JSON.
            // It decodes two keys: "version", an ld64 version it compares
            // against thresholds to decide which flags to pass (e.g.
            // -sdk_imports needs 1164), and "architectures". Apple's ld
            // also reports its LTO and TAPI versions; they are ignored.
            // We claim the ld64 version whose command line we implement
            // so that Xcode drives us exactly as it drives ld-prime.
            // With something to link, ld-prime goes on to link it.
            b"-version_details" => args.version_details = true,
            b"-noall_load" => args.all_load = false,
            b"-ObjC" => args.load_objc = true,

            // The default library search behavior already matches
            // -search_paths_first: each path is tried for both a dylib
            // and an archive before moving to the next.
            b"-search_paths_first" => args.search_dylibs_first = false,
            b"-search_dylibs_first" => args.search_dylibs_first = true,
            b"-search_in_sparse_frameworks" => args.search_in_sparse_frameworks = true,
            b"-umbrella" => args.umbrella = Some(cur.next_bytes(name)),
            b"-dylib_file" => {
                add_dylib_file(&mut args, &mut st.warnings, cur.next_arg(name).as_bytes());
            }
            // ld-prime takes this one without its argument.
            b"-oso_prefix" => {
                if let Some(arg) = cur.advance() {
                    args.oso_prefix = Some(arg.as_bytes().to_vec());
                }
            }
            // ld64 set MH_DEAD_STRIPPABLE_DYLIB for this, asking the
            // linker of a client to drop the dylib's load command if it
            // bound nothing from it. ld-prime neither sets nor honors
            // the flag.
            b"-mark_dead_strippable_dylib" => st.obsolete.push(format!("{name} is obsolete")),
            b"-export_dynamic" => args.export_dynamic = true,
            b"-order_file" => args.order_files.push(cur.next_path(name)),
            b"-order_file_statistics" => args.order_file_statistics = true,
            b"--print-dependencies" => args.print_dependencies = true,
            b"-why_load" | b"-whyload" => args.why_load = true,
            b"-why_live" => add_patterns(&mut st.lists.why_live, [cur.next_text(name)], 0),
            b"-allowable_client" => args.allowable_clients.push(cur.next_bytes(name)),
            b"-client_name" => args.client_name = Some(cur.next_bytes(name)),
            b"-t" => args.trace = true,
            b"-trace_symbol_layout" => args.trace_symbol_layout = true,
            b"-trace_symbol_layout_file" => {
                args.trace_symbol_layout_file = Some(cur.next_path(name))
            }
            b"-trace_implicit_libraries" => args.trace_implicit_libraries = true,
            b"-trace_file" => args.trace_file = Some(cur.next_path(name)),
            b"-trace_file_shared_cache" => {
                args.trace_file_shared_cache = Some(cur.next_path(name));
            }
            b"-trace_symbols_file" => args.trace_symbols_file = Some(cur.next_path(name)),
            b"-trace_implicit_library" => {
                args.trace_implicit_library.push(cur.next_bytes(name));
            }
            // ld-prime's reports on its own workings that mold does not
            // give: the branch islands it inserts, a snapshot of the link to
            // replay it from (in /tmp unless -snapshot_dir says; a
            // replay passes -no_snapshot not to take another), the graph
            // of its subsections for Graphviz, and its output compared to
            // a reference one. -arch_multiple
            // once named the architecture in ld64's messages, for a link
            // that is one of several.
            b"-verbose_branch_islands" | b"-no_snapshot" | b"-arch_multiple" => {}
            b"-snapshot_dir" | b"-dot" | b"-reference_output" => {
                cur.next_arg(name);
            }
            b"-no_warn_eh_frame_too_large" => args.warn_eh_frame_too_large = false,
            // ld-prime ignores this with a warning for another target
            // than arm64, and changes nothing seen in an arm64 image.
            b"-x86_64_layout_emulation" => st.x86_64_layout_emulation = true,
            b"-arch_errors_fatal" => args.arch_errors_fatal = true,
            b"-allow_sub_type_mismatches" => args.allow_sub_type_mismatches = true,
            b"-no_allow_dylib_sub_type_mismatches" => {
                st.dylib_subtype_list = Some(cur.next_arg(name).as_bytes());
            }
            // An architecture's variant (of arm64e's pointer
            // authentication ABI), which no -arch mold links for has.
            b"-arch_variant" => {
                let variant = cur.next_arg(name).as_bytes();
                if arch_cpu_family(variant).is_none() {
                    fatal!("unknown -arch name: {}", display(variant));
                }
                st.arch_variant = true;
            }
            b"-ignore_optimization_hints" => args.ignore_optimization_hints = true,
            b"-print_statistics" => args.perf = true,
            b"-warn_duplicate_libraries" => args.warn_duplicate_libraries = true,
            b"-no_warn_duplicate_libraries" => args.warn_duplicate_libraries = false,
            b"-non_global_symbols_strip_list" => {
                add_patterns(&mut st.lists.local_strip_list, cur.next_symbol_list(name), 0);
            }
            b"-non_global_symbols_no_strip_list" => {
                let names = cur.next_symbol_list(name);
                add_patterns(st.lists.local_keep_list.get_or_insert_default(), &names, 0);
            }
            b"-sectalign" => read_sectalign(&mut cur, &mut args, &mut st.warnings, name),
            b"-alias" => {
                let existing = cur.next_text(name).to_string();
                let new = cur.next_text(name).to_string();
                args.aliases.push((existing, new));
            }
            b"-alias_list" => {
                read_alias_list(&cur.next_path(name), &mut args.aliases, &mut st.warnings);
            }
            // ld64 took what @executable_path stands for in a dylib's
            // re-exports from this. ld-prime expands none, and ignores
            // the option with a warning.
            b"-executable_path" => {
                cur.arg_or_empty(name);
                st.obsolete.push(format!("{name} is obsolete"));
            }
            // What ld64 and its predecessors took for prebinding, the
            // two-level namespace hints, multiple modules, Objective-C
            // garbage collection, kext object files and the classic ld's
            // -X, -m, -b and -Sp: ld-prime ignores them all, with a
            // warning once it has read every option. The other old
            // symbol stripping flags, -s, -Si and -Sn, it warns about as
            // it reads them.
            b"-allow_simulator_linking_to_macosx_dylibs"
            | b"-b"
            | b"-m"
            | b"-M"
            | b"-new_linker"
            | b"-no_arch_warnings"
            | b"-no_kext_objects"
            | b"-no_new_main"
            | b"-nomultidefs"
            | b"-objc_gc"
            | b"-objc_gc_compaction"
            | b"-objc_gc_only"
            | b"-prebind"
            | b"-single_module"
            | b"-Sp"
            | b"-twolevel_namespace_hints"
            | b"-X" => st.obsolete.push(format!("{name} is obsolete")),
            b"-kext_objects_dir" | b"-multiply_defined" | b"-sdk_version" | b"-seg_addr_table"
            | b"-Y" => {
                cur.arg_or_empty(name);
                st.obsolete.push(format!("{name} is obsolete"));
            }
            b"-s" | b"-Si" | b"-Sn" => st.warnings.warn(format!("{name} is obsolete")),
            // Bitcode bundles went with Xcode 14, and ld-prime ignores
            // the options that asked for one, as it does -ld_classic,
            // which once picked ld64 over it. -ld_new picks ld-prime,
            // which -ld_prime still does with a warning.
            b"-bitcode_bundle"
            | b"-bitcode_hide_symbols"
            | b"-bitcode_process_mode"
            | b"-bitcode_symbol_map"
            | b"-bitcode_verify" => {
                st.obsolete.push(format!("{name} is no longer supported and will be ignored"))
            }
            b"-ld_classic" => {
                st.warnings.warn("-ld_classic is no longer supported and will be ignored")
            }
            b"-ld_prime" => st.obsolete.push("-ld_prime is deprecated, use -ld_new instead".into()),
            b"-ld_new" => {}
            // ld64 could still link the fragile (version 1) Objective-C
            // ABI of 32-bit macOS; ld-prime knows the modern one alone.
            b"-objc_abi_version" => {
                let version = cur.next_arg(name).as_bytes();
                if version != b"2" {
                    fatal!("-objc_abi_version '{}' not supported (expected 2)", display(version));
                }
            }

            // Reserve enough header padding that install_name_tool can
            // grow install names in place.
            b"-headerpad_max_install_names" => args.headerpad_max_install_names = true,

            b"-deduplicate" => args.deduplicate = true,
            b"-text_exec" => args.text_exec = true,
            b"-kexts_use_stubs" => args.kexts_use_stubs = true,
            b"-no_branch_islands" => args.no_branch_islands = true,
            b"-no_deduplicate" => args.deduplicate = false,
            b"-verbose_deduplicate" => args.verbose_deduplicate = true,
            // ld-prime folds identical functions in passes, each folding
            // the callers of those the one before folded, up to this
            // many (none limits it); mold folds them in one go.
            b"-max_code_deduplicate_passes" => {
                if decimal_number(cur.next_text(name)).is_none() {
                    fatal!("invalid argument for -max_code_deduplicate_passes");
                }
            }
            b"-function_starts" => st.function_starts = Some(true),
            b"-add_source_version" => st.source_version = Some(true),
            b"-no_source_version" => st.source_version = Some(false),
            b"-source_version" => {
                let arg = cur.next_text(name);
                st.source_version_number = Some(parse_source_version(arg).unwrap_or_else(|| {
                    fatal!("-source_version: malformed 64-bit a.b.c.d.e version number: {arg}")
                }));
                st.source_version = Some(true);
            }
            // ld64 kept the FDEs of functions with compact unwind
            // records for a target before macOS 10.9 (iOS 7), or as
            // these said. ld-prime goes by the target alone.
            b"-keep_dwarf_unwind" | b"-no_keep_dwarf_unwind" => {
                st.obsolete.push(format!("{name} is obsolete"))
            }
            b"-init_offsets" => args.init_offsets = true,
            b"-init" => args.init = Some(cur.next_text(name).to_string()),
            b"-data_const" => st.data_const = Some(true),
            b"-no_data_const" => st.data_const = Some(false),
            b"-no_implicit_dylibs" => args.no_implicit_dylibs = true,
            b"-objc_relative_method_lists" => st.objc_relative_method_lists = Some(true),
            b"-no_objc_relative_method_lists" => st.objc_relative_method_lists = Some(false),
            b"-no_objc_category_merging" => args.objc_category_merging = false,
            b"-no_function_starts" => st.function_starts = Some(false),
            b"-data_in_code_info" => st.data_in_code_info = Some(true),
            b"-add_split_seg_info" => args.add_split_seg_info = true,
            b"-no_data_in_code_info" => st.data_in_code_info = Some(false),

            // The last of the two counts.
            b"-no_uuid" => args.uuid = false,
            b"-random_uuid" => {
                args.uuid = true;
                args.random_uuid = true;
            }
            b"-no_dynamic_access" => args.no_dynamic_access = true,
            b"-no_shared_cache_eligible" => {
                args.not_for_dyld_shared_cache = true;
                args.shared_cache_marker = true;
            }
            b"-warn_weak_exports" => args.warn_weak_exports = true,
            b"-no_weak_exports" => args.no_weak_exports = true,
            b"-no_weak_imports" => args.no_weak_imports = true,
            b"-weak_reference_mismatches" => {
                args.weak_reference_mismatches = match cur.next_arg(name).as_bytes() {
                    b"non-weak" => WeakRefMismatches::NonWeak,
                    b"weak" => WeakRefMismatches::Weak,
                    b"error" => WeakRefMismatches::Error,
                    _ => fatal!(
                        "invalid option to -weak_reference_mismatches [ error | weak | non-weak ]"
                    ),
                }
            }
            // ld-prime's usage leaves use_dylibs out, and takes a
            // missing treatment for an invalid one.
            b"-commons" => {
                args.commons = match cur.advance().map(|arg| arg.as_bytes()) {
                    Some(b"ignore_dylibs") => CommonsMode::IgnoreDylibs,
                    Some(b"use_dylibs") => CommonsMode::UseDylibs,
                    Some(b"error") => CommonsMode::Error,
                    _ => fatal!("invalid option to -commons [ ignore_dylibs | error ]"),
                }
            }
            b"-warn_commons" => args.warn_commons = true,
            b"-max_default_common_align" => {
                let align = parse_common_align(cur.next_text(name), &mut st.warnings);
                st.max_default_common_align = Some(align);
            }
            b"-force_symbols_weak_list" | b"-force_symbols_not_weak_list" => {
                let names = cur.next_symbol_list(name);
                let glob = match name {
                    "-force_symbols_weak_list" => &mut st.lists.force_weak,
                    _ => &mut st.lists.force_not_weak,
                };
                add_patterns(glob, &names, 0);
                st.force_weakness_listed = true;
            }
            b"-keep_duplicate" => {
                add_patterns(&mut st.lists.keep_duplicates, [cur.next_text(name)], 0);
            }
            b"-keep_duplicates_list" => {
                add_patterns(&mut st.lists.keep_duplicates, cur.next_symbol_list(name), 0);
            }
            b"-allow_dead_duplicates" => args.allow_dead_duplicates = true,
            b"-deployment_target_mismatches" => {
                args.deployment_target_mismatches = parse_treatment(name, cur.next_arg(name), true);
            }
            b"-sub_library" => args.sub_libraries.push(cur.next_bytes(name)),
            b"-sub_umbrella" => args.sub_umbrellas.push(cur.next_bytes(name)),
            b"-image_suffix" => args.image_suffixes.push(cur.next_arg(name).to_owned()),
            b"-encryptable" => args.encryptable = true,
            b"-no_encryption" => args.encryptable = false,
            b"-interposable" => st.lists.interposable_all = true,
            b"-interposable_list" => {
                let names = cur.next_symbol_list(name);
                add_patterns(st.lists.interposable_list.get_or_insert_default(), &names, 0);
            }
            b"-unaligned_pointers" => {
                st.unaligned_pointers = Some(parse_treatment(name, cur.next_arg(name), true));
            }
            // Whether objects may disagree on signing class_ro_t
            // pointers, which only arm64e signs: nothing to check here.
            b"-objc_class_ro_signing_mismatch" => {
                parse_treatment(name, cur.next_arg(name), false);
            }
            b"-poison_symbol" => {
                st.lists.poisoned.add(cur.next_arg(name).as_bytes(), 0);
            }
            b"-poison_symbols_list" => {
                for pat in cur.next_symbol_list(name) {
                    st.lists.poisoned.add(pat.as_bytes(), 0);
                }
            }
            // For duplicate symbols ld-prime would only warn of, which
            // it has no more: it takes the treatment and does nothing.
            b"-duplicate_symbols" => {
                parse_treatment(name, cur.next_arg(name), false);
            }

            b"-dyld_env" => {
                let arg = cur.next_arg(name).as_bytes();
                if !arg.starts_with(b"DYLD_") || !arg.contains(&b'=') {
                    fatal!(
                        "malformed '-dyld_env {}', arg should be of form 'DYLD_xxx=something'",
                        display(arg)
                    );
                }
                args.dyld_envs.push(arg.to_vec());
            }

            b"-macos_version_min" | b"-macosx_version_min" => {
                read_macos_version_min(&mut cur, &mut args, &mut st, name)
            }
            b"-ios_version_min"
            | b"-iphoneos_version_min"
            | b"-maccatalyst_version_min"
            | b"-iosmac_version_min"
            | b"-uikitformac_version_min" => read_other_version_min(&mut cur, &mut st, name),

            // This linker's output is always deterministic, but ld-prime
            // writes no modification times in the stabs then either.
            b"-reproducible" => args.zero_ar_date = true,

            b"-lto_library" => st.lto_libraries.push(cur.next_path(name)),
            b"-mcpu" => args.lto_cpu = Some(cur.next_text(name).to_string()),
            b"-mllvm" => args.mllvm.push(cur.next_bytes(name)),
            b"-save-temps" => args.save_temps = true,
            b"-flto-codegen-only" => args.lto_codegen_only = true,
            // The ThinLTO cache. ld-prime reads the numbers as strtoul
            // does and hands libLTO their low 32 bits, as an int or an
            // unsigned (so -1 never prunes), checking the percentage
            // only then.
            b"-cache_path_lto" => args.lto_cache_dir = Some(cur.next_path(name)),
            b"-prune_interval_lto" => {
                args.lto_cache_prune_interval = Some(lto_cache_number(name, cur.next_arg(name)));
            }
            b"-prune_after_lto" => {
                args.lto_cache_expiration = lto_cache_number(name, cur.next_arg(name)) as u32;
            }
            b"-max_relative_cache_size_lto" => {
                let value = lto_cache_number(name, cur.next_arg(name)) as u32;
                if value > 100 {
                    fatal!("Expect a value between 0 and 100 for -max_relative_cache_size_lto");
                }
                args.lto_cache_max_size = value;
            }
            // The variant architectures' reuse of one another's LTO
            // results, which have no cache here.
            b"-cache_dir" => {
                cur.next_arg(name);
            }
            b"-arch_variant_lto_cache_mismatch" => {
                let treatment = cur.next_arg(name);
                if !matches!(treatment.as_bytes(), b"warning" | b"error" | b"suppress") {
                    fatal!(
                        "-arch_variant_lto_cache_mismatch invalid option (warning | error | suppress)"
                    );
                }
            }
            b"-use_lto_filenames_in_order_file_matching" => {
                args.lto_filenames_in_order_file = true;
            }
            b"-no_use_lto_filenames_in_order_file_matching" => {
                args.lto_filenames_in_order_file = false;
            }
            b"-lto_softload_runtime_symbols" => st.lto_softload = Some(true),
            b"-no_lto_softload_runtime_symbols" => st.lto_softload = Some(false),

            b"-dependency_info" => args.dependency_info = Some(cur.next_path(name)),

            b"-object_path_lto" => args.object_path_lto = Some(cur.next_path(name)),

            b"-objc_stubs_fast" => st.objc_stubs_small = Some(false),
            b"-objc_stubs_small" => st.objc_stubs_small = Some(true),
            b"-const_selrefs" => st.const_selrefs = Some(true),
            b"-no_const_selrefs" => st.const_selrefs = Some(false),
            // ld64's switches for passes ld-prime doesn't run: the
            // labels a -r output gave the FDEs in __eh_frame, the
            // ordering of initializer functions within __text, and
            // x86-64's pass for zero-fill sections out of reach of
            // 32-bit displacements. ld-prime takes them silently.
            b"-no_eh_labels" | b"-no_order_inits" | b"-no_huge" => {}
            b"-no_dwarf_unwind" => args.no_dwarf_unwind = true,
            b"-merge_zero_fill_sections" => args.merge_zero_fill_sections = true,
            b"-remove_swift_reflection_metadata_sections" => {
                args.remove_swift_reflection_metadata_sections = true
            }
            // ld64's order file for one section, -sectorder <segment>
            // <section> <path>, ld-prime takes for an -order_file
            // whatever the section names, empty ones too.
            b"-sectorder" => {
                let file = match (cur.advance(), cur.advance(), cur.advance()) {
                    (Some(_), Some(_), Some(file)) if !file.is_empty() => file,
                    _ => fatal!("-sectorder missing <segment> <section> <file-path>"),
                };
                args.order_files.push(PathBuf::from(file));
            }
            b"-ignore_auto_link" => args.ignore_auto_link = true,
            b"-force_load_swift_libs" => args.force_load_swift_libs = true,
            b"-add_linker_option" => {
                let opt = cur.next_arg(name).as_bytes();
                add_linker_option(&mut args.linker_options, opt, &mut st.warnings);
            }
            // ld64 took the D script of the image's probes from this;
            // ld-prime neither opens the file nor needs one, in a -r
            // link either.
            b"-dtrace" => {
                cur.next_arg(name);
            }
            // The DOF that describes the image's USDT probe sites
            // (__TEXT,__dof_<provider>), which ld-prime makes unless
            // told not to - and then fails to link the sites, branches
            // to address 0.
            b"-no_dtrace_dof" => args.dtrace_dof = false,
            // ld-prime skips an empty argument, which names no file: a
            // build system's empty variable, or '' in a response file.
            b"" => {}

            raw if raw.starts_with(b"-") => {
                read_joined_option(&mut cur, &mut args, &mut st, raw, name)
            }
            _ => args.inputs.push(input_file(opt)),
        }
    }

    finish_options(&mut args, &mut st);
    set_output_kind(&mut args, st.kind);
    if let Some(triple) = st.target_triple {
        apply_target_triple(&mut args, triple);
    }
    if args.inputs.is_empty() {
        exit_without_inputs(&args);
    }
    // A parse for another target than this one is redone by the driver,
    // so what depends on the target is left to that parse.
    if !resolve_target(target, &mut args) {
        crate::error::drop_held();
        return args;
    }

    check_segment_order(&args);
    resolve_defaults(target, &mut args, &st);
    std::mem::take(&mut st.lists).build(&mut args);
    args.merged_files = notes_merged_files(&args);

    // -fatal_warnings applies to every warning, wherever it appears on
    // the command line. So does -w to those from the option checks
    // below, but not to those given as options were read.
    crate::error::set_fatal_warnings(args.fatal_warnings);
    st.warnings.print();
    crate::error::set_suppress_warnings(args.suppress_warnings);
    check_arch_options(target, &mut args, &st);
    resolve_env_source_version(&mut args, &st);
    if let Some((old, new)) = st.incompatible_platforms {
        fatal!("incompatible platforms: {} - {}", platform_name(old), platform_name(new));
    }
    check_options(target, &mut args, &mut st);
    check_last(target, &mut args, &st);
    args
}

/// What ld-prime does once it has read the last option, before it looks
/// at the inputs: it vets -lto_library, reports the options it doesn't
/// know, and reads the environment variables that stand in for options.
fn finish_options(args: &mut Args, st: &mut ParseState) {
    args.lto_library = resolve_lto_library(std::mem::take(&mut st.lto_libraries));
    // ld-prime reports the options it doesn't know together, once it
    // has read the others (and given their warnings).
    if !st.unknown.is_empty() {
        fatal!("unknown options: {}", st.unknown);
    }
    // Then it reads -objc_class_ro_signing_mismatch's environment
    // variable, as it would the option.
    let env = "LD_OBJC_CLASS_RO_SIGNING_MISMATCH";
    if let Some(val) = std::env::var_os(env) {
        if val.is_empty() {
            fatal!("{env} missing <option>");
        }
        parse_treatment(env, &val, false);
    }
    trace_env(args);
}

/// Sets the Mach-O file type of the output and the kind of image it is.
fn set_output_kind(args: &mut Args, kind: OutputKind) {
    args.output_type = match kind {
        OutputKind::Dylib => MH_DYLIB,
        OutputKind::Bundle => MH_BUNDLE,
        OutputKind::Kext => MH_KEXT_BUNDLE,
        OutputKind::Dylinker => MH_DYLINKER,
        _ => MH_EXECUTE,
    };
    args.relocatable = kind == OutputKind::Object;
    args.static_link = matches!(kind, OutputKind::StaticExecutable | OutputKind::Preload);
    args.preload = kind == OutputKind::Preload;
}

/// `ld -v` with nothing to link just reports the version; build
/// systems and configure scripts probe the linker that way. mold
/// does the same for -v/--version with no inputs. So does
/// -version_details, unless -v comes with it.
fn exit_without_inputs(args: &Args) -> ! {
    if args.verbose {
        print_version();
        std::process::exit(0);
    }
    if args.version_details {
        print_version_details();
        std::process::exit(0);
    }
    fatal!("no object files specified");
}

/// Settles what the image is linked for, and returns whether that is
/// `target`. Without -arch, the first object file names the
/// architecture (see detect_target); without -platform_version (or the
/// like), the parse for the target looks for the platform in the
/// objects too (see infer_platform).
fn resolve_target(target: &TargetTraits, args: &mut Args) -> bool {
    if args.arch.is_none() {
        args.arch = Some(detect_target(args));
    }
    if args.arch != Some(target.name) {
        return false;
    }
    if args.platform == 0 {
        infer_platform(args);
    }
    true
}

/// Resolves the options whose defaults depend on the target and the
/// kind of output, as ld-prime does once it knows them: the code
/// tables, the source version, the signature, the Objective-C
/// optimizations, the alignment of common symbols, the header padding,
/// and how the image starts.
fn resolve_defaults(target: &TargetTraits, args: &mut Args, st: &ParseState) {
    // A -static image (a kernel) carries the code tables only when
    // asked to, as ld-prime writes it.
    args.function_starts = st.function_starts.unwrap_or(!args.without_dyld());
    args.data_in_code_info = st.data_in_code_info.unwrap_or(!args.without_dyld());
    // LC_SOURCE_VERSION came with macOS 10.8; ld-prime gives an image
    // for an older one none.
    args.source_version = st
        .source_version
        .unwrap_or(
            args.platform != PLATFORM_MACOS || args.platform_minos >= encode_version(10, 8, 0),
        )
        .then_some(st.source_version_number.unwrap_or(0));

    // ld-prime signs arm64 macOS images by default and leaves x86_64
    // ones unsigned (Intel Macs and Rosetta run unsigned code), and a
    // -static image or a kext (signed if at all by whoever packages it)
    // and firmware unsigned too.
    args.adhoc_codesign = st.adhoc_codesign.unwrap_or(
        target.name == "arm64" && !args.without_dyld() && args.platform == PLATFORM_MACOS,
    );

    // ld-prime converts Objective-C method lists from macOS 11 on, in
    // every arm64 image, and on x86-64 in dylibs and bundles only: an
    // x86-64 executable keeps the compiler's absolute lists at any
    // deployment target. It optimizes the Objective-C of no image dyld
    // doesn't load (a -static or -preload one, a kext): it converts no
    // method list there, whatever the option says, and merges no
    // category (nor folds a class reference, see fold_objc_classrefs).
    args.objc_relative_method_lists = !args.without_dyld()
        && st.objc_relative_method_lists.unwrap_or(
            (target.name == "arm64" || args.output_type != MH_EXECUTE)
                && args.platform == PLATFORM_MACOS
                && args.platform_minos >= encode_version(11, 0, 0),
        );
    args.objc_category_merging &= !args.without_dyld();

    // A -preload image has no __LINKEDIT segment: ld-prime keeps nothing
    // outside its segments but the symbol table (and the local
    // relocations of a -pie one). The options asking for the code
    // tables, a build or source version or a signature go unheeded, as
    // does -rpath, which only dyld would read.
    // A -preload image aligns its common symbols to 256 bytes at most.
    args.max_default_common_align =
        st.max_default_common_align.unwrap_or(if args.preload { 8 } else { 15 });
    if args.preload {
        args.function_starts = false;
        args.data_in_code_info = false;
        args.version_load_command = false;
        args.source_version = None;
        args.adhoc_codesign = false;
        args.rpaths.clear();
    }

    // ld-prime leaves 32 bytes free after the load commands unless
    // -headerpad says otherwise, but 128 in firmware that dyld loads.
    let dyld_loaded_firmware =
        args.platform == PLATFORM_FIRMWARE && !args.without_dyld() && !args.relocatable;
    args.headerpad = st.headerpad.unwrap_or(if dyld_loaded_firmware { 128 } else { 32 });

    // An image no dyld loads, and dyld, which the kernel loads, start
    // from LC_UNIXTHREAD at "start", crt1.o's entry point, as every
    // executable did before LC_MAIN had dyld call _main (from macOS
    // 10.8 on): ld-prime starts an x86-64 one for an older macOS so.
    let old_x86_64_executable = target.name == "x86_64"
        && args.output_type == MH_EXECUTE
        && !args.relocatable
        && args.platform == PLATFORM_MACOS
        && args.platform_minos < encode_version(10, 8, 0);
    args.unixthread = args.static_link || args.is_dylinker() || old_x86_64_executable;
    if args.unixthread && !st.explicit_entry {
        args.entry = "start".to_string();
    }
}

impl SymbolLists {
    /// Compiles the lists into Args's matchers.
    fn build(mut self, args: &mut Args) {
        args.exported_symbols = self.exported_symbols.map(GlobBuilder::build);
        args.unexported_symbols = self.unexported_symbols.build();
        args.reexported_symbols = self.reexported_symbols.build();
        args.why_live = self.why_live.build();
        args.local_strip_list = self.local_strip_list.build();
        args.local_keep_list = self.local_keep_list.map(GlobBuilder::build);
        args.force_weak = self.force_weak.build();
        args.force_not_weak = self.force_not_weak.build();
        args.keep_duplicates = self.keep_duplicates.build();
        args.poisoned = self.poisoned.build();
        if self.interposable_all && self.interposable_list.is_none() {
            self.interposable_list.get_or_insert_default().add(b"*", 0);
        }
        args.interposable = self.interposable_list.map(GlobBuilder::build);
    }
}

/// Whether the link notes the files of the private libraries a dylib
/// re-exports and merges: see Args::merged_files.
fn notes_merged_files(args: &Args) -> bool {
    let lists_reexports = args.exported_symbols.is_some() || !args.reexported_symbols.is_empty();
    let reexports_library = !args.sub_libraries.is_empty()
        || !args.sub_umbrellas.is_empty()
        || args.inputs.iter().any(|input| {
            matches!(input, InputArg::Library(LibraryKind::Reexport | LibraryKind::NoMerge, _))
        });
    args.map.is_some()
        || !args.why_live.is_empty()
        || args.warn_commons
        || args.commons == CommonsMode::Error
        || (lists_reexports && reexports_library)
}

/// The architecture options ld-prime checks against the target, and the
/// environment variables it reads for them.
fn check_arch_options(target: &TargetTraits, args: &mut Args, st: &ParseState) {
    if st.arch_variant {
        fatal!("-arch_variant is not supported with -arch {}", target.name);
    }
    let env_subtypes = std::env::var_os("LD_DYLIB_CPU_SUBTYPES_MUST_MATCH");
    if let Some(list) = st.dylib_subtype_list.or(env_subtypes.as_deref().map(OsStrExt::as_bytes)) {
        args.dylib_subtypes_must_match = names_cpu_family(list, target.name);
    }
    args.dylib_arch_fallback = dylib_arch_fallback(target.name);
}

/// The build system's source version stands in for -source_version
/// unless -no_source_version says there is none (ld-prime reads it
/// even where there is none anyway).
fn resolve_env_source_version(args: &mut Args, st: &ParseState) {
    if st.source_version != Some(false) && st.source_version_number.is_none() {
        let version = env_source_version();
        if let Some(v) = &mut args.source_version {
            *v = version;
        }
    }
}

/// ld-prime checks the options it has read in this order, each
/// diagnostic in its place: a fatal error stops the checks after it.
/// It resolves the defaults of some options on the way, from what the
/// checks before them settled.
fn check_options(target: &TargetTraits, args: &mut Args, st: &mut ParseState) {
    args.segaddrs = resolve_segaddrs(std::mem::take(&mut args.segaddrs));
    if args.kernel && st.kind != OutputKind::StaticExecutable {
        fatal!("-kernel must be used with -static");
    }
    // Only a bundle has a loader.
    if st.kind != OutputKind::Bundle
        && args.inputs.iter().any(|arg| matches!(arg, InputArg::BundleLoader(_)))
    {
        fatal!("-bundle_loader can only be used with -bundle");
    }
    resolve_lazy_load(args);
    resolve_delay_init(args);
    // ld64 chained a static arm64e image's rebases through its pointers
    // from a __TEXT,__thread_starts list; ld-prime has chained fixups.
    if st.threaded_starts {
        if st.fixup_chains == Some(true) {
            fatal!(
                "-fixup_chains*, -rebase_section and -threaded_starts_section can't be used together"
            );
        }
        fatal!("-threaded_starts_section is no longer supported");
    }
    check_output_kind(args, st.pie);
    resolve_fixups(target, args, st);
    // Only a dylib is mergeable: ld-prime checks so here, and that only
    // a dylib gets the debug hook right after the next check.
    if args.make_mergeable && args.output_type != MH_DYLIB {
        fatal!("-make_mergeable can only be used when creating a dynamic library");
    }
    // A mergeable dylib's code is linked again where it is merged, by
    // the fixups of the instructions it has, which an applied hint may
    // have rewritten (ld-prime applies none anywhere).
    if args.make_mergeable {
        args.ignore_optimization_hints = true;
    }
    // What is dead is known only once the final link sees every
    // reference.
    if args.relocatable && args.dead_strip {
        fatal!("-r and -dead_strip cannot be used together");
    }
    if args.add_mergeable_debug_hook && args.output_type != MH_DYLIB {
        fatal!("-add_mergeable_debug_hook can only be used with -dylib");
    }
    if st.x86_64_layout_emulation && target.name != "arm64" {
        crate::warn!(
            "ignoring -x86_64_layout_emulation option, it can only be used with -arch arm64"
        );
    }
    check_dylib_use(target, args);
    args.objc_stubs_small = st.objc_stubs_small == Some(true);
    resolve_shared_region(target, args);
    resolve_dirty_data(args);
    resolve_sdk_order_file(args);

    args.segment_align = resolve_segment_align(target, args, st.segalign);
    resolve_encryptable(args);
    // An encryptable image's __oslogstring, which goes unencrypted,
    // starts a page of its own unless -sectalign says otherwise.
    let oslog =
        |(seg, sect, _): &(Vec<u8>, Vec<u8>, u8)| seg == b"__TEXT" && sect == b"__oslogstring";
    if args.encryptable && !args.sectalign.iter().any(oslog) {
        let p2align = args.segment_align.max(1).ilog2() as u8;
        args.sectalign.push((b"__TEXT".to_vec(), b"__oslogstring".to_vec(), p2align));
    }
    args.segprots = resolve_segprots(target, std::mem::take(&mut st.segprots));
    args.seg_page_sizes = resolve_seg_page_sizes(args, std::mem::take(&mut st.seg_page_sizes));
    resolve_pagezero_size(args);
    resolve_stack(target, args, st.stack_size, st.stack_addr);
    check_relocatable(args, st.data_const);
    args.const_selrefs = st.const_selrefs.unwrap_or(args.shared_region);
    args.lto_softload = st.lto_softload.unwrap_or(args.static_link || args.preload);
    args.warn_unused_dylibs =
        st.warn_unused_dylibs.unwrap_or(args.shared_region && args.output_type == MH_DYLIB);
    args.data_const = st.data_const.unwrap_or_else(|| default_data_const(args, st.pie));
    resolve_kext(target, args);
    check_segaddrs(args);
    complete_segment_order(args);
    check_section_order(args);
    resolve_image_base(args);
    args.unaligned_pointers = resolve_unaligned_pointers(target, args, st.unaligned_pointers);
    args.objc_stubs_small &= target.name == "arm64";
}

/// Resolves how the image's pointers are fixed up as it loads: by
/// chained fixups, dyld's opcodes or the legacy LINKEDIT's relocations
/// (or not at all, in an image no dyld loads), bound lazily or not; and
/// whether it is position independent, emits its initializers as
/// offsets, or may fix up read-only segments.
fn resolve_fixups(target: &TargetTraits, args: &mut Args, st: &mut ParseState) {
    // kmutil links a kext by its relocations and slides a -kernel
    // image by its local ones: ld-prime takes neither -fixup_chains nor
    // -no_fixup_chains for them.
    if args.is_kext() || args.kernel {
        st.fixup_chains = None;
    }
    args.pie = resolve_pie(target, args, st.pie, st.fixup_chains);
    args.fixup_chains = resolve_fixup_chains(target, args, st.fixup_chains);
    args.no_fixup_chains = st.fixup_chains == Some(false);
    args.fixup_chains_section = st.chain_starts.is_some() && args.static_link && args.fixup_chains;
    args.chain_starts_kind = st.chain_starts.unwrap_or(0);
    // ld64 binds lazily below the chained-fixups deployment targets
    // unless -bind_at_load.
    args.lazy_binding =
        !args.relocatable && !args.without_dyld() && !args.fixup_chains && !args.bind_at_load;
    args.legacy_linkedit = resolve_legacy_linkedit(target, args);
    // ld-prime emits initializers as offsets implicitly with chained
    // fixups: the point of chains is a fixup-free __DATA_CONST, and
    // absolute initializer pointers would drag rebases back in. It
    // follows -fixup_chains or the deployment target even when
    // -undefined dynamic_lookup sends the fixups themselves back to
    // classic dyld info; only -no_fixup_chains keeps __mod_init_func.
    // Not so for an image whose initializers dyld never runs, a
    // -static one or a kext (XNU runs the kernel's __mod_init_func
    // itself, and a kext's): it converts only with -init_offsets.
    args.init_offsets |= !args.without_dyld()
        && st.fixup_chains.unwrap_or_else(|| chained_fixups_by_default(target, args));
    args.text_relocs = resolve_text_relocs(target, args, st.read_only_relocs);
    check_fixup_sections(args, st.fixup_chains, st.chain_starts.is_some(), st.rebase_section);
}

/// The checks and warnings ld-prime gives last, once it has resolved
/// the image's layout.
fn check_last(target: &TargetTraits, args: &mut Args, st: &ParseState) {
    // An image dyld loads keeps 32 bytes for the command of a code
    // signature added later (see chunks::header_pad).
    if let Some(size) = st.headerpad
        && size < 32
        && !args.without_dyld()
        && !args.relocatable
    {
        crate::warn!(
            "-headerpad {size:#x} is too small, at least 32 bytes are required to reserve space for code signature"
        );
    }
    warn_platform_options(target, args, st.read_only_relocs.is_some());
    check_dynamic_lookup(args);
    // So is the -init function an initial undefine, as -u would make
    // it (a -r output keeps it undefined).
    args.forced_undefined.extend(args.init.clone());
    // Only a dylib has exports of others' symbols to publish.
    if st.reexports_listed && args.output_type != MH_DYLIB {
        fatal!("-reexported_symbols_list can only used used when created dynamic libraries");
    }
    // ld-prime deprecates the lists but for the libraries of /usr/lib,
    // libSystem's among them, which still use them.
    let usr_lib =
        args.output_type == MH_DYLIB && args.output_install_name().starts_with(b"/usr/lib/");
    if st.force_weakness_listed && !usr_lib {
        crate::warn!("-force_symbols_[not_]weak_list is deprecated");
    }
    for msg in &st.obsolete {
        crate::warn!("{msg}");
    }
    // ld-prime leaves this one out under -w, -fatal_warnings or not.
    if !args.has_entry_point() && st.explicit_entry && !args.suppress_warnings {
        crate::warn!("ignoring -e, not used for output type");
    }
}

/// Whether an install name lies where the dyld shared cache takes
/// libraries from: /usr/lib, /System/Library or their counterparts
/// under /Library/Apple.
pub fn in_shared_cache_path(install_name: &[u8]) -> bool {
    [
        &b"/usr/lib/"[..],
        b"/System/Library/",
        b"/Library/Apple/usr/lib/",
        b"/Library/Apple/System/Library/",
    ]
    .iter()
    .any(|dir| install_name.starts_with(dir))
}

/// Decides whether the image is bound for the dyld shared cache or a
/// kernel collection (ld64's fSharedRegionEligible): with
/// -add_split_seg_info or -kernel, an arm64 kext, dyld, or a dylib
/// installed where the cache takes libraries from, unless
/// -not_for_dyld_shared_cache, or -debug_variant for a dylib. Such an
/// image records its references between sections
/// (LC_SEGMENT_SPLIT_INFO), so ld64 leaves its code as compiled (no
/// optimization hints); it may not look symbols up dynamically, since
/// the cache builder binds every one to the dylib that exports it (as
/// ld-prime checks later: check_dynamic_lookup); nor may it have small
/// objc stubs, on either architecture; and ld-prime warns about run
/// paths, which an OS library must not need. (A flat namespace it
/// refuses earlier: check_dylib_use.)
fn resolve_shared_region(target: &TargetTraits, args: &mut Args) {
    args.shared_region = shared_region_eligible(target, args);
    if !args.shared_region {
        return;
    }
    args.ignore_optimization_hints = true;
    if !args.rpaths.is_empty() {
        crate::warn!(
            "OS dylibs should not add rpaths (linker option: -rpath) (Xcode build setting: \
             LD_RUNPATH_SEARCH_PATHS)"
        );
    }
    // Nor be found by run path, as a dylib -add_split_seg_info makes
    // eligible may be.
    let install_name = args.install_name.as_deref().unwrap_or_default();
    if args.output_type == MH_DYLIB && install_name.starts_with(b"@rpath") {
        crate::warn!(
            "OS dylibs should not use @rpath for -install_name. Use absolute path instead"
        );
    }
    if args.objc_stubs_small {
        fatal!("Shared cache eligible dylibs cannot use '-objc_stubs_small'");
    }
}

/// The data an OS dylib bound for the shared cache dirties, which
/// ld-prime moves to __DATA_DIRTY (see symbol_moves) when no
/// -dirty_data_list gives any: an Apple-internal SDK's list for the
/// dylib, AppleInternal/DirtyDataFiles/<the install name's leaf>.dirty
/// under the first -syslibroot, if there is one. Its lines name symbols
/// alone, a pattern's characters as any others.
fn resolve_dirty_data(args: &mut Args) {
    let install_name = args.output_install_name();
    if !args.dirty_data.is_empty()
        || !args.shared_region
        || args.output_type != MH_DYLIB
        || !in_shared_cache_path(install_name)
    {
        return;
    }
    let Some(root) = args.syslibroot.first() else {
        return;
    };
    let leaf = install_name.rsplit(|&c| c == b'/').next().unwrap_or_default();
    let file = [leaf, b".dirty"].concat();
    let path = root.join("AppleInternal/DirtyDataFiles").join(std::ffi::OsStr::from_bytes(&file));
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let mut symbols = GlobBuilder::default();
    for sym in symbol_list(&text) {
        symbols.add_literal(sym.as_bytes(), 1);
    }
    let segment = b"__DATA_DIRTY".to_vec();
    args.dirty_data.push(SymbolMove { segment, symbols: symbols.build() });
}

/// The order of an image no -order_file orders, as ld-prime finds it:
/// an Apple-internal SDK's file for the -final_output name,
/// AppleInternal/OrderFiles/<that name>.order under the first
/// -syslibroot, if there is one (the name as given: a path finds none).
fn resolve_sdk_order_file(args: &mut Args) {
    if !args.order_files.is_empty() {
        return;
    }
    let (Some(name), Some(root)) = (&args.final_output, args.syslibroot.first()) else {
        return;
    };
    let path = [root.as_os_str().as_bytes(), b"/AppleInternal/OrderFiles/", name, b".order"];
    let path = PathBuf::from(std::ffi::OsStr::from_bytes(&path.concat()));
    if path.is_file() {
        args.order_files.push(path);
    }
}

/// ld-prime's checks of -U and -undefined dynamic_lookup, among the last
/// of the options: -U is redundant with dynamic_lookup, and with it
/// ignored, for the entry point too, which must otherwise resolve in
/// the link, as an initial undefine; and an image bound for the shared
/// region may use neither (see resolve_shared_region), but a kext,
/// which looks up every import.
fn check_dynamic_lookup(args: &Args) {
    let dynamic_lookup = args.undefined_dynamic_lookup;
    if dynamic_lookup && !args.allowed_undefined.is_empty() {
        crate::warn!("-U option is redundant when using -undefined dynamic_lookup");
    } else if args.has_entry_point() && args.allowed_undefined.contains(&args.entry) {
        fatal!("{} is an entry point and can't be used with -U for dynamic lookup", args.entry);
    }
    if args.shared_region
        && (dynamic_lookup || !args.allowed_undefined.is_empty())
        && !args.is_kext()
    {
        fatal!(
            "Shared cache eligible dylibs cannot use '-undefined dynamic_lookup' or '-U' to find \
             symbols. Remove these options or opt out of the shared cache using the build \
             setting 'LD_SHARED_CACHE_ELIGIBLE=NO' (or linker flag '-not_for_dyld_shared_cache')"
        );
    }
}

/// Whether the image is bound for the shared region: see
/// resolve_shared_region.
fn shared_region_eligible(target: &TargetTraits, args: &Args) -> bool {
    let is_dylib = args.output_type == MH_DYLIB;
    !args.not_for_dyld_shared_cache
        && !(is_dylib && args.debug_variant)
        && (args.add_split_seg_info
            || args.kernel
            || (args.is_kext() && target.name == "arm64")
            || args.is_dylinker()
            || (is_dylib && in_shared_cache_path(args.output_install_name())))
}

/// What ld-prime makes of a pointer dyld fixes up that is not 8-aligned
/// (see chunks::chained_fixups::check_pointer_alignment), in an image
/// dyld loads: what -unaligned_pointers says, else a warning where the
/// image has chained fixups or its deployment target would, else
/// nothing. An arm64 image with chained fixups, or bound for the shared
/// region, fails on one whatever the option says, with a warning if it
/// says warning. (An x86-64 one gives chained fixups up instead.)
fn resolve_unaligned_pointers(
    target: &TargetTraits,
    args: &Args,
    treatment: Option<Treatment>,
) -> Treatment {
    if args.relocatable || args.without_dyld() {
        return Treatment::Suppress;
    }
    if target.name == "arm64" && (args.fixup_chains || args.shared_region) {
        if treatment == Some(Treatment::Warning) {
            match args.fixup_chains {
                true => {
                    crate::warn!("unaligned pointer errors are fatal when using chained fixups")
                }
                false => crate::warn!("unaligned pointer errors are fatal in OS binaries"),
            }
        }
        return Treatment::Error;
    }
    let new_os = is_new_os(target.name, args.output_type, args.platform, args.platform_minos);
    treatment.unwrap_or(match args.fixup_chains || new_os {
        true => Treatment::Warning,
        false => Treatment::Suppress,
    })
}

/// A kext (ld64's kKextBundle, MH_KEXT_BUNDLE) is linked into the
/// kernel by kmutil, which resolves its undefined symbols against the
/// kernel's and other kexts' exports: ld64 treats them as dynamically
/// looked up. On arm64 its code gets a __TEXT_EXEC segment of its own
/// (-text_exec) and, as the kext is bound for the shared region,
/// __DATA_CONST (-data_const).
fn resolve_kext(target: &TargetTraits, args: &mut Args) {
    if !args.is_kext() {
        return;
    }
    args.undefined_dynamic_lookup = true;
    args.text_exec |= target.name == "arm64";
}

impl Args {
    pub fn is_kext(&self) -> bool {
        self.output_type == MH_KEXT_BUNDLE
    }

    /// Whether the image is dyld itself (ld64's kDyld, MH_DYLINKER),
    /// which the kernel maps next to a main executable and starts: it
    /// loads no dylib, and slides itself by its fixups before it loads
    /// anything else.
    pub fn is_dylinker(&self) -> bool {
        self.output_type == MH_DYLINKER
    }

    /// Whether the image starts at an entry point (-e): a main
    /// executable, or dyld.
    pub fn has_entry_point(&self) -> bool {
        (self.output_type == MH_EXECUTE && !self.relocatable) || self.is_dylinker()
    }

    /// Whether the image may link dylibs: not an image no dyld loads,
    /// with nothing to load them, nor dyld, which is what loads them.
    /// ld-prime searches -l for archives alone in either, and ignores a
    /// dylib named outright.
    pub fn links_dylibs(&self) -> bool {
        !self.without_dyld() && !self.is_dylinker()
    }

    /// Whether no dyld loads the image: a -static one, which loads (and
    /// slides) itself, or a kext, which kmutil links into the kernel
    /// by its relocations.
    pub fn without_dyld(&self) -> bool {
        self.static_link || self.is_kext()
    }

    /// Whether the image gets __unwind_info, the table the unwinder
    /// looks a function up in: not a -r output, which carries the
    /// objects' __compact_unwind records instead, nor an image no dyld
    /// loads or one linked with -no_compact_unwind, which unwind by
    /// their __eh_frame alone and so keep every FDE.
    pub fn unwind_info(&self) -> bool {
        !self.relocatable && !self.without_dyld() && !self.no_compact_unwind
    }

    /// Whether the FDEs of the functions that have compact unwind
    /// records reach the output too: in an image without __unwind_info
    /// for the records, and, as ld-prime keeps them, in one for a macOS
    /// before 10.9, whose unwinders ld64 still gave the FDEs (its
    /// -keep_dwarf_unwind default).
    pub fn keeps_all_fdes(&self) -> bool {
        !self.unwind_info()
            || (self.platform == PLATFORM_MACOS && self.platform_minos < encode_version(10, 9, 0))
    }

    /// The output's install name: -install_name, else -final_output,
    /// else the output path.
    pub fn output_install_name(&self) -> &[u8] {
        self.install_name
            .as_deref()
            .or(self.final_output.as_deref())
            .unwrap_or(crate::util::path_bytes(&self.output))
    }
}

/// Rejects the options the kind of output has no use for, as ld-prime
/// does early on: only a main executable has a dyld environment and the
/// MH_PIE flag (one ld-prime ignores in another image dyld loads, with a
/// warning), and a client name is what a bundle or an executable
/// presents to the umbrella it links against. Only dyld reads
/// -no_dynamic_access's mark, and only in what it loads by name. (The
/// other options only a main executable takes ld-prime checks later:
/// -pagezero_size in resolve_pagezero_size, -stack_size in
/// resolve_stack and -e last of all.)
fn check_output_kind(args: &mut Args, pie: Option<bool>) {
    let main_executable = args.output_type == MH_EXECUTE && !args.relocatable;
    // Only dyld reads LC_DYLD_ENVIRONMENT, and only a main executable's.
    if !args.dyld_envs.is_empty() && (!main_executable || args.static_link) {
        fatal!("-dyld_env can only used used when creating a main executables");
    }
    if args.client_name.is_some()
        && (args.relocatable || matches!(args.output_type, MH_DYLIB | MH_DYLINKER))
    {
        fatal!("-client_name can only be used when creating a bundle or main executable");
    }
    let dyld_loaded = args.output_type == MH_DYLIB || (main_executable && !args.static_link);
    if args.no_dynamic_access && !dyld_loaded {
        crate::warn!(
            "-no_dynamic_access ignored. It can only be used with dylibs and main executables"
        );
        args.no_dynamic_access = false;
    }
    if pie == Some(true) && !main_executable {
        if args.relocatable || args.is_dylinker() {
            fatal!("-pie can only be used when linking a main executable");
        }
        crate::warn!("-pie being ignored. It is only used when linking a main executable");
    }
}

/// Rejects in a relocatable object what only a final image lays out, as
/// ld-prime does, in this order: the moves of symbols to other
/// segments - but -dirty_data_list's, which it ignores there - and the
/// __DATA_CONST split, which the link that consumes it decides.
fn check_relocatable(args: &Args, data_const: Option<bool>) {
    if !args.relocatable {
        return;
    }
    if !args.move_to_rw.is_empty() {
        fatal!("-move_to_rw_segment not supported with -r");
    }
    if !args.move_to_ro.is_empty() {
        fatal!("-move_to_ro_segment not supported with -r");
    }
    if data_const == Some(true) {
        fatal!("-data_const not supported with -r");
    }
}

/// Rejects the options at odds with where the image goes: a flat
/// namespace in one bound for the shared cache, whose builder binds
/// each import to the dylib that exports it once and for all (dyld,
/// which binds nothing, may have one), or in a mergeable dylib, whose
/// subsections a two-level image may take in; the debug hook in a
/// mergeable dylib, which only a debug build that merges nothing gets;
/// and the lazy-load and delay-init dylibs in one.
fn check_dylib_use(target: &TargetTraits, args: &Args) {
    if args.flat_namespace && !args.is_dylinker() && shared_region_eligible(target, args) {
        fatal!(
            "Shared cache eligible dylibs cannot use '-flat_namespace'.  Remove '-flat_namespace' \
             or opt out of the shared cache using the build setting 'LD_SHARED_CACHE_ELIGIBLE=NO' \
             (or linker flag '-not_for_dyld_shared_cache')"
        );
    }
    if args.make_mergeable && args.flat_namespace {
        fatal!("-flat_namespace cannot be used with -make_mergeable");
    }
    if args.make_mergeable && args.add_mergeable_debug_hook {
        fatal!("-add_mergeable_debug_hook cannot be used with -make_mergeable");
    }
    // Code reaching a lazy-load or delay-init dylib's symbol is
    // rewritten to call a helper, which a mergeable dylib would record
    // under the compiler's fixup, for no merge to relink: ld-prime
    // aborts on the first and makes a dylib no link can merge of the
    // second. (A -lazy-l the deployment target ignores is a plain -l.)
    if args.make_mergeable && args.lazy_load {
        fatal!("-lazy-l/-lazy_library/-lazy_framework cannot be used with -make_mergeable");
    }
    let delay = |arg: &InputArg| matches!(arg, InputArg::Library(LibraryKind::Delay, _));
    if args.make_mergeable && args.inputs.iter().any(delay) {
        fatal!("-delay-l/-delay_library/-delay_framework cannot be used with -make_mergeable");
    }
}

/// -stack_size and -stack_addr. No dyld starts an executable that starts
/// from LC_UNIXTHREAD with LC_MAIN's stack size: its stack is a segment
/// of its own, __UNIXSTACK, which ld64 pins as -segaddr would (unless
/// one pins it elsewhere) below a top of stack, -stack_addr's or a fixed
/// one; LC_UNIXTHREAD's stack pointer starts at its end. ld-prime checks
/// -stack_addr for a multiple of the page size (the segment alignment,
/// but 4 KiB in an object file) and a size to go with it, then the size
/// against the most a stack may take on the target and for a main
/// executable, one that starts from LC_UNIXTHREAD if it has an address,
/// then for a multiple of the page size and smaller than the address.
fn resolve_stack(target: &TargetTraits, args: &mut Args, size: Option<u64>, addr: Option<u64>) {
    if addr == Some(0) {
        crate::warn!("-stack_addr 0x0 has no effect");
    }
    let addr = addr.filter(|&addr| addr != 0);
    if let Some(addr) = addr {
        let page = if args.relocatable { 0x1000 } else { args.segment_align };
        if !addr.is_multiple_of(page) {
            fatal!("-stack_addr (0x{addr:08X}) must be multiples of page size (0x{page:08X})");
        }
        if size.unwrap_or(0) == 0 {
            fatal!("-stack_addr must be used with -stack_size");
        }
    }
    let Some(size) = size else { return };
    if size == 0 {
        crate::warn!("-stack_size 0x0 has no effect");
        return;
    }
    let macos_x86_64 = target.name == "x86_64" && args.platform == PLATFORM_MACOS;
    if macos_x86_64 && size > 0x100_0000_0000 {
        fatal!("-stack_size must be <= 1TB on x86_64 macOS");
    }
    if !macos_x86_64 && size > 0x2000_0000 {
        fatal!("-stack_size must be <= 512MB on {} platforms", target.name);
    }
    if args.output_type != MH_EXECUTE || args.relocatable || args.preload {
        fatal!("-stack_size option can only be used when linking a main executable");
    }
    if addr.is_some() && !args.unixthread {
        fatal!("-stack_addr can't be used with modern executables");
    }
    let page = args.segment_align;
    if !size.is_multiple_of(page) {
        fatal!("-stack_size (0x{size:08X}) must be multiples of page size (0x{page:08X})");
    }
    let default_top = if macos_x86_64 { 0x7fff_5c00_0000 } else { 0x1_2000_0000 };
    let top = addr.unwrap_or(default_top);
    if size > top {
        fatal!("-stack_size (0x{size:08X}) must be smaller than -stack_addr (0x{top:08X})");
    }
    args.stack_size = size;
    if args.unixthread && args.segaddr(b"__UNIXSTACK").is_none() {
        args.segaddrs.push((b"__UNIXSTACK".to_vec(), top - size));
    }
}

/// Whether an executable is position independent (MH_PIE). It is
/// unless -no_pie says otherwise, which arm64 ignores (arm64 macOS runs
/// PIE executables only) and which ld-prime deprecates from the OS
/// versions that default to chained fixups. An x86-64 one is PIE by
/// default from macOS 10.6 on, as ld64 made it, and only with -pie for
/// an older macOS. A -static image (a kernel) is PIE only with -pie or
/// -kernel, or with -fixup_chains, whose chains exist to slide it.
fn resolve_pie(
    target: &TargetTraits,
    args: &Args,
    pie: Option<bool>,
    fixup_chains: Option<bool>,
) -> bool {
    match pie {
        Some(false) if args.output_type == MH_EXECUTE && !args.static_link && !args.relocatable => {
            if is_new_os(target.name, MH_EXECUTE, args.platform, args.platform_minos) {
                crate::warn!("-no_pie is deprecated when targeting new OS versions");
            }
            if target.name == "arm64" {
                crate::warn!("-no_pie ignored for arm64*");
            }
            target.name == "arm64"
        }
        Some(pie) => pie,
        None => {
            (!args.static_link && !is_before_x86_64_macos_10_6(target, args))
                || args.kernel
                || fixup_chains == Some(true)
        }
    }
}

/// Whether the image is for x86-64 macOS before 10.6.
fn is_before_x86_64_macos_10_6(target: &TargetTraits, args: &Args) -> bool {
    target.name == "x86_64"
        && args.platform == PLATFORM_MACOS
        && args.platform_minos < encode_version(10, 6, 0)
}

/// Whether an image dyld loads goes without LC_DYLD_INFO, the opcode
/// streams that came with macOS 10.6, as ld-prime links one for x86-64
/// macOS before that (arm64 macOS has none so old). Its legacy LINKEDIT
/// gives dyld the same facts as older dyld read them: what each GOT
/// slot and lazy pointer binds to by the indirect symbol table, what
/// data pointers bind to by external relocations, and what pointers
/// slide by local relocations. dyld binds a lazy pointer on the first
/// call through it, entering dyld_stub_binding_helper, which crt1.o
/// (dylib1.o, bundle1.o) defines, from the pointer's stub helper entry.
fn resolve_legacy_linkedit(target: &TargetTraits, args: &Args) -> bool {
    is_before_x86_64_macos_10_6(target, args)
        && !args.relocatable
        && !args.without_dyld()
        && !args.fixup_chains
}

/// Whether the image is laid out for chained fixups (see
/// Args::fixup_chains).
fn resolve_fixup_chains(target: &TargetTraits, args: &Args, fixup_chains: Option<bool>) -> bool {
    // A static executable has no dyld: it has no chains unless
    // -fixup_chains asks for them, which its own loader then walks
    // (a -kernel image cannot). A kext has none either: kmutil
    // links it into the kernel by its relocations.
    if args.is_kext() {
        return false;
    }
    if args.static_link {
        return fixup_chains == Some(true);
    }
    // ld-prime's defaults: chained fixups from macOS 12 on arm64 and
    // from macOS 13 on x86_64 (below that, classic dyld info with
    // lazy binding), and never under -undefined dynamic_lookup or
    // suppress - only an explicit -fixup_chains overrides that.
    fixup_chains.unwrap_or_else(|| {
        !args.undefined_dynamic_lookup && chained_fixups_by_default(target, args)
    })
}

/// Whether the image defaults to chained fixups: its deployment target
/// is new enough, and it is not a non-PIE executable, which ld-prime
/// gives classic dyld info whatever the target.
fn chained_fixups_by_default(target: &TargetTraits, args: &Args) -> bool {
    (args.pie || args.output_type != MH_EXECUTE)
        && is_new_os(target.name, args.output_type, args.platform, args.platform_minos)
}

/// Whether ld-prime gives the image __DATA_CONST, the segment dyld makes
/// read-only once it has applied the fixups, when neither -data_const
/// nor -no_data_const says. An image no dyld loads has one only if
/// bound for the shared region: nothing else would make it read-only.
/// A non-PIE executable, which keeps its classic layout, has none; an
/// image bound for the shared region and firmware have one. On macOS,
/// ld64 gives one from its version2019Fall (10.15) on, but not for
/// 10.15.4 up to 10.16, and not with -no_pie, even where the option is
/// otherwise ignored (an arm64 executable, a dylib or a bundle).
fn default_data_const(args: &Args, pie: Option<bool>) -> bool {
    if args.without_dyld() {
        return args.shared_region;
    }
    if args.output_type == MH_EXECUTE && !args.pie {
        return false;
    }
    if args.shared_region || args.platform == PLATFORM_FIRMWARE {
        return true;
    }
    let minos = args.platform_minos;
    pie != Some(false)
        && minos >= encode_version(10, 15, 0)
        && !(encode_version(10, 15, 4)..encode_version(10, 16, 0)).contains(&minos)
}

/// Whether the command line may lay out the image's segments and
/// sections: an image no dyld loads (a -static or a -preload one) or
/// firmware (ld-prime also allows sepOS, which mold has not).
fn custom_layout(args: &Args) -> bool {
    args.static_link || args.platform == PLATFORM_FIRMWARE
}

fn check_segment_order(args: &Args) {
    if !args.segment_order.is_empty() && args.segment_order.len() < 2 {
        fatal!("-segment_order should specifify at least two segments");
    }
}

/// In an image dyld loads, __DATA_CONST is one of the standard
/// segments, and ld-prime puts it right before __DATA where
/// -segment_order names that alone - with a warning, which comes even
/// before it refuses the option for an image that may not order its
/// segments (only firmware may).
fn complete_segment_order(args: &mut Args) {
    let has = |name: &[u8]| args.segment_order.iter().position(|s| s == name);
    if !args.without_dyld()
        && args.data_const
        && has(b"__DATA_CONST").is_none()
        && let Some(i) = has(b"__DATA")
    {
        crate::warn!(
            "-segment_order lists __DATA, but not __DATA_CONST, assuming standard order. list __DATA_CONST explicitly or disable the segment using -no_data_const"
        );
        args.segment_order.insert(i, b"__DATA_CONST".to_vec());
    }
    if !args.segment_order.is_empty() && !custom_layout(args) {
        fatal!(
            "-segment_order can only be used with -preload, -static, or with -platform_version \"firmware\"/\"sepOS\""
        );
    }
}

/// dyld may order its sections too, though not its segments.
fn check_section_order(args: &Args) {
    if !args.section_order.is_empty() && !custom_layout(args) && !args.is_dylinker() {
        fatal!(
            "-section_order can only be used with -preload, -dylinker, -static, or with -platform_version \"firmware\"/\"sepOS\""
        );
    }
}

/// ld-prime deprecates -flat_namespace on every platform but macOS, and
/// takes -read_only_relocs only where it may allow text relocations.
fn warn_platform_options(target: &TargetTraits, args: &Args, read_only_relocs: bool) {
    if args.flat_namespace && args.platform == PLATFORM_FIRMWARE {
        crate::warn!("-flat_namespace is deprecated on firmware");
    }
    if read_only_relocs && !read_only_relocs_apply(target, args) {
        crate::warn!("-read_only_relocs relocs cannot be used in this configuration");
    }
}

/// Whether -read_only_relocs decides on text relocations: in firmware,
/// in an image no dyld loads but an arm64 kext, and in -r (which has
/// none).
fn read_only_relocs_apply(target: &TargetTraits, args: &Args) -> bool {
    args.relocatable
        || args.static_link
        || args.platform == PLATFORM_FIRMWARE
        || (args.is_kext() && target.name == "x86_64")
}

/// Whether a pointer may need a fixup in a segment mapped without write
/// permission, which the loader would have to make writable (a text
/// relocation). ld-prime allows one by default only in an x86-64 kext
/// or non-PIE executable. -read_only_relocs warning or suppress allows
/// one where the option applies, and error refuses it; elsewhere the
/// option, ignored with a warning, leaves none allowed.
fn resolve_text_relocs(target: &TargetTraits, args: &Args, read_only_relocs: Option<bool>) -> bool {
    match read_only_relocs {
        Some(allow) => allow && read_only_relocs_apply(target, args),
        None => {
            target.name == "x86_64"
                && (args.is_kext() || (args.output_type == MH_EXECUTE && !args.pie))
        }
    }
}

/// -segaddr's (segment, address) pins, one per segment: ld-prime takes
/// the last address given for a segment, with a warning.
fn resolve_segaddrs(segaddrs: Vec<(Vec<u8>, u64)>) -> Vec<(Vec<u8>, u64)> {
    let mut out: Vec<(Vec<u8>, u64)> = Vec::new();
    for (name, addr) in segaddrs {
        match out.iter_mut().find(|(seen, _)| *seen == name) {
            Some((_, old)) if *old == addr => {
                crate::warn!("-segaddr {} used more than once", raw(&name))
            }
            Some((_, old)) => {
                crate::warn!("-segaddr {} has conflicting values, using 0x{addr:X}", raw(&name));
                *old = addr;
            }
            None => out.push((name, addr)),
        }
    }
    out
}

/// -segprot's (segment, max, init) protections, as ld-prime applies
/// them: the first one given for a segment wins, and arm64's maximum
/// is its initial protection (nothing may raise it later there).
fn resolve_segprots(
    target: &TargetTraits,
    segprots: Vec<(Vec<u8>, u8, u8)>,
) -> Vec<(Vec<u8>, u8, u8)> {
    let mut out: Vec<(Vec<u8>, u8, u8)> = Vec::new();
    for (name, max, init) in segprots {
        if out.iter().all(|(seen, _, _)| *seen != name) {
            out.push((name, if target.name == "arm64" { init } else { max }, init));
        }
    }
    out
}

/// The segment alignment: -segalign's, rounded down to a power of two
/// with a warning as ld-prime does (the last one given wins), else the
/// page size (4 KiB for a -preload image). ld-prime takes 0 as it is:
/// pages of no size, and so segments of none, which fails a final link
/// (see passes::check_segments).
fn resolve_segment_align(target: &TargetTraits, args: &Args, segalign: Option<u64>) -> u64 {
    match segalign {
        // -encryptable gives an image 16 KiB pages for 4 KiB ones, as
        // iOS has, though ld-prime then makes it no encryptable one
        // (see resolve_encryptable).
        None if args.encryptable => target.page_size.max(0x4000),
        Some(0x1000) if args.encryptable => 0x4000,
        None if args.preload => 0x1000,
        None => target.page_size,
        Some(align) if align == 0 || align.is_power_of_two() => align,
        Some(align) => {
            let p2 = 1 << align.ilog2();
            crate::warn!(
                "alignment for -segalign 0x{align:X} is not a power of two, using 0x{p2:X}"
            );
            p2
        }
    }
}

/// Whether the image is encryptable: an image dyld or the kernel loads
/// (ld-prime gives a -r or -preload output no LC_ENCRYPTION_INFO_64,
/// and crashes on a kext), as -encryptable says; macOS images are not
/// by default. (ld64 made iOS apps encryptable unless $LD_NO_ENCRYPT,
/// which ld-prime reads but which -encryptable overrides.)
fn resolve_encryptable(args: &mut Args) {
    args.encryptable &= !args.relocatable && !args.preload && !args.is_kext();
}

/// -seg_page_size's (segment, size) pairs, as ld-prime takes them: a
/// size rounds down to a power of two, with a warning; one below the
/// page size (the segment alignment) is an error but in an object file,
/// where it means nothing; and the first size given for a segment wins.
fn resolve_seg_page_sizes(args: &Args, sizes: Vec<(Vec<u8>, u64)>) -> Vec<(Vec<u8>, u64)> {
    let page = args.segment_align;
    let mut out: Vec<(Vec<u8>, u64)> = Vec::new();
    for (name, mut size) in sizes {
        if size != 0 && !size.is_power_of_two() {
            size = 1 << size.ilog2();
            crate::warn!(
                "-seg_page_size for {} is not a power of two, rounding down to 0x{size:x}",
                raw(&name)
            );
        }
        if size < page && !args.relocatable {
            fatal!(
                "-seg_page_size {} 0x{size:x} can't be smaller than page size (0x{page:x})",
                raw(&name)
            );
        }
        if out.iter().all(|(seen, _)| *seen != name) {
            out.push((name, size));
        }
    }
    out
}

/// -pagezero_size, as ld-prime takes it: only for a main executable
/// (not a -preload one), rounded up to a page (past the top, to 0), and
/// no more than 4 GiB in an executable with chained fixups. A dylib is
/// loaded at an arbitrary address, and a -preload image copied to
/// wherever its segments say; only a main executable reserves the low
/// 4 GiB against NULL dereferences. A -kernel image, which ld-prime
/// makes position independent for the kernel collection to slide, has
/// none unless -pagezero_size asks.
fn resolve_pagezero_size(args: &mut Args) {
    let has_pagezero = args.output_type == MH_EXECUTE && !args.relocatable && !args.preload;
    if !has_pagezero && args.explicit_pagezero && args.pagezero_size != 0 {
        fatal!("-pagezero_size can only be used when linking a main executable");
    }
    if args.output_type != MH_EXECUTE || args.preload || (args.kernel && !args.explicit_pagezero) {
        args.pagezero_size = 0;
    }
    if args.relocatable {
        return;
    }
    let size = args.pagezero_size;
    let page = args.segment_align;
    if !size.is_multiple_of(page) {
        let aligned = page_align(size, page);
        // (As printf's %#llx spells it.)
        let shown = if aligned == 0 { "0".to_string() } else { format!("{aligned:#x}") };
        crate::warn!(
            "-pagezero_size not aligned, rounded up to: {shown}, use -segalign to change the alignment"
        );
        args.pagezero_size = aligned;
    }
    if args.output_type == MH_EXECUTE && args.fixup_chains && args.pagezero_size > 0x1_0000_0000 {
        crate::warn!("-pagezero_size is too large, setting it to 4GB");
        args.pagezero_size = 0x1_0000_0000;
    }
}

/// ld-prime's checks of the -segaddr pins, one pin at a time: a pin may
/// not lie in __PAGEZERO, share its address with another one, or be off
/// a page boundary - even one for a segment the image does not have.
fn check_segaddrs(args: &Args) {
    if args.relocatable {
        return;
    }
    let segaddrs = &args.segaddrs;
    for (i, (name, addr)) in segaddrs.iter().enumerate() {
        let name = raw(name);
        if *addr < args.pagezero_size {
            fatal!("-segaddr {name} 0x{addr:X} conflicts with -pagezero_size");
        }
        if let Some((other, _)) = segaddrs[i + 1..].iter().find(|(_, a)| a == addr) {
            fatal!("duplicate -segaddr addresses for {name} and {}", raw(other));
        }
        if !addr.is_multiple_of(args.segment_align) {
            fatal!(
                "-segaddr {name} 0x{addr:X} is not aligned to the page size ({:#x}), use -segalign to change it",
                args.segment_align
            );
        }
    }
}

/// -image_base (or -seg1addr) sets __TEXT's address, for an image that
/// stays where it was linked. A -segaddr for __TEXT names the same
/// address, and the two must agree (a non-PIE -static image takes the
/// -segaddr's, with a warning). dyld slides a PIE executable wherever
/// it likes, and ld-prime ignores the base for one with a warning; it
/// ignores it too for any other image dyld loads with chained fixups (a
/// non-PIE executable only when -fixup_chains asks for them). A pinned
/// __TEXT stays where it is even then: in a dylib the other segments
/// still follow it, while in a PIE executable they go from __PAGEZERO's
/// end and so below it, out of order (passes::place_segments).
fn resolve_image_base(args: &mut Args) {
    // Before anything else looks at it, ld-prime rounds a base up to a
    // page (past the top, to 0): 4 KiB in an object file, which is
    // loaded nowhere.
    let align = if args.relocatable { 0x1000 } else { args.segment_align };
    if let Some(base) = args.image_base
        && !base.is_multiple_of(align)
    {
        let aligned = page_align(base, align);
        crate::warn!(
            "base address 0x{base:X} is not properly aligned. Changing it to 0x{aligned:X}"
        );
        args.image_base = Some(aligned);
    }
    // It takes a zero base as none at all.
    if args.image_base == Some(0) {
        args.image_base = None;
    }
    if args.relocatable {
        args.image_base = None;
        return;
    }

    let text = args.segaddr(b"__TEXT");
    if let (Some(base), Some(text)) = (args.image_base, text)
        && base != text
    {
        if !args.static_link || args.pie {
            fatal!("-image_base and -segaddr __TEXT must match");
        }
        crate::warn!(
            "-image_base and -segaddr __TEXT must match, changing image base to {text:#x}"
        );
    }
    let Some(base) = text.or(args.image_base) else { return };
    args.image_base = Some(base);

    if args.output_type == MH_EXECUTE && args.pie && !args.static_link {
        crate::warn!("Linking with PIE, -image_base will be ignored");
        args.image_base = None;
    } else if !args.static_link && args.fixup_chains {
        crate::warn!("prefered load addresses (-seg1addr) are disabled with chained fixups");
        args.image_base = text;
    }
}

/// ld-prime vets -lto_library before the rest of the command line: it
/// loads the library in place of its own by running itself again with
/// the library's directory first in the dynamic loader's search path,
/// so every one given must be named libLTO.dylib. The last one counts,
/// unless no such file exists - then ld-prime warns and keeps its own.
fn resolve_lto_library(mut paths: Vec<PathBuf>) -> Option<PathBuf> {
    if paths.iter().any(|path| path.file_name() != Some(OsStr::new("libLTO.dylib"))) {
        fatal!("-lto_library library filename must be 'libLTO.dylib'");
    }
    let path = paths.pop()?;
    if std::fs::metadata(&path).is_err() {
        crate::warn!("ignoring -lto_library '{}', file does not exist", path.display());
        return None;
    }
    Some(path)
}

/// Without -arch, ld-prime links for the target of the first object
/// file named on the command line: a Mach-O object's CPU type, or a
/// bitcode file's target triple. Archives, dylibs and universal files
/// don't count, and without such an object there is no target.
fn detect_target(args: &Args) -> &'static str {
    for input in &args.inputs {
        let (InputArg::File(path) | InputArg::Listed(path)) = input else { continue };
        let Some(mf) = open_for_target(path) else { continue };
        match get_file_type(mf) {
            FileType::Object => {
                if let Some(name) = crate::filetype::get_macho_target(mf.data()) {
                    return name;
                }
            }
            FileType::LlvmBitcode => {
                let plugin = crate::lto::load_plugin(args.lto_library.as_deref());
                let triple = crate::lto::target_triple(&plugin, mf.data(), &mf.name);
                let arch = triple.split('-').next().unwrap_or_default();
                return target_arch(arch)
                    .unwrap_or_else(|| fatal!("unknown architecture in target triple '{triple}'"));
            }
            _ => {}
        }
    }
    fatal!("Missing -arch option");
}

/// An input file ld-prime reads before the link proper to work out the
/// target (see detect_target and infer_platform): None for an empty
/// one, which says nothing. A file it can't map stops it, in words that
/// name no input.
fn open_for_target(path: &Path) -> Option<&'static MappedFile> {
    match MappedFile::try_open(path) {
        Ok(mf) => (mf.size() > 0).then_some(mf),
        Err(e) => fatal!("{}", crate::passes::unreadable_file(path, &e)),
    }
}

/// Without -platform_version (or -macos_version_min or -target),
/// ld-prime links for what the first object file named on the command
/// line that has a platform load command was built for: its platform,
/// minimum OS and SDK versions, whatever later objects say (one built
/// for a newer OS draws a warning, one for another platform an error).
/// Archive members, universal files and dylibs don't count, nor does a
/// bitcode file unless no Mach-O object does: then the first one's
/// target triple names the platform and OS version, and no SDK. A final
/// image must have a platform; a -r or -preload output may be for none.
fn infer_platform(args: &mut Args) {
    let mut bitcode = None;
    for input in &args.inputs {
        let (InputArg::File(path) | InputArg::Listed(path)) = input else { continue };
        let Some(mf) = open_for_target(path) else { continue };
        match get_file_type(mf) {
            FileType::Object => {
                let Some(v) = PlatformVersion::of_object(mf.data()) else {
                    continue;
                };
                if v.platform != PLATFORM_MACOS && v.platform != PLATFORM_FIRMWARE {
                    fatal!(
                        "{}: unsupported platform: {}",
                        mf.name.display(),
                        platform_name(v.platform)
                    );
                }
                args.platform = v.platform;
                args.platform_minos = v.minos;
                args.platform_sdk = v.sdk;
                return;
            }
            FileType::LlvmBitcode => {
                bitcode.get_or_insert(mf);
            }
            _ => {}
        }
    }
    if let Some(mf) = bitcode {
        let plugin = crate::lto::load_plugin(args.lto_library.as_deref());
        let triple = crate::lto::target_triple(&plugin, mf.data(), &mf.name);
        (_, args.platform, args.platform_minos) = parse_triple(&triple);
    } else if !args.relocatable && !args.preload {
        fatal!("Missing -platform_version option");
    }
}
