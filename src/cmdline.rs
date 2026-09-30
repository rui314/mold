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
    /// -bundle_loader: the executable a bundle's undefined symbols may
    /// resolve to, bound at run time as the main executable.
    pub bundle_loader: Option<PathBuf>,
    /// -arch, canonicalized to the target's own spelling of its name.
    pub arch: Option<&'static str>,
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
    /// Symbols to treat as undefined from the start (-u), forcing
    /// archive members that define them to be linked.
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
    /// it off).
    pub deduplicate: bool,
    /// Emit LC_FUNCTION_STARTS (on by default).
    pub function_starts: bool,
    /// Emit LC_DATA_IN_CODE (on by default).
    pub data_in_code_info: bool,
    /// -init_offsets: emit initializers as 32-bit image offsets
    /// (__init_offsets) instead of absolute pointers (__mod_init_func).
    pub init_offsets: bool,
    /// -data_const / -no_data_const: whether read-only-after-fixup data
    /// sections (__const, __cfstring, the ObjC lists, __got ...) go in
    /// a __DATA_CONST segment. ld64's default is on.
    pub data_const: bool,
    /// -no_implicit_dylibs: do not bind through a re-export to the
    /// defining dylib (and add a load command for it); bind to the
    /// re-exporting dylib named on the command line instead.
    pub no_implicit_dylibs: bool,
    /// -objc_relative_method_lists / -no_objc_relative_method_lists:
    /// rewrite Objective-C method lists in the relative form. ld64's
    /// default is on from macOS 11.
    pub objc_relative_method_lists: Option<bool>,
    /// -objc_category_merging / -no_objc_category_merging: merge
    /// categories into the classes defined in the same image. ld64's
    /// default is on.
    pub objc_category_merging: Option<bool>,
    /// Compute a content-hash LC_UUID (on by default; -no_uuid leaves
    /// it zeroed - dyld refuses executables without the load command).
    pub uuid: bool,
    /// -w: suppress warnings.
    pub suppress_warnings: bool,
    pub fatal_warnings: bool,
    pub demangle: bool,
    /// -undefined dynamic_lookup: leave unresolved symbols to be looked
    /// up in any loaded image at run time.
    pub undefined_dynamic_lookup: bool,
    /// -undefined warning/suppress: report unresolved symbols without
    /// failing (they resolve like dynamic_lookup).
    pub undefined_warning: bool,
    /// -undefined warning specifically (as opposed to suppress or
    /// dynamic_lookup): the one treatment under which ld64 still
    /// defaults to chained fixups.
    pub undefined_is_warning: bool,
    /// -U: individual symbols allowed to stay undefined.
    pub allowed_undefined: Vec<String>,
    /// -dead_strip_dylibs: drop load commands for dylibs nothing binds
    /// to.
    pub dead_strip_dylibs: bool,
    /// -bind_at_load: ask dyld to resolve all bindings at load time.
    pub bind_at_load: bool,
    /// -application_extension: mark the image safe for app extensions.
    pub application_extension: bool,
    /// -add_ast_path: Swift AST paths recorded as N_AST stabs for the
    /// debugger.
    pub add_ast_paths: Vec<PathBuf>,
    pub dynamic: bool,
    pub headerpad: u64,
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
    /// -non_global_symbols_keep_list: if set, only matching local
    /// symbols stay.
    pub local_keep_list: Option<Glob>,
    pub pagezero_size: u64,
    /// True when -pagezero_size was given explicitly (it is an error
    /// anywhere but a main executable).
    pub explicit_pagezero: bool,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            output: PathBuf::from("a.out"),
            output_type: MH_EXECUTE,
            install_name: None,
            final_output: None,
            keep_private_externs: false,
            bundle_loader: None,
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
            init_offsets: false,
            data_const: true,
            no_implicit_dylibs: false,
            objc_relative_method_lists: None,
            objc_category_merging: None,
            uuid: true,
            suppress_warnings: false,
            fatal_warnings: false,
            demangle: false,
            undefined_dynamic_lookup: false,
            undefined_warning: false,
            undefined_is_warning: false,
            allowed_undefined: Vec::new(),
            dead_strip_dylibs: false,
            bind_at_load: false,
            application_extension: false,
            add_ast_paths: Vec::new(),
            dynamic: true,
            headerpad: 0x100,
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
        }
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
fn add_patterns<'a>(glob: &mut GlobBuilder, opt: &str, pats: impl IntoIterator<Item = &'a str>) {
    for pat in pats {
        if !glob.add(pat.as_bytes(), 0) {
            fatal!("{opt}: invalid pattern: {pat}");
        }
    }
}

fn read_symbol_list(path: &Path) -> Vec<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => symbol_list(&text),
        Err(_) => fatal!("cannot read symbol list: {}", path.display()),
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
    let Ok(text) = std::fs::read(path) else {
        fatal!("cannot read -filelist file: {}", path.display());
    };
    text.split(|&b| b == b'\n')
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
        .filter(|line| !line.is_empty())
        .map(|line| match dir {
            Some(dir) => dir.join(os_str(line)),
            None => PathBuf::from(os_str(line)),
        })
        .collect()
}

/// Parses all options. `cmdline` includes the program name.
///
/// Options are matched as bytes and their arguments keep the bytes they
/// were given in: paths, install names and rpaths pass through to the
/// file system and the load commands unchanged. Arguments that are text
/// by nature (symbol and section names, versions, the -undefined
/// treatment) must be UTF-8.
pub fn parse_args(cmdline: &[Cow<'_, OsStr>]) -> Args {
    let mut args = Args::default();
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

    crate::error::set_color(std::io::stderr().is_terminal());

    let next_arg = |i: &mut usize| -> &OsStr {
        *i += 1;
        match cmdline.get(*i) {
            Some(val) => val.as_ref(),
            None => fatal!("option {}: argument missing", display(cmdline[*i - 1].as_bytes())),
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
            b"-e" => args.entry = text(name, next_arg(&mut i)).to_string(),
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
            b"-bundle_loader" => args.bundle_loader = Some(path(next_arg(&mut i))),
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
            b"-headerpad" => args.headerpad = parse_hex(name, text(name, next_arg(&mut i))),
            b"-pagezero_size" => {
                args.pagezero_size = parse_hex(name, text(name, next_arg(&mut i)));
                args.explicit_pagezero = true;
            }
            b"-stack_size" => args.stack_size = parse_hex(name, text(name, next_arg(&mut i))),
            b"-sectcreate" => {
                let seg = text(name, next_arg(&mut i)).to_string();
                let sect = text(name, next_arg(&mut i)).to_string();
                let file = path(next_arg(&mut i));
                args.sectcreate.push((seg, sect, file));
            }
            b"-add_empty_section" => {
                let seg = text(name, next_arg(&mut i)).to_string();
                let sect = text(name, next_arg(&mut i)).to_string();
                args.add_empty_section.push((seg, sect));
            }
            b"-x" => args.strip_locals = true,
            b"-Z" => args.no_standard_dirs = true,
            b"-r" => args.relocatable = true,
            b"-flat_namespace" => args.flat_namespace = true,
            b"-twolevel_namespace" => args.flat_namespace = false,
            b"-undefined" => match text(name, next_arg(&mut i)) {
                "error" => args.undefined_dynamic_lookup = false,
                "dynamic_lookup" => args.undefined_dynamic_lookup = true,
                t @ ("warning" | "suppress") => {
                    args.undefined_dynamic_lookup = true;
                    args.undefined_warning = true;
                    args.undefined_is_warning = t == "warning";
                }
                treatment => fatal!("-undefined: unsupported treatment: {treatment}"),
            },
            b"-U" => args.allowed_undefined.push(text(name, next_arg(&mut i)).to_string()),
            b"-w" => args.suppress_warnings = true,
            b"-fatal_warnings" => args.fatal_warnings = true,
            b"-demangle" => args.demangle = true,
            b"-help" => {
                println!("Usage: ld64.mold [options] file...");
                crate::error::exit_after_cleanup(0);
            }

            b"-dead_strip" => args.dead_strip = true,
            b"-dead_strip_dylibs" => args.dead_strip_dylibs = true,
            b"-bind_at_load" => args.bind_at_load = true,
            b"-application_extension" => args.application_extension = true,
            b"-no_application_extension" => args.application_extension = false,
            b"-add_ast_path" => args.add_ast_paths.push(path(next_arg(&mut i))),
            b"-S" => args.strip_debug = true,
            b"-all_load" => args.all_load = true,
            b"-u" => args.forced_undefined.push(text(name, next_arg(&mut i)).to_string()),
            b"-exported_symbol" => {
                let pat = text(name, next_arg(&mut i));
                add_patterns(exported_symbols.get_or_insert_default(), name, [pat]);
            }
            b"-no_exported_symbols" => args.no_exported_symbols = true,
            b"-exported_symbols_list" => {
                let names = read_symbol_list(&path(next_arg(&mut i)));
                add_patterns(
                    exported_symbols.get_or_insert_default(),
                    name,
                    names.iter().map(String::as_str),
                );
            }
            b"-unexported_symbol" => {
                add_patterns(&mut unexported_symbols, name, [text(name, next_arg(&mut i))])
            }
            b"-unexported_symbols_list" => {
                let names = read_symbol_list(&path(next_arg(&mut i)));
                add_patterns(&mut unexported_symbols, name, names.iter().map(String::as_str));
            }
            b"-reexported_symbols_list" => {
                let names = read_symbol_list(&path(next_arg(&mut i)));
                // Exact names force a reference even if no object
                // mentions them. Patterns only match existing symbols.
                for sym in &names {
                    if !sym.contains(['*', '?', '[']) {
                        args.forced_undefined.push(sym.clone());
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
                let names = read_symbol_list(&path(next_arg(&mut i)));
                add_patterns(&mut local_strip_list, name, names.iter().map(String::as_str));
            }
            b"-non_global_symbols_keep_list" => {
                let names = read_symbol_list(&path(next_arg(&mut i)));
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
                let Ok(contents) = std::fs::read_to_string(&list) else {
                    fatal!("cannot read -alias_list: {}", list.display());
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
            b"-headerpad_max_install_names" => {
                args.headerpad = args.headerpad.max(1024);
            }

            b"-no_deduplicate" => args.deduplicate = false,
            b"-function_starts" => args.function_starts = true,
            b"-init_offsets" => args.init_offsets = true,
            b"-data_const" => args.data_const = true,
            b"-no_data_const" => args.data_const = false,
            b"-no_implicit_dylibs" => args.no_implicit_dylibs = true,
            b"-objc_relative_method_lists" => args.objc_relative_method_lists = Some(true),
            b"-no_objc_relative_method_lists" => args.objc_relative_method_lists = Some(false),
            b"-objc_category_merging" => args.objc_category_merging = Some(true),
            b"-no_objc_category_merging" => args.objc_category_merging = Some(false),
            b"-no_function_starts" => args.function_starts = false,
            b"-data_in_code_info" => args.data_in_code_info = true,
            b"-no_data_in_code_info" => args.data_in_code_info = false,

            b"-no_uuid" => args.uuid = false,

            // The old pre-LC_BUILD_VERSION way of stating the
            // deployment target, still emitted by clang for older
            // -mmacosx-version-min targets. It fixes the platform to
            // macOS; the SDK version stays unset, as ld64 records when
            // it isn't told.
            b"-macos_version_min" | b"-macosx_version_min" => {
                args.platform = PLATFORM_MACOS;
                args.platform_minos = parse_version(text(name, next_arg(&mut i)));
            }

            // Ignored options. ld64 takes -O<n> as a linker
            // optimization level hint (Xcode passes -O0 for debug and
            // -Os for release builds). This linker's output is always
            // deterministic, so -reproducible has nothing to switch on.
            // -debug_variant silences ld64's warnings that only matter
            // for binaries shipped to customers; there are none here.
            b"-reproducible" | b"-debug_variant" | b"-O0" | b"-O1" | b"-O2" | b"-O3" | b"-Os"
            | b"-Oz" => {}

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

    // A dylib is loaded at an arbitrary address; only a main executable
    // reserves the low 4 GiB against NULL dereferences.
    if args.output_type != MH_EXECUTE {
        if args.explicit_pagezero {
            fatal!("-pagezero_size option can only be used when linking a main executable");
        }
        args.pagezero_size = 0;
    }

    if args.relocatable && args.sdk_imports.is_some() {
        fatal!("-sdk_imports cannot be used with -r");
    }
    args.exported_symbols = exported_symbols.map(GlobBuilder::build);
    args.unexported_symbols = unexported_symbols.build();
    args.reexported_symbols = reexported_symbols.build();
    args.why_live = why_live.build();
    args.local_strip_list = local_strip_list.build();
    args.local_keep_list = local_keep_list.map(GlobBuilder::build);
    if args.no_exported_symbols
        && (args.exported_symbols.is_some() || !args.unexported_symbols.is_empty())
    {
        fatal!("-no_exported_symbols cannot be used with -exported_symbol* or -unexported_symbol*");
    }
    args
}
