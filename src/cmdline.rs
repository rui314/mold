//! Command line parsing.
//!
//! The command line is compatible with Apple's ld64: options are single-dash
//! long names, and input files and `-l` options are position-dependent.

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::io::IsTerminal;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use crate::fatal;
use crate::macho::*;
use crate::mapped_file::MappedFile;
use crate::util::glob::{Glob, GlobBuilder};
use crate::util::{display, os_str};

/// The Apple ld64 version whose command line this linker implements,
/// reported by -version_details. Xcode passes flags according to this
/// number (Xcode 26.6 ships ld-1267).
pub const LD64_COMPAT_VERSION: &str = "1267";

/// An input in command line order. Paths keep the bytes they were given
/// in; library and framework names are OS strings, since they become
/// path components.
#[derive(Clone, Debug)]
pub enum InputArg {
    /// A file path.
    File(PathBuf),
    /// `-lfoo`: a library to search for in the library paths. The flag
    /// marks a weak library (`-weak-lfoo`).
    Lib(OsString, bool),
    /// `-framework Foo`: a framework to search for in the framework
    /// paths. The flag marks a weak framework.
    Framework(OsString, bool),
    /// `-force_load path`: an archive all of whose members are linked.
    ForceLoad(PathBuf),
    /// `-weak_library path`: a dylib whose absence is tolerated at load
    /// time.
    WeakFile(PathBuf),
    /// `-reexport-lfoo` / `-reexport_library path`: a dylib whose
    /// exports this dylib re-exports as its own.
    ReexportLib(OsString),
    ReexportFile(PathBuf),
    ReexportFramework(OsString),
    /// `-hidden-lfoo`: an archive whose external symbols are demoted
    /// to private externals.
    HiddenLib(OsString),
    /// `-needed-lfoo` / `-needed_framework Foo`: always keep the
    /// dylib's load command.
    NeededLib(OsString),
    NeededFramework(OsString),
    NeededFile(PathBuf),
    /// `-bundle_loader path`: the executable a bundle's undefined
    /// symbols may resolve to, bound at run time as the main executable.
    BundleLoader(PathBuf),
}

/// Parsed command line arguments.
#[derive(Debug)]
pub struct Args {
    pub output: PathBuf,
    /// The output file type: MH_EXECUTE, MH_DYLIB or MH_BUNDLE.
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
    /// no dyld loads).
    pub entry: String,
    pub platform: u32,
    pub platform_minos: u32,
    pub platform_sdk: u32,
    pub syslibroot: Vec<PathBuf>,
    pub library_paths: Vec<PathBuf>,
    pub framework_paths: Vec<PathBuf>,
    pub inputs: Vec<InputArg>,
    /// -rpath: LC_RPATH strings, as given.
    pub rpaths: Vec<Vec<u8>>,
    /// -adhoc_codesign / -no_adhoc_codesign. None means "decide from
    /// the target".
    pub adhoc_codesign: Option<bool>,
    pub dead_strip: bool,
    /// -S: do not emit debug stab symbols.
    pub strip_debug: bool,
    pub all_load: bool,
    pub load_objc: bool,
    /// Symbols to treat as undefined from the start, forcing archive
    /// members that define them to be linked: those -u names, and those
    /// an export list names without wildcards (ld64's "initial
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
    /// The names in -reexported_symbols_list given without wildcards,
    /// each of which must resolve.
    pub reexported_names: Vec<String>,
    pub current_version: u32,
    pub compatibility_version: u32,
    /// -map: write a map file describing the output layout.
    pub map: Option<PathBuf>,
    /// -dependency_info: write Xcode's binary dependency listing.
    pub dependency_info: Option<PathBuf>,
    /// -sdk_imports: Xcode's JSON report of imported APIs.
    pub sdk_imports: Option<PathBuf>,
    /// Emit chained fixups instead of classic dyld info. None means
    /// "decide from the deployment target".
    pub fixup_chains: Option<bool>,
    /// The libLTO to load for bitcode inputs (-lto_library).
    pub lto_library: Option<PathBuf>,
    /// -stack_size: the main thread's stack size, recorded in LC_MAIN.
    pub stack_size: u64,
    /// -sectcreate: sections to synthesize from files:
    /// (segment, section, path).
    pub sectcreate: Vec<(String, String, PathBuf)>,
    /// -add_empty_section: zero-length sections to synthesize:
    /// (segment, section).
    pub add_empty_section: Vec<(String, String)>,
    /// -r: produce a relocatable object instead of a final image.
    pub relocatable: bool,
    /// -flat_namespace: bind imports by name across all loaded images
    /// instead of to specific dylibs.
    pub flat_namespace: bool,
    /// -Z: do not search the standard library and framework
    /// directories.
    pub no_standard_dirs: bool,
    /// -x: strip non-global symbols from the output symbol table.
    pub strip_locals: bool,
    /// Fold identical functions (on by default; -no_deduplicate turns
    /// it off and a later -deduplicate back on).
    pub deduplicate: bool,
    /// Emit LC_FUNCTION_STARTS (on by default but in a -static image).
    pub function_starts: bool,
    /// Emit LC_DATA_IN_CODE (on by default but in a -static image).
    pub data_in_code_info: bool,
    /// -version_load_command: give a -static image LC_BUILD_VERSION.
    pub version_load_command: bool,
    /// -add_split_seg_info: emit LC_SEGMENT_SPLIT_INFO, which lets a
    /// dyld shared cache or kernel collection builder slide the
    /// segments apart. ld64 and ld-prime have no negative form.
    pub add_split_seg_info: bool,
    /// -init_offsets: emit initializers as 32-bit image offsets
    /// (__init_offsets) instead of absolute pointers (__mod_init_func).
    pub init_offsets: bool,
    /// -data_const / -no_data_const: whether read-only-after-fixup data
    /// sections (__const, __cfstring, the ObjC lists, __got ...) go in
    /// a __DATA_CONST segment. ld64's default is on but for a -static
    /// image.
    pub data_const: bool,
    /// -no_implicit_dylibs: do not bind through a re-export to the
    /// defining dylib (and add a load command for it); bind to the
    /// re-exporting dylib named on the command line instead.
    pub no_implicit_dylibs: bool,
    /// -objc_relative_method_lists / -no_objc_relative_method_lists:
    /// rewrite Objective-C method lists in the relative form. ld64's
    /// default is on from macOS 11.
    pub objc_relative_method_lists: Option<bool>,
    /// Merge categories into the classes defined in the same image, as
    /// ld64 does unless -no_objc_category_merging is given (there is no
    /// option to turn it on).
    pub objc_category_merging: bool,
    /// Compute a content-hash LC_UUID (on by default; -no_uuid leaves
    /// it zeroed - dyld refuses executables without the load command).
    pub uuid: bool,
    /// -w: suppress warnings.
    pub suppress_warnings: bool,
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
    /// -warn_unused_dylibs / -no_warn_unused_dylibs: warn about linked
    /// dylibs nothing binds to (by default only for a dylib bound for
    /// the dyld shared cache).
    pub warn_unused_dylibs: Option<bool>,
    /// -not_for_dyld_shared_cache: a dylib installed in /usr/lib or
    /// /System/Library that won't go into the dyld shared cache.
    pub not_for_dyld_shared_cache: bool,
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
    /// -bind_at_load: ask dyld to resolve all bindings at load time.
    pub bind_at_load: bool,
    /// -application_extension: mark the image safe for app extensions.
    pub application_extension: bool,
    /// -add_ast_path: Swift AST paths recorded as N_AST stabs for the
    /// debugger.
    pub add_ast_paths: Vec<PathBuf>,
    pub dynamic: bool,
    /// -headerpad: the space left free after the load commands (32
    /// unless given; a final image never gets less).
    pub headerpad: u64,
    /// -headerpad_max_install_names: room for every dylib load command
    /// to grow to MAXPATHLEN.
    pub headerpad_max_install_names: bool,
    /// -search_dylibs_first: search every path for a dylib before
    /// falling back to archives.
    pub search_dylibs_first: bool,
    /// -umbrella: declare this dylib a subframework of the named
    /// umbrella framework (LC_SUB_FRAMEWORK).
    pub umbrella: Option<Vec<u8>>,
    /// -oso_prefix: prefix to strip from N_OSO stab paths ("."  means
    /// the current directory).
    pub oso_prefix: Option<Vec<u8>>,
    /// -mark_dead_strippable_dylib: mark the output dylib as
    /// removable when a client binds nothing from it.
    pub mark_dead_strippable_dylib: bool,
    /// -export_dynamic: keep all global symbols through LTO even in an
    /// executable, for dlsym or plugin use.
    pub export_dynamic: bool,
    /// -order_file: files of symbol names; matching atoms are placed
    /// first in their output sections, in file order.
    pub order_files: Vec<PathBuf>,
    /// -executable_path: what @executable_path in dependent dylibs'
    /// install names stands for at link time (defaults to the output
    /// path when linking an executable).
    pub executable_path: Option<PathBuf>,
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
    /// -alias/-alias_list: (existing, new) symbol aliases to define.
    pub aliases: Vec<(String, String)>,
    /// -sectalign: (segment, section, p2align) overrides.
    pub sectalign: Vec<(String, String, u8)>,
    /// -allowable_client: clients that may link this subframework
    /// (LC_SUB_CLIENT).
    pub allowable_clients: Vec<Vec<u8>>,
    /// -client_name: the name this link presents when checking
    /// subframework restrictions.
    pub client_name: Option<Vec<u8>>,
    /// -t: print each file that takes part in the link.
    pub trace: bool,
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
    pub segaddrs: Vec<(String, u64)>,
    /// -segprot: (segment, max, init) protections.
    pub segprots: Vec<(String, u8, u8)>,
    /// -segment_order: segment names in output order.
    pub segment_order: Vec<String>,
    /// -rename_section: (old_seg, old_sect, new_seg, new_sect).
    pub rename_sections: Vec<(String, String, String, String)>,
    /// -rename_segment: (old, new).
    pub rename_segments: Vec<(String, String)>,
    /// ZERO_AR_DATE is set: the stabs record no modification times.
    pub zero_ar_date: bool,
    /// -static: an image no dyld loads (the XNU kernel), with no LC_MAIN
    /// or imports, and fixups only if -fixup_chains or -no_fixup_chains
    /// asks.
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
    /// Code goes in its own __TEXT_EXEC segment, and __TEXT is
    /// read-only (ld64's -text_exec, implied by an arm64 -kext).
    pub text_exec: bool,
    /// Whether an executable is position independent (MH_PIE):
    /// -pie / -no_pie, resolved for the target at the end of parsing.
    pub pie: bool,
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
            platform: PLATFORM_MACOS,
            platform_minos: encode_version(0, 0, 0),
            platform_sdk: encode_version(0, 0, 0),
            syslibroot: Vec::new(),
            library_paths: Vec::new(),
            framework_paths: Vec::new(),
            inputs: Vec::new(),
            rpaths: Vec::new(),
            adhoc_codesign: None,
            dead_strip: false,
            strip_debug: false,
            all_load: false,
            load_objc: false,
            forced_undefined: Vec::new(),
            exported_symbols: None,
            no_exported_symbols: false,
            unexported_symbols: Glob::new(),
            reexported_symbols: Glob::new(),
            reexported_names: Vec::new(),
            // ld64 leaves both at 0.0.0 unless -current_version /
            // -compatibility_version say otherwise.
            current_version: encode_version(0, 0, 0),
            compatibility_version: encode_version(0, 0, 0),
            map: None,
            dependency_info: None,
            sdk_imports: None,
            fixup_chains: None,
            lto_library: None,
            stack_size: 0,
            sectcreate: Vec::new(),
            add_empty_section: Vec::new(),
            relocatable: false,
            flat_namespace: false,
            no_standard_dirs: false,
            strip_locals: false,
            deduplicate: true,
            function_starts: true,
            data_in_code_info: true,
            version_load_command: false,
            add_split_seg_info: false,
            init_offsets: false,
            data_const: true,
            no_implicit_dylibs: false,
            objc_relative_method_lists: None,
            objc_category_merging: true,
            uuid: true,
            suppress_warnings: false,
            fatal_warnings: false,
            demangle: false,
            undefined_dynamic_lookup: false,
            allowed_undefined: Vec::new(),
            dead_strip_dylibs: false,
            warn_unused_dylibs: None,
            not_for_dyld_shared_cache: false,
            debug_variant: false,
            shared_region: false,
            no_inits: false,
            no_warn_inits: false,
            bind_at_load: false,
            application_extension: false,
            add_ast_paths: Vec::new(),
            dynamic: true,
            headerpad: 32,
            headerpad_max_install_names: false,
            search_dylibs_first: false,
            umbrella: None,
            oso_prefix: None,
            mark_dead_strippable_dylib: false,
            export_dynamic: false,
            order_files: Vec::new(),
            executable_path: None,
            object_path_lto: None,
            print_dependencies: false,
            why_load: false,
            why_live: Glob::new(),
            aliases: Vec::new(),
            sectalign: Vec::new(),
            allowable_clients: Vec::new(),
            client_name: None,
            trace: false,
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
            rename_sections: Vec::new(),
            rename_segments: Vec::new(),
            zero_ar_date: false,
            static_link: false,
            preload: false,
            kernel: false,
            text_exec: false,
            pie: true,
        }
    }
}

impl Args {
    /// The address -segaddr pins a segment to.
    pub fn segaddr(&self, segname: &str) -> Option<u64> {
        self.segaddrs.iter().find(|(name, _)| name == segname).map(|&(_, addr)| addr)
    }
}

/// Parses an X.Y.Z version string.
fn parse_version(arg: &str) -> u32 {
    let mut it = arg.split('.');
    let mut next = |what| match it.next() {
        None => 0,
        Some(s) => match s.parse() {
            Ok(num) => num,
            Err(_) => fatal!("malformed version number: {what}: {arg}"),
        },
    };
    let major = next("major");
    let minor = next("minor");
    let patch = next("patch");
    encode_version(major, minor, patch)
}

/// ld64 takes the platform by name or by its PLATFORM_* number; Xcode
/// passes the number for some prelink steps (`-platform_version 1 11.0`).
fn parse_platform(arg: &str) -> u32 {
    match arg {
        "macos" | "macosx" | "1" => PLATFORM_MACOS,
        _ => fatal!("unsupported platform: {arg}"),
    }
}

/// Parses a symbol list file: one symbol per line, '#' starts a
/// comment.
/// Reads a symbol-list file for an option, fatal on I/O error.
/// Adds symbol-list patterns to a matcher. ld64's lists accept `*`, `?`
/// and `[...]` wildcards.
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

/// Whether a symbol list entry is a wildcard pattern rather than a name.
fn is_pattern(s: &str) -> bool {
    s.contains(['*', '?', '['])
}

/// Makes the names among a symbol list's entries initial undefines: an
/// object need not mention them for them to pull in an archive member,
/// and each must resolve. Patterns only match symbols already there.
fn add_initial_undefines<'a>(undefs: &mut Vec<String>, entries: impl IntoIterator<Item = &'a str>) {
    undefs.extend(entries.into_iter().filter(|s| !is_pattern(s)).map(str::to_string));
}

fn add_patterns<'a>(glob: &mut GlobBuilder, opt: &str, pats: impl IntoIterator<Item = &'a str>) {
    for pat in pats {
        if !glob.add(pat.as_bytes(), 0) {
            fatal!("{opt}: invalid pattern: {pat}");
        }
    }
}

fn read_symbol_list(opt: &str, path: &Path) -> Vec<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => symbol_list(&text),
        Err(e) => fatal!(
            "{opt} file '{}' could not be opened, {}",
            path.display(),
            crate::error::errno_text(&e)
        ),
    }
}

fn symbol_list(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect()
}

/// ld64 numeric option arguments are hexadecimal, with or without a
/// 0x prefix.
fn parse_hex(opt: &str, val: &str) -> u64 {
    match u64::from_str_radix(val.trim_start_matches("0x"), 16) {
        Ok(num) => num,
        Err(_) => fatal!("malformed {opt}: {val}"),
    }
}

/// Parses a -segprot protection: the letters r, w and x in either
/// case, and '-' for none. ld-prime warns about any other byte and
/// ignores it, so a non-ASCII letter draws a warning for each of its
/// bytes (which the warnings spell lossily).
fn parse_prot(val: &[u8], warnings: &mut OptionWarnings) -> u8 {
    let mut prot = 0u8;
    for &c in val {
        match c.to_ascii_lowercase() {
            b'r' => prot |= 1,
            b'w' => prot |= 2,
            b'x' => prot |= 4,
            b'-' => {}
            _ => warnings.warn(format!("unknown -segprot letter '{}'", display(&[c]))),
        }
    }
    prot
}

/// A section or segment name an option gives, cut to the 16 bytes of
/// a Mach-O header's name field, as ld-prime silently does for the new
/// names of -rename_section and -rename_segment. (The names they
/// rename from are matched as given, so a longer one matches nothing.)
fn section_name(name: &str) -> String {
    name[..name.floor_char_boundary(16)].to_string()
}

/// A -sectcreate segment or section name, cut to 16 bytes with
/// ld-prime's warning. (-add_empty_section's are cut silently: ld-prime
/// fails an assertion on them.)
fn sectcreate_name(kind: &str, name: &str) -> String {
    let cut = section_name(name);
    if cut.len() < name.len() {
        crate::warn!("-sectcreate {kind} name too long ('{name}'), will be truncated to '{cut}'");
    }
    cut
}

fn is_space(c: u8) -> bool {
    // Same as isspace() in the C locale, without the function call that the
    // tokenizer below would otherwise make for every byte of a response file.
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

// Response files, given as "@path", are how build systems pass thousands
// of input files without exceeding the kernel's command line length
// limit. Option arguments such as "@rpath/libfoo.dylib" start with '@'
// too, so an argument is expanded only when the named file exists.
//
// This function opens a given file, tokenizes its contents, and returns a
// list of tokens.
fn read_response_file(mf: &'static MappedFile, depth: usize) -> Vec<Cow<'static, OsStr>> {
    let path = &mf.name;
    if depth > 10 {
        fatal!("{}: response file nesting too deep", path.display());
    }

    let data = mf.data();
    let mut expanded = Vec::new();
    let mut i = 0;

    while i < data.len() {
        if is_space(data[i]) {
            i += 1;
            continue;
        }

        // Plain tokens can borrow the mapping, which lives for the complete
        // link. Copy only when removing quotes or backslashes.
        let start = i;
        while i < data.len() && !is_space(data[i]) && !matches!(data[i], b'\\' | b'\'' | b'"') {
            i += 1;
        }
        let mut tok = Cow::Borrowed(&data[start..i]);
        let mut quote = None;
        while i < data.len() {
            let c = data[i];
            if c == b'\\' {
                if i + 1 == data.len() {
                    fatal!("{}: premature end of input", path.display());
                }
                tok.to_mut().push(data[i + 1]);
                i += 2;
            } else if let Some(q) = quote {
                if c == q {
                    quote = None;
                } else {
                    tok.to_mut().push(c);
                }
                i += 1;
            } else if c == b'\'' || c == b'"' {
                quote = Some(c);
                i += 1;
            } else if is_space(c) {
                break;
            } else {
                tok.to_mut().push(c);
                i += 1;
            }
        }
        if quote.is_some() {
            fatal!("{}: premature end of input", path.display());
        }
        if let Some(nested) = tok.strip_prefix(b"@")
            && let Some(nested) = MappedFile::open(os_str(nested))
        {
            expanded.extend(read_response_file(nested, depth + 1));
        } else {
            expanded.push(match tok {
                Cow::Borrowed(bytes) => Cow::Borrowed(os_str(bytes)),
                Cow::Owned(bytes) => Cow::Owned(OsString::from_vec(bytes)),
            });
        }
    }
    expanded
}

// Replace "@path/to/some/text/file" with its file contents.
pub fn expand_response_files(argv: Vec<OsString>) -> Vec<Cow<'static, OsStr>> {
    let mut args = Vec::new();
    for arg in argv {
        if let Some(path) = arg.as_bytes().strip_prefix(b"@")
            && let Some(mf) = MappedFile::open(os_str(path))
        {
            args.extend(read_response_file(mf, 1));
        } else {
            args.push(Cow::Owned(arg));
        }
    }
    args
}

/// Reads a -filelist file: one input path per line, in whatever bytes
/// the file system uses, optionally under a directory given after a
/// comma in the option's argument.
fn read_filelist(arg: &OsStr) -> Vec<PathBuf> {
    let (path, dir) = match memchr::memchr(b',', arg.as_bytes()) {
        Some(comma) => (
            Path::new(os_str(&arg.as_bytes()[..comma])),
            Some(Path::new(os_str(&arg.as_bytes()[comma + 1..]))),
        ),
        None => (Path::new(arg), None),
    };
    let text = std::fs::read(path).unwrap_or_else(|e| {
        let errno = crate::error::errno_text(&e);
        fatal!("-filelist file '{}' could not be opened, {errno}", path.display())
    });
    text.split(|&b| b == b'\n')
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
        .filter(|line| !line.is_empty())
        .map(|line| match dir {
            Some(dir) => dir.join(os_str(line)),
            None => PathBuf::from(os_str(line)),
        })
        .collect()
}

/// What option parsing needs to know about the target: the driver
/// parses once per speculated target, as mold does.
pub struct TargetTraits {
    pub name: &'static str,
}

/// The warnings ld-prime gives as it reads an option, which only a -w
/// before the option silences. They wait until the parse is known to
/// be for the target, so that they are given once.
#[derive(Default)]
struct OptionWarnings {
    quiet: bool,
    msgs: Vec<String>,
}

impl OptionWarnings {
    fn warn(&mut self, msg: impl Into<String>) {
        if !self.quiet {
            self.msgs.push(msg.into());
        }
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
    let mut args =
        Args { zero_ar_date: std::env::var_os("ZERO_AR_DATE").is_some(), ..Default::default() };
    let mut pie: Option<bool> = None;
    let mut function_starts: Option<bool> = None;
    let mut data_in_code_info: Option<bool> = None;
    let mut data_const: Option<bool> = None;
    let mut segprots: Vec<(String, u8, u8)> = Vec::new();
    let mut explicit_entry = false;
    let mut warnings = OptionWarnings::default();
    let mut i = 1;
    let mut version_shown = false;

    // Symbol name patterns are collected here and compiled into
    // matchers once the whole command line is known.
    let mut exported_symbols: Option<GlobBuilder> = None;
    let mut unexported_symbols = GlobBuilder::default();
    let mut reexported_symbols = GlobBuilder::default();
    let mut why_live = GlobBuilder::default();
    let mut local_strip_list = GlobBuilder::default();
    let mut local_keep_list: Option<GlobBuilder> = None;
    let mut export_choice: Option<ExportChoice> = None;
    let mut deprecated_undefined: Vec<&str> = Vec::new();

    crate::error::set_color(std::io::stderr().is_terminal());

    let next_arg = |i: &mut usize| -> &OsStr {
        *i += 1;
        match cmdline.get(*i) {
            Some(val) => val.as_ref(),
            None => fatal!("option {}: argument missing", display(cmdline[*i - 1].as_bytes())),
        }
    };
    // An operand of -rename_section or -rename_segment: ld-prime
    // reports a missing or empty one with the option's usage.
    let rename_operand = |i: &mut usize, opt: &str, usage: &str| -> &str {
        *i += 1;
        match cmdline.get(*i) {
            Some(arg) if !arg.is_empty() => text(opt, arg),
            _ => fatal!("{opt} missing {usage}"),
        }
    };
    // An argument that is text by nature.
    fn text<'a>(opt: &str, arg: &'a OsStr) -> &'a str {
        arg.to_str().unwrap_or_else(|| {
            fatal!("option {opt}: expected a UTF-8 argument: {}", display(arg.as_bytes()))
        })
    }
    let bytes = |arg: &OsStr| -> Vec<u8> { arg.as_bytes().to_vec() };
    let path = |arg: &OsStr| -> PathBuf { PathBuf::from(arg) };

    while i < cmdline.len() {
        let opt: &OsStr = cmdline[i].as_ref();
        // Every option name is ASCII; an unknown one is reported lossily.
        let name = opt.to_string_lossy();
        let name: &str = &name;
        match opt.as_bytes() {
            b"-o" => args.output = path(next_arg(&mut i)),
            b"-arch" => {
                let arch = text(name, next_arg(&mut i));
                args.arch = Some(
                    crate::target::canonical_name(arch)
                        .unwrap_or_else(|| fatal!("unsupported target: {arch}")),
                );
            }
            b"-e" => {
                args.entry = text(name, next_arg(&mut i)).to_string();
                explicit_entry = true;
            }
            b"-platform_version" => {
                args.platform = parse_platform(text(name, next_arg(&mut i)));
                args.platform_minos = parse_version(text(name, next_arg(&mut i)));
                args.platform_sdk = parse_version(text(name, next_arg(&mut i)));
            }
            b"-syslibroot" => args.syslibroot.push(path(next_arg(&mut i))),
            b"-L" => args.library_paths.push(path(next_arg(&mut i))),
            b"-l" => args.inputs.push(InputArg::Lib(next_arg(&mut i).to_owned(), false)),
            b"-framework" => {
                args.inputs.push(InputArg::Framework(next_arg(&mut i).to_owned(), false))
            }
            b"-weak_framework" => {
                args.inputs.push(InputArg::Framework(next_arg(&mut i).to_owned(), true))
            }
            b"-reexport_framework" => {
                args.inputs.push(InputArg::ReexportFramework(next_arg(&mut i).to_owned()))
            }
            b"-needed_framework" => {
                args.inputs.push(InputArg::NeededFramework(next_arg(&mut i).to_owned()))
            }
            b"-needed_library" => args.inputs.push(InputArg::NeededFile(path(next_arg(&mut i)))),
            b"-weak_library" => args.inputs.push(InputArg::WeakFile(path(next_arg(&mut i)))),
            b"-reexport_library" => {
                args.inputs.push(InputArg::ReexportFile(path(next_arg(&mut i))))
            }
            b"-sub_library" => args.inputs.push(InputArg::ReexportLib(next_arg(&mut i).to_owned())),
            b"-filelist" => {
                args.inputs.extend(read_filelist(next_arg(&mut i)).into_iter().map(InputArg::File));
            }
            b"-F" => args.framework_paths.push(path(next_arg(&mut i))),
            b"-dylib" => args.output_type = MH_DYLIB,
            b"-bundle" => args.output_type = MH_BUNDLE,
            b"-kext" => args.output_type = MH_KEXT_BUNDLE,
            b"-bundle_loader" => args.inputs.push(InputArg::BundleLoader(path(next_arg(&mut i)))),
            b"-final_output" => args.final_output = Some(bytes(next_arg(&mut i))),
            b"-keep_private_externs" => args.keep_private_externs = true,
            b"-rpath" => args.rpaths.push(bytes(next_arg(&mut i))),
            b"-install_name" | b"-dylib_install_name" => {
                args.install_name = Some(bytes(next_arg(&mut i)))
            }
            b"-map" => args.map = Some(path(next_arg(&mut i))),
            b"-sdk_imports" => args.sdk_imports = Some(path(next_arg(&mut i))),
            b"-fixup_chains" => args.fixup_chains = Some(true),
            b"-no_fixup_chains" => args.fixup_chains = Some(false),
            b"-adhoc_codesign" => args.adhoc_codesign = Some(true),
            b"-no_adhoc_codesign" => args.adhoc_codesign = Some(false),
            b"-dynamic" => args.dynamic = true,
            // The last of -static and -preload names the output type.
            b"-static" => {
                args.static_link = true;
                args.preload = false;
            }
            b"-preload" => {
                args.output_type = MH_EXECUTE;
                args.static_link = true;
                args.preload = true;
            }
            b"-kernel" => args.kernel = true,
            b"-version_load_command" => args.version_load_command = true,
            b"-pie" => pie = Some(true),
            b"-no_pie" => pie = Some(false),
            // Given with -dead_strip, this once kept initializers and
            // terminators nothing referenced. -dead_strip always keeps
            // them now, and ld64 takes this for -dead_strip alone.
            b"-no_dead_strip_inits_and_terms" => {
                args.dead_strip = true;
                warnings.warn(
                    "option '-no_dead_strip_inits_and_terms' is obsolete, use '-dead_strip' instead",
                );
            }
            b"-headerpad" => args.headerpad = parse_hex(name, text(name, next_arg(&mut i))),
            b"-pagezero_size" => {
                args.pagezero_size = parse_hex(name, text(name, next_arg(&mut i)));
                args.explicit_pagezero = true;
            }
            b"-image_base" | b"-seg1addr" => {
                args.image_base = Some(parse_hex(name, text(name, next_arg(&mut i))));
            }
            b"-segaddr" => {
                let seg = text(name, next_arg(&mut i)).to_string();
                let addr = parse_hex(name, text(name, next_arg(&mut i)));
                args.segaddrs.push((seg, addr));
            }
            b"-segprot" => {
                // ld-prime takes a missing argument for an empty one.
                let mut arg = || {
                    i += 1;
                    cmdline.get(i).map_or(&b""[..], |arg| arg.as_bytes())
                };
                let (seg, max, init) = (arg(), arg(), arg());
                if seg.is_empty() || max.is_empty() || init.is_empty() {
                    fatal!("-segprot missing <seg> <max-prot> <init-prot>");
                }
                let seg = text(name, OsStr::from_bytes(seg)).to_string();
                // __LINKEDIT, which dyld reads, keeps its own.
                if seg == "__LINKEDIT" {
                    warnings.warn("-segprot cannot be used to modify __LINKEDIT protections");
                } else {
                    let max = parse_prot(max, &mut warnings);
                    let init = parse_prot(init, &mut warnings);
                    segprots.push((seg, max, init));
                }
            }
            b"-segment_order" => {
                if !args.segment_order.is_empty() {
                    fatal!("-segment_order used more than once");
                }
                args.segment_order = text(name, next_arg(&mut i))
                    .split(':')
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect();
            }
            b"-rename_section" => {
                let usage = "<from-segment> <from-section> <to-segment> <to-section>";
                let old_seg = rename_operand(&mut i, name, usage).to_string();
                let old_sect = rename_operand(&mut i, name, usage).to_string();
                let new_seg = section_name(rename_operand(&mut i, name, usage));
                let new_sect = section_name(rename_operand(&mut i, name, usage));
                args.rename_sections.push((old_seg, old_sect, new_seg, new_sect));
            }
            b"-rename_segment" => {
                let usage = "<from-segment> <to-segment>";
                let old = rename_operand(&mut i, name, usage).to_string();
                let new = section_name(rename_operand(&mut i, name, usage));
                args.rename_segments.push((old, new));
            }
            b"-stack_size" => args.stack_size = parse_hex(name, text(name, next_arg(&mut i))),
            b"-sectcreate" => {
                let seg = sectcreate_name("segment", text(name, next_arg(&mut i)));
                let sect = sectcreate_name("section", text(name, next_arg(&mut i)));
                let file = path(next_arg(&mut i));
                args.sectcreate.push((seg, sect, file));
            }
            b"-add_empty_section" => {
                let seg = section_name(text(name, next_arg(&mut i)));
                let sect = section_name(text(name, next_arg(&mut i)));
                args.add_empty_section.push((seg, sect));
            }
            b"-x" => args.strip_locals = true,
            b"-Z" => args.no_standard_dirs = true,
            b"-r" => args.relocatable = true,
            b"-flat_namespace" => args.flat_namespace = true,
            b"-twolevel_namespace" => args.flat_namespace = false,
            // ld-prime knows one treatment besides the default error:
            // dynamic_lookup, which suppress selects too. It deprecates
            // every other one (error, warning or anything else) and
            // ignores it, so none undoes an earlier dynamic_lookup.
            b"-undefined" => {
                let treatment = text(name, next_arg(&mut i));
                if matches!(treatment, "dynamic_lookup" | "suppress") {
                    args.undefined_dynamic_lookup = true;
                }
                if treatment != "dynamic_lookup" {
                    deprecated_undefined.push(treatment);
                }
            }
            b"-U" => args.allowed_undefined.push(text(name, next_arg(&mut i)).to_string()),
            b"-w" => {
                args.suppress_warnings = true;
                warnings.quiet = true;
            }
            b"-fatal_warnings" => args.fatal_warnings = true,
            b"-demangle" => args.demangle = true,
            b"-help" => {
                println!("Usage: ld64.mold [options] file...");
                crate::error::exit_after_cleanup(0);
            }

            b"-dead_strip" => args.dead_strip = true,
            b"-dead_strip_dylibs" => args.dead_strip_dylibs = true,
            b"-warn_unused_dylibs" => args.warn_unused_dylibs = Some(true),
            b"-no_warn_unused_dylibs" => args.warn_unused_dylibs = Some(false),
            b"-not_for_dyld_shared_cache" => args.not_for_dyld_shared_cache = true,
            b"-debug_variant" => args.debug_variant = true,
            b"-no_inits" => args.no_inits = true,
            b"-no_warn_inits" => args.no_warn_inits = true,
            b"-bind_at_load" => args.bind_at_load = true,
            b"-application_extension" => args.application_extension = true,
            b"-no_application_extension" => args.application_extension = false,
            b"-add_ast_path" => args.add_ast_paths.push(path(next_arg(&mut i))),
            b"-S" => args.strip_debug = true,
            b"-all_load" => args.all_load = true,
            b"-u" => args.forced_undefined.push(text(name, next_arg(&mut i)).to_string()),
            b"-exported_symbol" => {
                check_export_choice(&mut export_choice, ExportChoice::Exported, name);
                let pat = text(name, next_arg(&mut i));
                add_initial_undefines(&mut args.forced_undefined, [pat]);
                add_patterns(exported_symbols.get_or_insert_default(), name, [pat]);
            }
            b"-no_exported_symbols" => {
                check_export_choice(&mut export_choice, ExportChoice::None, name);
                args.no_exported_symbols = true;
            }
            b"-exported_symbols_list" => {
                check_export_choice(&mut export_choice, ExportChoice::Exported, name);
                let names = read_symbol_list(name, &path(next_arg(&mut i)));
                add_initial_undefines(&mut args.forced_undefined, names.iter().map(String::as_str));
                add_patterns(
                    exported_symbols.get_or_insert_default(),
                    name,
                    names.iter().map(String::as_str),
                );
            }
            b"-unexported_symbol" => {
                check_export_choice(&mut export_choice, ExportChoice::Unexported, name);
                add_patterns(&mut unexported_symbols, name, [text(name, next_arg(&mut i))])
            }
            b"-unexported_symbols_list" => {
                check_export_choice(&mut export_choice, ExportChoice::Unexported, name);
                let names = read_symbol_list(name, &path(next_arg(&mut i)));
                add_patterns(&mut unexported_symbols, name, names.iter().map(String::as_str));
            }
            b"-reexported_symbols_list" => {
                let names = read_symbol_list(name, &path(next_arg(&mut i)));
                // Exact names force a reference even if no object
                // mentions them. Patterns only match existing symbols.
                add_initial_undefines(&mut args.forced_undefined, names.iter().map(String::as_str));
                for sym in &names {
                    if !is_pattern(sym) {
                        args.reexported_names.push(sym.clone());
                    }
                }
                add_patterns(&mut reexported_symbols, name, names.iter().map(String::as_str));
            }
            // The -dylib_ spellings are the older names ld64 still
            // accepts; Xcode passes -dylib_compatibility_version.
            b"-current_version" | b"-dylib_current_version" => {
                args.current_version = parse_version(text(name, next_arg(&mut i)))
            }
            b"-compatibility_version" | b"-dylib_compatibility_version" => {
                args.compatibility_version = parse_version(text(name, next_arg(&mut i)))
            }
            // ld64 prints its version banner to stdout and continues
            // with the link.
            b"-v" => {
                println!("mold-macho {} (compatible with Apple ld64)", env!("CARGO_PKG_VERSION"));
                version_shown = true;
            }
            // Xcode's build system runs `ld -version_details` before the
            // first link and refuses to build if the output is not JSON.
            // It decodes two keys: "version", an ld64 version it compares
            // against thresholds to decide which flags to pass (e.g.
            // -sdk_imports needs 1164), and "architectures". Apple's ld
            // also reports its LTO and TAPI versions; they are ignored.
            // We claim the ld64 version whose command line we implement
            // so that Xcode drives us exactly as it drives ld-prime.
            b"-version_details" => {
                println!(
                    "{{\n\t\"version\": \"{LD64_COMPAT_VERSION}\",\n\t\"architectures\": [\n\t\t\"arm64\",\n\t\t\"x86_64\"\n\t]\n}}"
                );
                std::process::exit(0);
            }
            b"-noall_load" => args.all_load = false,
            b"-ObjC" => args.load_objc = true,
            b"-force_load" => args.inputs.push(InputArg::ForceLoad(path(next_arg(&mut i)))),

            // The default library search behavior already matches
            // -search_paths_first: each path is tried for both a dylib
            // and an archive before moving to the next.
            b"-search_paths_first" => args.search_dylibs_first = false,
            b"-search_dylibs_first" => args.search_dylibs_first = true,
            b"-umbrella" => args.umbrella = Some(bytes(next_arg(&mut i))),
            b"-oso_prefix" => args.oso_prefix = Some(bytes(next_arg(&mut i))),
            b"-mark_dead_strippable_dylib" => args.mark_dead_strippable_dylib = true,
            b"-export_dynamic" => args.export_dynamic = true,
            b"-order_file" => args.order_files.push(path(next_arg(&mut i))),
            b"--print-dependencies" => args.print_dependencies = true,
            b"-why_load" | b"-whyload" => args.why_load = true,
            b"-why_live" => add_patterns(&mut why_live, name, [text(name, next_arg(&mut i))]),
            b"-allowable_client" => args.allowable_clients.push(bytes(next_arg(&mut i))),
            b"-client_name" => args.client_name = Some(bytes(next_arg(&mut i))),
            b"-t" => args.trace = true,
            b"-ignore_optimization_hints" => args.ignore_optimization_hints = true,
            b"-print_statistics" => args.perf = true,
            b"-warn_duplicate_libraries" => args.warn_duplicate_libraries = true,
            b"-no_warn_duplicate_libraries" => args.warn_duplicate_libraries = false,
            b"-non_global_symbols_strip_list" => {
                let names = read_symbol_list(name, &path(next_arg(&mut i)));
                add_patterns(&mut local_strip_list, name, names.iter().map(String::as_str));
            }
            b"-non_global_symbols_no_strip_list" => {
                let names = read_symbol_list(name, &path(next_arg(&mut i)));
                add_patterns(
                    local_keep_list.get_or_insert_default(),
                    name,
                    names.iter().map(String::as_str),
                );
            }
            b"-sectalign" => {
                let seg = text(name, next_arg(&mut i)).to_string();
                let sect = text(name, next_arg(&mut i)).to_string();
                let val = text(name, next_arg(&mut i));
                let align = parse_hex("-sectalign", val);
                if !align.is_power_of_two() {
                    fatal!("-sectalign: alignment not a power of two: {val}");
                }
                args.sectalign.push((seg, sect, align.trailing_zeros() as u8));
            }
            b"-alias" => {
                let existing = text(name, next_arg(&mut i)).to_string();
                let new = text(name, next_arg(&mut i)).to_string();
                args.aliases.push((existing, new));
            }
            b"-alias_list" => {
                let list = path(next_arg(&mut i));
                // ld64 links on without the aliases, warning in the
                // words it uses for an order file.
                let contents = match std::fs::read_to_string(&list) {
                    Ok(contents) => contents,
                    Err(e) => {
                        let errno = crate::error::errno_text(&e);
                        warnings.warn(format!(
                            "order file '{}' could not be opened, {errno}",
                            list.display()
                        ));
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
                        (Some(existing), Some(new)) => {
                            args.aliases.push((existing.to_string(), new.to_string()))
                        }
                        _ => fatal!("malformed -alias_list line: {line}"),
                    }
                }
            }
            b"-executable_path" => args.executable_path = Some(path(next_arg(&mut i))),

            // Reserve enough header padding that install_name_tool can
            // grow install names in place.
            b"-headerpad_max_install_names" => args.headerpad_max_install_names = true,

            b"-deduplicate" => args.deduplicate = true,
            b"-text_exec" => args.text_exec = true,
            b"-no_deduplicate" => args.deduplicate = false,
            b"-function_starts" => function_starts = Some(true),
            b"-init_offsets" => args.init_offsets = true,
            b"-data_const" => data_const = Some(true),
            b"-no_data_const" => data_const = Some(false),
            b"-no_implicit_dylibs" => args.no_implicit_dylibs = true,
            b"-objc_relative_method_lists" => args.objc_relative_method_lists = Some(true),
            b"-no_objc_relative_method_lists" => args.objc_relative_method_lists = Some(false),
            b"-no_objc_category_merging" => args.objc_category_merging = false,
            b"-no_function_starts" => function_starts = Some(false),
            b"-data_in_code_info" => data_in_code_info = Some(true),
            b"-add_split_seg_info" => args.add_split_seg_info = true,
            b"-no_data_in_code_info" => data_in_code_info = Some(false),

            b"-no_uuid" => args.uuid = false,

            // The old pre-LC_BUILD_VERSION way of stating the
            // deployment target, still emitted by clang for older
            // -mmacosx-version-min targets. It fixes the platform to
            // macOS; ld64 records the SDK as the same version (the
            // flag carries no separate SDK).
            b"-macos_version_min" | b"-macosx_version_min" => {
                args.platform = PLATFORM_MACOS;
                args.platform_minos = parse_version(text(name, next_arg(&mut i)));
                args.platform_sdk = args.platform_minos;
            }

            // This linker's output is always deterministic, so
            // -reproducible has nothing to switch on.
            b"-reproducible" => {}

            b"-lto_library" => args.lto_library = Some(path(next_arg(&mut i))),

            b"-dependency_info" => args.dependency_info = Some(path(next_arg(&mut i))),

            // Ignored options with an argument
            b"-object_path_lto" => args.object_path_lto = Some(path(next_arg(&mut i))),

            // Ignored options with an argument
            b"-mllvm" => {
                next_arg(&mut i);
            }

            raw => {
                let os_name = |rest: &[u8]| os_str(rest).to_owned();
                if let Some(lib) = raw.strip_prefix(b"-reexport-l") {
                    args.inputs.push(InputArg::ReexportLib(os_name(lib)));
                } else if let Some(lib) = raw.strip_prefix(b"-hidden-l") {
                    args.inputs.push(InputArg::HiddenLib(os_name(lib)));
                } else if let Some(lib) = raw.strip_prefix(b"-needed-l") {
                    args.inputs.push(InputArg::NeededLib(os_name(lib)));
                } else if let Some(lib) = raw.strip_prefix(b"-weak-l") {
                    args.inputs.push(InputArg::Lib(os_name(lib), true));
                } else if let Some(lib) = raw.strip_prefix(b"-l") {
                    args.inputs.push(InputArg::Lib(os_name(lib), false));
                } else if let Some(dir) = raw.strip_prefix(b"-L") {
                    args.library_paths.push(PathBuf::from(os_str(dir)));
                } else if let Some(dir) = raw.strip_prefix(b"-F") {
                    args.framework_paths.push(PathBuf::from(os_str(dir)));
                } else if raw.starts_with(b"-O") {
                    // An optimization level, which clang passes on from
                    // its own command line (-O2, -Ofast, -Og, ...).
                    // ld-prime takes -O followed by anything; it only
                    // switches function deduplication, which here is
                    // on unless -no_deduplicate, whatever the level.
                } else if raw.starts_with(b"-") {
                    fatal!("unknown command line option: {name}");
                } else {
                    args.inputs.push(InputArg::File(PathBuf::from(opt)));
                }
            }
        }
        i += 1;
    }

    // `ld -v` with nothing to link just reports the version; build
    // systems and configure scripts probe the linker that way. mold
    // does the same for -v/--version with no inputs.
    if version_shown && args.inputs.is_empty() {
        std::process::exit(0);
    }

    check_segment_order(&args);

    // A -static image (a kernel) carries the code tables only when
    // asked to, as ld-prime writes it.
    args.function_starts = function_starts.unwrap_or(!args.without_dyld());
    args.data_in_code_info = data_in_code_info.unwrap_or(!args.without_dyld());

    // A -preload image has no __LINKEDIT segment: ld-prime keeps nothing
    // outside its segments but the symbol table (and the local
    // relocations of a -pie one). The options asking for the code
    // tables, a build version or a signature go unheeded, as does
    // -rpath, which only dyld would read.
    if args.preload {
        args.function_starts = false;
        args.data_in_code_info = false;
        args.version_load_command = false;
        args.adhoc_codesign = Some(false);
        args.rpaths.clear();
    }

    // An image no dyld loads starts from LC_UNIXTHREAD at "start",
    // crt1.o's entry point, as every executable did before LC_MAIN had
    // dyld call _main.
    if args.static_link && !explicit_entry {
        args.entry = "start".to_string();
    }

    if args.relocatable && args.sdk_imports.is_some() {
        fatal!("-sdk_imports cannot be used with -r");
    }
    // What is dead is known only once the final link sees every
    // reference.
    if args.relocatable && args.dead_strip {
        fatal!("-r and -dead_strip cannot be used together");
    }
    args.exported_symbols = exported_symbols.map(GlobBuilder::build);
    args.unexported_symbols = unexported_symbols.build();
    args.reexported_symbols = reexported_symbols.build();
    args.why_live = why_live.build();
    args.local_strip_list = local_strip_list.build();
    args.local_keep_list = local_keep_list.map(GlobBuilder::build);

    // Without -arch, the first Mach-O input names the target. A parse
    // for another target than this one is redone by the driver, so
    // what depends on the target is left to that parse.
    if args.arch.is_none() {
        args.arch = Some(detect_target(&args.inputs));
    }
    if args.arch != Some(target.name) {
        return args;
    }

    // -fatal_warnings applies to every warning, wherever it appears on
    // the command line. So does -w to those from the option checks
    // below, but not to those given as options were read.
    crate::error::set_fatal_warnings(args.fatal_warnings);
    for msg in &warnings.msgs {
        crate::warn!("{msg}");
    }
    crate::error::set_suppress_warnings(args.suppress_warnings);

    for treatment in deprecated_undefined {
        crate::warn!("-undefined {treatment} is deprecated");
    }
    if args.kernel && !args.static_link {
        fatal!("-kernel must be used with -static");
    }
    check_output_kind(&args, pie, data_const, explicit_entry);

    // A dylib is loaded at an arbitrary address, and a -preload image
    // copied to wherever its segments say; only a main executable
    // reserves the low 4 GiB against NULL dereferences.
    if args.output_type != MH_EXECUTE || args.preload {
        args.pagezero_size = 0;
    }

    // kmutil links a kext by its relocations and slides a -kernel
    // image by its local ones: ld-prime takes neither -fixup_chains nor
    // -no_fixup_chains for them.
    if args.is_kext() || args.kernel {
        args.fixup_chains = None;
    }
    args.pie = resolve_pie(target, &args, pie);
    args.segaddrs = resolve_segaddrs(std::mem::take(&mut args.segaddrs));
    args.segprots = resolve_segprots(target, segprots);
    resolve_shared_region(target, &mut args);
    // An image dyld doesn't load has no __DATA_CONST unless bound for
    // the shared region: nothing else makes that segment read-only
    // after fixups. Nor does ld-prime give one to a non-PIE executable,
    // which keeps its classic layout.
    args.data_const = data_const.unwrap_or(if args.without_dyld() {
        args.shared_region
    } else {
        args.pie || args.output_type != MH_EXECUTE
    });
    resolve_kext(target, &mut args);
    if args.undefined_dynamic_lookup && !args.allowed_undefined.is_empty() {
        crate::warn!("-U option is redundant when using -undefined dynamic_lookup");
    }

    args
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
/// -add_split_seg_info or -kernel, an arm64 kext, or a dylib installed
/// where the cache takes libraries from, unless
/// -not_for_dyld_shared_cache, or -debug_variant for a dylib. Such an
/// image records its references between sections
/// (LC_SEGMENT_SPLIT_INFO), so ld64 leaves its code as compiled (no
/// optimization hints); it may not look symbols up dynamically, since
/// the cache builder binds every one to the dylib that exports it; and
/// ld-prime warns about run paths, which an OS library must not need.
fn resolve_shared_region(target: &TargetTraits, args: &mut Args) {
    let is_dylib = args.output_type == MH_DYLIB;
    args.shared_region = !args.not_for_dyld_shared_cache
        && !(is_dylib && args.debug_variant)
        && (args.add_split_seg_info
            || args.kernel
            || (args.is_kext() && target.name == "arm64")
            || (is_dylib && in_shared_cache_path(args.output_install_name())));
    if !args.shared_region {
        return;
    }
    args.ignore_optimization_hints = true;
    if args.flat_namespace {
        fatal!(
            "Shared cache eligible dylibs cannot use '-flat_namespace'.  Remove '-flat_namespace' \
             or opt out of the shared cache using the build setting 'LD_SHARED_CACHE_ELIGIBLE=NO' \
             (or linker flag '-not_for_dyld_shared_cache')"
        );
    }
    // (A kext looks up every import.)
    if (args.undefined_dynamic_lookup || !args.allowed_undefined.is_empty()) && !args.is_kext() {
        fatal!(
            "Shared cache eligible dylibs cannot use '-undefined dynamic_lookup' or '-U' to find \
             symbols. Remove these options or opt out of the shared cache using the build \
             setting 'LD_SHARED_CACHE_ELIGIBLE=NO' (or linker flag '-not_for_dyld_shared_cache')"
        );
    }
    if !args.rpaths.is_empty() {
        crate::warn!(
            "OS dylibs should not add rpaths (linker option: -rpath) (Xcode build setting: \
             LD_RUNPATH_SEARCH_PATHS)"
        );
    }
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

    /// Whether no dyld loads the image: a -static one, which loads (and
    /// slides) itself, or a kext, which kmutil links into the kernel
    /// by its relocations.
    pub fn without_dyld(&self) -> bool {
        self.static_link || self.is_kext()
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
/// does. Only a main executable has an entry point, a main-thread
/// stack, a __PAGEZERO and the MH_PIE flag (a -preload one, copied to
/// wherever its segments say, has neither stack nor __PAGEZERO), and a
/// client name is what a bundle or an executable presents to the
/// umbrella it links against. A relocatable object also leaves the
/// __DATA_CONST split to the link that consumes it.
fn check_output_kind(args: &Args, pie: Option<bool>, data_const: Option<bool>, entry: bool) {
    let main_executable = args.output_type == MH_EXECUTE && !args.relocatable;
    let has_stack = main_executable && !args.preload;
    if args.client_name.is_some() && (args.relocatable || args.output_type == MH_DYLIB) {
        fatal!("-client_name can only be used when creating a bundle or main executable");
    }
    if pie == Some(true) && !main_executable {
        if args.relocatable {
            fatal!("-pie can only be used when linking a main executable");
        }
        crate::warn!("-pie being ignored. It is only used when linking a main executable");
    }
    if args.relocatable && data_const == Some(true) {
        fatal!("-data_const not supported with -r");
    }
    if !has_stack && args.explicit_pagezero && args.pagezero_size != 0 {
        fatal!("-pagezero_size can only be used when linking a main executable");
    }
    if !has_stack && args.stack_size != 0 {
        fatal!("-stack_size option can only be used when linking a main executable");
    }
    if !main_executable && entry {
        crate::warn!("ignoring -e, not used for output type");
    }
}

/// Whether an executable is position independent (MH_PIE). It is
/// unless -no_pie says otherwise, which arm64 ignores (arm64 macOS runs
/// PIE executables only) and which ld-prime deprecates from the OS
/// versions that default to chained fixups. A -static image (a kernel)
/// is PIE only with -pie or -kernel, or with -fixup_chains, whose
/// chains exist to slide it.
fn resolve_pie(target: &TargetTraits, args: &Args, pie: Option<bool>) -> bool {
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
        None => !args.static_link || args.kernel || args.fixup_chains == Some(true),
    }
}

/// -segment_order lays out an image no dyld loads, a -static or a
/// -preload one (ld-prime also allows firmware platforms, which mold
/// has not).
fn check_segment_order(args: &Args) {
    if args.segment_order.is_empty() {
        return;
    }
    if args.segment_order.len() < 2 {
        fatal!("-segment_order should specifify at least two segments");
    }
    if !args.static_link {
        fatal!(
            "-segment_order can only be used with -preload, -static, or with -platform_version \"firmware\"/\"sepOS\""
        );
    }
}

/// -segaddr's (segment, address) pins, one per segment: ld-prime takes
/// the last address given for a segment, with a warning.
fn resolve_segaddrs(segaddrs: Vec<(String, u64)>) -> Vec<(String, u64)> {
    let mut out: Vec<(String, u64)> = Vec::new();
    for (name, addr) in segaddrs {
        match out.iter_mut().find(|(seen, _)| *seen == name) {
            Some((_, old)) if *old == addr => crate::warn!("-segaddr {name} used more than once"),
            Some((_, old)) => {
                crate::warn!("-segaddr {name} has conflicting values, using 0x{addr:X}");
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
    segprots: Vec<(String, u8, u8)>,
) -> Vec<(String, u8, u8)> {
    let mut out: Vec<(String, u8, u8)> = Vec::new();
    for (name, max, init) in segprots {
        if out.iter().all(|(seen, _, _)| *seen != name) {
            out.push((name, if target.name == "arm64" { init } else { max }, init));
        }
    }
    out
}

/// The target of the first Mach-O input file named on the command line;
/// the host's if there is none.
fn detect_target(inputs: &[InputArg]) -> &'static str {
    for input in inputs {
        if let InputArg::File(path) = input
            && let Some(mf) = MappedFile::open(path)
            && let Some(name) = crate::filetype::get_macho_target(mf.data())
        {
            return name;
        }
    }
    if cfg!(target_arch = "aarch64") { "arm64" } else { "x86_64" }
}
