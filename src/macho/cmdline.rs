//! Command line parsing.
//!
//! The command line is compatible with Apple's ld64: options are single-dash
//! long names, and input files and `-l` options are position-dependent.

use crate::fatal;
use crate::macho::format::*;

/// The Apple ld64 version whose command line this linker implements,
/// reported by -version_details. Xcode passes flags according to this
/// number (Xcode 26.6 ships ld-1267).
pub const LD64_COMPAT_VERSION: &str = "1267";

/// An input in command line order.
#[derive(Clone, Debug)]
pub enum InputArg {
    /// A file path.
    File(String),
    /// `-lfoo`: a library to search for in the library paths. The flag
    /// marks a weak library (`-weak-lfoo`).
    Lib(String, bool),
    /// `-framework Foo`: a framework to search for in the framework
    /// paths. The flag marks a weak framework.
    Framework(String, bool),
    /// `-force_load path`: an archive all of whose members are linked.
    ForceLoad(String),
    /// `-weak_library path`: a dylib whose absence is tolerated at load
    /// time.
    WeakFile(String),
    /// `-reexport-lfoo` / `-reexport_library path`: a dylib whose
    /// exports this dylib re-exports as its own.
    ReexportLib(String),
    ReexportFile(String),
    ReexportFramework(String),
    /// `-hidden-lfoo`: an archive whose external symbols are demoted
    /// to private externals.
    HiddenLib(String),
    /// `-needed-lfoo` / `-needed_framework Foo`: always keep the
    /// dylib's load command.
    NeededLib(String),
    NeededFramework(String),
    NeededFile(String),
}

/// Parsed command line arguments.
#[derive(Debug)]
pub struct Args {
    pub output: String,
    /// The output file type: MH_EXECUTE, MH_DYLIB or MH_BUNDLE.
    pub output_type: u32,
    pub install_name: Option<String>,
    /// -final_output: the install name a dylib gets when -install_name
    /// is absent (the compiler driver passes it with several -arch).
    pub final_output: Option<String>,
    /// -keep_private_externs: a -r output keeps private externals as
    /// such instead of making them non-external.
    pub keep_private_externs: bool,
    /// -bundle_loader: the executable a bundle's undefined symbols may
    /// resolve to, bound at run time as the main executable.
    pub bundle_loader: Option<String>,
    pub arch: Option<String>,
    pub entry: String,
    pub platform: u32,
    pub platform_minos: u32,
    pub platform_sdk: u32,
    pub syslibroot: Vec<String>,
    pub library_paths: Vec<String>,
    pub framework_paths: Vec<String>,
    pub inputs: Vec<InputArg>,
    pub rpaths: Vec<String>,
    pub adhoc_codesign: bool,
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
    pub exported_symbols: Option<Vec<String>>,
    /// -no_exported_symbols: hide every definition.
    pub no_exported_symbols: bool,
    /// Symbols to remove from the exported set.
    pub unexported_symbols: Vec<String>,
    /// -reexported_symbols_list: publish selected imports as exports.
    pub reexported_symbols: Vec<String>,
    pub current_version: u32,
    pub compatibility_version: u32,
    /// -map: write a map file describing the output layout.
    pub map: Option<String>,
    /// -dependency_info: write Xcode's binary dependency listing.
    pub dependency_info: Option<String>,
    /// -sdk_imports: Xcode's JSON report of imported APIs.
    pub sdk_imports: Option<String>,
    /// Emit chained fixups instead of classic dyld info. None means
    /// "decide from the deployment target".
    pub fixup_chains: Option<bool>,
    /// The libLTO to load for bitcode inputs (-lto_library).
    pub lto_library: Option<String>,
    /// -stack_size: the main thread's stack size, recorded in LC_MAIN.
    pub stack_size: u64,
    /// -sectcreate: sections to synthesize from files:
    /// (segment, section, path).
    pub sectcreate: Vec<(String, String, String)>,
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
    pub add_ast_paths: Vec<String>,
    pub dynamic: bool,
    pub headerpad: u64,
    /// -search_dylibs_first: search every path for a dylib before
    /// falling back to archives.
    pub search_dylibs_first: bool,
    /// -umbrella: declare this dylib a subframework of the named
    /// umbrella framework (LC_SUB_FRAMEWORK).
    pub umbrella: Option<String>,
    /// -oso_prefix: prefix to strip from N_OSO stab paths ("."  means
    /// the current directory).
    pub oso_prefix: Option<String>,
    /// -mark_dead_strippable_dylib: mark the output dylib as
    /// removable when a client binds nothing from it.
    pub mark_dead_strippable_dylib: bool,
    /// -export_dynamic: keep all global symbols through LTO even in an
    /// executable, for dlsym or plugin use.
    pub export_dynamic: bool,
    /// -order_file: files of symbol names; matching atoms are placed
    /// first in their output sections, in file order.
    pub order_files: Vec<String>,
    /// -executable_path: what @executable_path in dependent dylibs'
    /// install names stands for at link time (defaults to the output
    /// path when linking an executable).
    pub executable_path: Option<String>,
    /// -object_path_lto: keep the LTO-compiled object at this path for
    /// the debugger.
    pub object_path_lto: Option<String>,
    /// --print-dependencies: report which file satisfied each
    /// undefined symbol.
    pub print_dependencies: bool,
    /// -why_load: report which symbol caused each archive member to
    /// load.
    pub why_load: bool,
    /// -why_live: for each matching symbol, print the reference chain
    /// that kept it alive through -dead_strip ("*" wildcards allowed).
    pub why_live: Vec<String>,
    /// -alias/-alias_list: (existing, new) symbol aliases to define.
    pub aliases: Vec<(String, String)>,
    /// -sectalign: (segment, section, p2align) overrides.
    pub sectalign: Vec<(String, String, u8)>,
    /// -allowable_client: clients that may link this subframework
    /// (LC_SUB_CLIENT).
    pub allowable_clients: Vec<String>,
    /// -client_name: the name this link presents when checking
    /// subframework restrictions.
    pub client_name: Option<String>,
    /// -t: print each file that takes part in the link.
    pub trace: bool,
    /// -ignore_optimization_hints: skip LC_LINKER_OPTIMIZATION_HINT
    /// processing.
    pub ignore_optimization_hints: bool,
    /// -print_statistics: report phase timings and sizes to stderr.
    pub print_statistics: bool,
    /// -warn_duplicate_libraries (default): warn when one library is
    /// named more than once.
    pub warn_duplicate_libraries: bool,
    /// -non_global_symbols_strip_list: local symbols to drop from the
    /// output symbol table (glob patterns).
    pub local_strip_list: Vec<String>,
    /// -non_global_symbols_keep_list: if set, only matching local
    /// symbols stay.
    pub local_keep_list: Option<Vec<String>>,
    pub pagezero_size: u64,
    /// True when -pagezero_size was given explicitly (it is an error
    /// anywhere but a main executable).
    pub explicit_pagezero: bool,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            output: "a.out".to_string(),
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
            adhoc_codesign: true,
            dead_strip: false,
            strip_debug: false,
            all_load: false,
            load_objc: false,
            forced_undefined: Vec::new(),
            exported_symbols: None,
            no_exported_symbols: false,
            unexported_symbols: Vec::new(),
            reexported_symbols: Vec::new(),
            current_version: encode_version(1, 0, 0),
            compatibility_version: encode_version(1, 0, 0),
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
            why_live: Vec::new(),
            aliases: Vec::new(),
            sectalign: Vec::new(),
            allowable_clients: Vec::new(),
            client_name: None,
            trace: false,
            ignore_optimization_hints: false,
            print_statistics: false,
            warn_duplicate_libraries: true,
            local_strip_list: Vec::new(),
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
fn read_symbol_list(path: &str) -> Vec<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => symbol_list(&text),
        Err(_) => fatal!("cannot read symbol list: {path}"),
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

/// Expands @file response-file arguments, splitting the file's contents
/// on whitespace with simple quote handling.
pub fn expand_response_files(argv: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(argv.len());
    for arg in argv {
        if let Some(path) = arg.strip_prefix('@') {
            // Option arguments like "@rpath/libfoo.dylib" also start
            // with '@': expand only when the file actually exists.
            let Ok(text) = std::fs::read_to_string(path) else {
                out.push(arg.clone());
                continue;
            };
            let mut cur = String::new();
            let mut quote: Option<char> = None;
            for c in text.chars() {
                match quote {
                    Some(q) if c == q => quote = None,
                    Some(_) => cur.push(c),
                    None if c == '"' || c == '\'' => quote = Some(c),
                    None if c.is_whitespace() => {
                        if !cur.is_empty() {
                            out.push(std::mem::take(&mut cur));
                        }
                    }
                    None => cur.push(c),
                }
            }
            if !cur.is_empty() {
                out.push(cur);
            }
        } else {
            out.push(arg.clone());
        }
    }
    out
}

pub fn parse_args(cmdline: &[String]) -> Args {
    let mut args = Args::default();
    let mut i = 1;
    let mut version_shown = false;

    let next_arg = |i: &mut usize| -> &str {
        *i += 1;
        match cmdline.get(*i) {
            Some(val) => val,
            None => fatal!("option {}: argument missing", cmdline[*i - 1]),
        }
    };

    while i < cmdline.len() {
        let opt = cmdline[i].as_str();
        match opt {
            "-o" => args.output = next_arg(&mut i).to_string(),
            "-arch" => args.arch = Some(next_arg(&mut i).to_string()),
            "-e" => args.entry = next_arg(&mut i).to_string(),
            "-platform_version" => {
                args.platform = parse_platform(next_arg(&mut i));
                args.platform_minos = parse_version(next_arg(&mut i));
                args.platform_sdk = parse_version(next_arg(&mut i));
            }
            "-syslibroot" => args.syslibroot.push(next_arg(&mut i).to_string()),
            "-L" => args.library_paths.push(next_arg(&mut i).to_string()),
            "-l" => args.inputs.push(InputArg::Lib(next_arg(&mut i).to_string(), false)),
            "-framework" => {
                args.inputs.push(InputArg::Framework(next_arg(&mut i).to_string(), false))
            }
            "-weak_framework" => {
                args.inputs.push(InputArg::Framework(next_arg(&mut i).to_string(), true))
            }
            "-reexport_framework" => {
                args.inputs.push(InputArg::ReexportFramework(next_arg(&mut i).to_string()))
            }
            "-needed_framework" => {
                args.inputs.push(InputArg::NeededFramework(next_arg(&mut i).to_string()))
            }
            "-needed_library" => {
                args.inputs.push(InputArg::NeededFile(next_arg(&mut i).to_string()))
            }
            "-weak_library" => args.inputs.push(InputArg::WeakFile(next_arg(&mut i).to_string())),
            "-reexport_library" => {
                args.inputs.push(InputArg::ReexportFile(next_arg(&mut i).to_string()))
            }
            "-sub_library" => args.inputs.push(InputArg::ReexportLib(next_arg(&mut i).to_string())),
            "-filelist" => {
                // A file listing one input path per line, optionally
                // with a directory prefix after a comma.
                let arg = next_arg(&mut i).to_string();
                let (path, dir) = match arg.split_once(',') {
                    Some((path, dir)) => (path.to_string(), format!("{dir}/")),
                    None => (arg, String::new()),
                };
                match std::fs::read_to_string(&path) {
                    Ok(text) => {
                        for line in text.lines() {
                            if !line.is_empty() {
                                args.inputs.push(InputArg::File(format!("{dir}{line}")));
                            }
                        }
                    }
                    Err(_) => fatal!("cannot read -filelist file: {path}"),
                }
            }
            "-F" => args.framework_paths.push(next_arg(&mut i).to_string()),
            "-dylib" => args.output_type = MH_DYLIB,
            "-bundle" => args.output_type = MH_BUNDLE,
            "-bundle_loader" => args.bundle_loader = Some(next_arg(&mut i).to_string()),
            "-final_output" => args.final_output = Some(next_arg(&mut i).to_string()),
            "-keep_private_externs" => args.keep_private_externs = true,
            "-rpath" => args.rpaths.push(next_arg(&mut i).to_string()),
            "-install_name" | "-dylib_install_name" => {
                args.install_name = Some(next_arg(&mut i).to_string())
            }
            "-map" => args.map = Some(next_arg(&mut i).to_string()),
            "-sdk_imports" => args.sdk_imports = Some(next_arg(&mut i).to_string()),
            "-fixup_chains" => args.fixup_chains = Some(true),
            "-no_fixup_chains" => args.fixup_chains = Some(false),
            "-adhoc_codesign" => args.adhoc_codesign = true,
            "-no_adhoc_codesign" => args.adhoc_codesign = false,
            "-dynamic" => args.dynamic = true,
            "-headerpad" => args.headerpad = parse_hex(opt, next_arg(&mut i)),
            "-pagezero_size" => {
                args.pagezero_size = parse_hex(opt, next_arg(&mut i));
                args.explicit_pagezero = true;
            }
            "-stack_size" => args.stack_size = parse_hex(opt, next_arg(&mut i)),
            "-sectcreate" => {
                let seg = next_arg(&mut i).to_string();
                let sect = next_arg(&mut i).to_string();
                let file = next_arg(&mut i).to_string();
                args.sectcreate.push((seg, sect, file));
            }
            "-add_empty_section" => {
                let seg = next_arg(&mut i).to_string();
                let sect = next_arg(&mut i).to_string();
                args.add_empty_section.push((seg, sect));
            }
            "-x" => args.strip_locals = true,
            "-Z" => args.no_standard_dirs = true,
            "-r" => args.relocatable = true,
            "-flat_namespace" => args.flat_namespace = true,
            "-twolevel_namespace" => args.flat_namespace = false,
            "-undefined" => match next_arg(&mut i) {
                "error" => args.undefined_dynamic_lookup = false,
                "dynamic_lookup" => args.undefined_dynamic_lookup = true,
                t @ ("warning" | "suppress") => {
                    args.undefined_dynamic_lookup = true;
                    args.undefined_warning = true;
                    args.undefined_is_warning = t == "warning";
                }
                treatment => fatal!("-undefined: unsupported treatment: {treatment}"),
            },
            "-U" => args.allowed_undefined.push(next_arg(&mut i).to_string()),
            "-w" => args.suppress_warnings = true,
            "-fatal_warnings" => args.fatal_warnings = true,
            "-demangle" => args.demangle = true,
            "-help" => {
                println!("Usage: ld64.mold [options] file...");
                crate::error::exit_after_cleanup(0);
            }

            "-dead_strip" => args.dead_strip = true,
            "-dead_strip_dylibs" => args.dead_strip_dylibs = true,
            "-bind_at_load" => args.bind_at_load = true,
            "-application_extension" => args.application_extension = true,
            "-no_application_extension" => args.application_extension = false,
            "-add_ast_path" => args.add_ast_paths.push(next_arg(&mut i).to_string()),
            "-S" => args.strip_debug = true,
            "-all_load" => args.all_load = true,
            "-u" => args.forced_undefined.push(next_arg(&mut i).to_string()),
            "-exported_symbol" => args
                .exported_symbols
                .get_or_insert_with(Vec::new)
                .push(next_arg(&mut i).to_string()),
            "-no_exported_symbols" => args.no_exported_symbols = true,
            "-exported_symbols_list" => {
                let path = next_arg(&mut i).to_string();
                let list = args.exported_symbols.get_or_insert_with(Vec::new);
                match std::fs::read_to_string(&path) {
                    Ok(text) => list.extend(symbol_list(&text)),
                    Err(_) => fatal!("cannot read -exported_symbols_list: {path}"),
                }
            }
            "-unexported_symbol" => args.unexported_symbols.push(next_arg(&mut i).to_string()),
            "-unexported_symbols_list" => {
                let path = next_arg(&mut i).to_string();
                match std::fs::read_to_string(&path) {
                    Ok(text) => args.unexported_symbols.extend(symbol_list(&text)),
                    Err(_) => fatal!("cannot read -unexported_symbols_list: {path}"),
                }
            }
            "-reexported_symbols_list" => {
                let path = next_arg(&mut i).to_string();
                let text = std::fs::read_to_string(&path)
                    .unwrap_or_else(|_| fatal!("cannot read -reexported_symbols_list: {path}"));
                let names = symbol_list(&text);
                // Exact names force a reference even if no object
                // mentions them. Patterns only match existing symbols.
                args.forced_undefined
                    .extend(names.iter().filter(|name| !name.contains(['*', '?', '['])).cloned());
                args.reexported_symbols.extend(names);
            }
            // The -dylib_ spellings are the older names ld64 still
            // accepts; Xcode passes -dylib_compatibility_version.
            "-current_version" | "-dylib_current_version" => {
                args.current_version = parse_version(next_arg(&mut i))
            }
            "-compatibility_version" | "-dylib_compatibility_version" => {
                args.compatibility_version = parse_version(next_arg(&mut i))
            }
            // ld64 prints its version banner to stdout and continues
            // with the link.
            "-v" => {
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
            "-version_details" => {
                println!(
                    "{{\n\t\"version\": \"{LD64_COMPAT_VERSION}\",\n\t\"architectures\": [\n\t\t\"arm64\",\n\t\t\"x86_64\"\n\t]\n}}"
                );
                std::process::exit(0);
            }
            "-noall_load" => args.all_load = false,
            "-ObjC" => args.load_objc = true,
            "-force_load" => args.inputs.push(InputArg::ForceLoad(next_arg(&mut i).to_string())),

            // The default library search behavior already matches
            // -search_paths_first: each path is tried for both a dylib
            // and an archive before moving to the next.
            "-search_paths_first" => args.search_dylibs_first = false,
            "-search_dylibs_first" => args.search_dylibs_first = true,
            "-umbrella" => args.umbrella = Some(next_arg(&mut i).to_string()),
            "-oso_prefix" => args.oso_prefix = Some(next_arg(&mut i).to_string()),
            "-mark_dead_strippable_dylib" => args.mark_dead_strippable_dylib = true,
            "-export_dynamic" => args.export_dynamic = true,
            "-order_file" => args.order_files.push(next_arg(&mut i).to_string()),
            "--print-dependencies" => args.print_dependencies = true,
            "-why_load" | "-whyload" => args.why_load = true,
            "-why_live" => args.why_live.push(next_arg(&mut i).to_string()),
            "-allowable_client" => args.allowable_clients.push(next_arg(&mut i).to_string()),
            "-client_name" => args.client_name = Some(next_arg(&mut i).to_string()),
            "-t" => args.trace = true,
            "-ignore_optimization_hints" => args.ignore_optimization_hints = true,
            "-print_statistics" => args.print_statistics = true,
            "-warn_duplicate_libraries" => args.warn_duplicate_libraries = true,
            "-no_warn_duplicate_libraries" => args.warn_duplicate_libraries = false,
            "-non_global_symbols_strip_list" => {
                let path = next_arg(&mut i);
                args.local_strip_list.extend(read_symbol_list(path));
            }
            "-non_global_symbols_keep_list" => {
                let path = next_arg(&mut i);
                args.local_keep_list.get_or_insert_with(Vec::new).extend(read_symbol_list(path));
            }
            "-sectalign" => {
                let seg = next_arg(&mut i).to_string();
                let sect = next_arg(&mut i).to_string();
                let val = next_arg(&mut i);
                let align = parse_hex("-sectalign", val);
                if !align.is_power_of_two() {
                    fatal!("-sectalign: alignment not a power of two: {val}");
                }
                args.sectalign.push((seg, sect, align.trailing_zeros() as u8));
            }
            "-alias" => {
                let existing = next_arg(&mut i).to_string();
                let new = next_arg(&mut i).to_string();
                args.aliases.push((existing, new));
            }
            "-alias_list" => {
                let path = next_arg(&mut i);
                let Ok(text) = std::fs::read_to_string(path) else {
                    fatal!("cannot read -alias_list: {path}");
                };
                for line in text.lines() {
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
            "-executable_path" => args.executable_path = Some(next_arg(&mut i).to_string()),

            // Reserve enough header padding that install_name_tool can
            // grow install names in place.
            "-headerpad_max_install_names" => {
                args.headerpad = args.headerpad.max(1024);
            }

            "-no_deduplicate" => args.deduplicate = false,
            "-function_starts" => args.function_starts = true,
            "-init_offsets" => args.init_offsets = true,
            "-data_const" => args.data_const = true,
            "-no_data_const" => args.data_const = false,
            "-no_implicit_dylibs" => args.no_implicit_dylibs = true,
            "-objc_relative_method_lists" => args.objc_relative_method_lists = Some(true),
            "-no_objc_relative_method_lists" => args.objc_relative_method_lists = Some(false),
            "-objc_category_merging" => args.objc_category_merging = Some(true),
            "-no_objc_category_merging" => args.objc_category_merging = Some(false),
            "-no_function_starts" => args.function_starts = false,
            "-data_in_code_info" => args.data_in_code_info = true,
            "-no_data_in_code_info" => args.data_in_code_info = false,

            "-no_uuid" => args.uuid = false,

            // The old pre-LC_BUILD_VERSION way of stating the
            // deployment target, still emitted by clang for older
            // -mmacosx-version-min targets. It fixes the platform to
            // macOS; the SDK version stays unset, as ld64 records when
            // it isn't told.
            "-macos_version_min" | "-macosx_version_min" => {
                args.platform = PLATFORM_MACOS;
                args.platform_minos = parse_version(next_arg(&mut i));
            }

            // Ignored options. ld64 takes -O<n> as a linker
            // optimization level hint (Xcode passes -O0 for debug and
            // -Os for release builds). This linker's output is always
            // deterministic, so -reproducible has nothing to switch on.
            // -debug_variant silences ld64's warnings that only matter
            // for binaries shipped to customers; there are none here.
            "-reproducible" | "-debug_variant" | "-O0" | "-O1" | "-O2" | "-O3" | "-Os" | "-Oz" => {}

            "-lto_library" => args.lto_library = Some(next_arg(&mut i).to_string()),

            "-dependency_info" => args.dependency_info = Some(next_arg(&mut i).to_string()),

            // Ignored options with an argument
            "-object_path_lto" => args.object_path_lto = Some(next_arg(&mut i).to_string()),

            // Ignored options with an argument
            "-mllvm" => {
                next_arg(&mut i);
            }

            _ => {
                if let Some(name) = opt.strip_prefix("-reexport-l") {
                    args.inputs.push(InputArg::ReexportLib(name.to_string()));
                } else if let Some(name) = opt.strip_prefix("-hidden-l") {
                    args.inputs.push(InputArg::HiddenLib(name.to_string()));
                } else if let Some(name) = opt.strip_prefix("-needed-l") {
                    args.inputs.push(InputArg::NeededLib(name.to_string()));
                } else if let Some(name) = opt.strip_prefix("-weak-l") {
                    args.inputs.push(InputArg::Lib(name.to_string(), true));
                } else if let Some(name) = opt.strip_prefix("-l") {
                    args.inputs.push(InputArg::Lib(name.to_string(), false));
                } else if let Some(path) = opt.strip_prefix("-L") {
                    args.library_paths.push(path.to_string());
                } else if let Some(path) = opt.strip_prefix("-F") {
                    args.framework_paths.push(path.to_string());
                } else if opt.starts_with('-') {
                    fatal!("unknown command line option: {opt}");
                } else {
                    args.inputs.push(InputArg::File(opt.to_string()));
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
    if args.no_exported_symbols
        && (args.exported_symbols.is_some() || !args.unexported_symbols.is_empty())
    {
        fatal!("-no_exported_symbols cannot be used with -exported_symbol* or -unexported_symbol*");
    }
    args
}
