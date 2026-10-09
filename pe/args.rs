//! The command line of lld-link and MSVC link: `/name[:value]` options, which
//! are case-insensitive, input files, and `@file` response files. rustc passes
//! it to linkers for its MSVC-style targets, such as x86_64-unknown-uefi.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use mold_common::fatal;

pub const SUBSYSTEM_WINDOWS: u16 = 2;
pub const SUBSYSTEM_CONSOLE: u16 = 3;

const SUBSYSTEMS: &[(&str, u16)] = &[
    ("native", 1),
    ("windows", SUBSYSTEM_WINDOWS),
    ("console", SUBSYSTEM_CONSOLE),
    ("efi_application", 10),
    ("efi_boot_service_driver", 11),
    ("efi_runtime_driver", 12),
    ("efi_rom", 13),
];

/// Options that don't change the image: their effect is either not
/// implemented (no PDB or manifest is written) or is the default.
const IGNORED_OPTIONS: &[&str] = &[
    "nologo",
    "pdb",
    "pdbaltpath",
    "incremental",
    "manifest",
    "manifestuac",
    "dynamicbase",
    "highentropyva",
    "largeaddressaware",
    "timestamp",
    "verbose",
];

pub struct Options {
    pub output: PathBuf,
    /// The entry point symbol. If None, it depends on the subsystem.
    pub entry: Option<String>,
    pub subsystem: u16,
    pub image_base: Option<u64>,
    pub gc_sections: bool,
    pub nxcompat: bool,
    /// Symbols that the link must include, from `/include:`.
    pub includes: Vec<String>,
    /// Set by /nodefaultlib, which makes the link ignore libraries that objects ask for.
    pub no_default_lib: bool,
    pub inputs: Vec<PathBuf>,
}

/// Parses the arguments of a linker invocation. `argv[0]` is the program
/// name, and an initial `-flavor link` is skipped.
pub fn parse(argv: &[OsString]) -> Options {
    let mut raw = Vec::new();
    expand(&argv[1..], &mut raw, 0);
    if raw.first().is_some_and(|a| a == "-flavor") && raw.get(1).is_some_and(|a| a == "link") {
        raw.drain(..2);
    }

    let mut opts = Options {
        output: PathBuf::from("a.exe"),
        entry: None,
        subsystem: SUBSYSTEM_CONSOLE,
        image_base: None,
        gc_sections: true,
        nxcompat: true,
        includes: Vec::new(),
        no_default_lib: false,
        inputs: Vec::new(),
    };
    let mut debug = false;
    let mut opt_ref = None;

    for arg in &raw {
        let Some(text) = arg.to_str() else {
            opts.inputs.push(PathBuf::from(arg));
            continue;
        };
        let Some((name, value)) = split_option(text) else {
            opts.inputs.push(PathBuf::from(arg));
            continue;
        };
        let name = name.to_ascii_lowercase();
        match (name.as_str(), value) {
            ("out", Some(v)) => opts.output = PathBuf::from(v),
            ("entry", Some(v)) => opts.entry = Some(v.to_string()),
            ("include", Some(v)) => opts.includes.push(v.to_string()),
            ("subsystem", Some(v)) => {
                let (sub, _version) = v.split_once(',').unwrap_or((v, ""));
                let sub = sub.to_ascii_lowercase();
                let Some(&(_, code)) = SUBSYSTEMS.iter().find(|(n, _)| *n == sub) else {
                    fatal!("unsupported subsystem: {v}");
                };
                opts.subsystem = code;
            }
            ("base", Some(v)) => {
                let parsed = match v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
                    Some(hex) => u64::from_str_radix(hex, 16),
                    None => v.parse(),
                };
                let Ok(base) = parsed else {
                    fatal!("invalid base address: {v}");
                };
                opts.image_base = Some(base);
            }
            ("machine", Some(v)) => {
                if !v.eq_ignore_ascii_case("x64") && !v.eq_ignore_ascii_case("amd64") {
                    fatal!("unsupported machine: {v}; only x64 is supported");
                }
            }
            ("opt", Some(v)) => {
                for part in v.split(',') {
                    match part.to_ascii_lowercase().as_str() {
                        "ref" => opt_ref = Some(true),
                        "noref" => opt_ref = Some(false),
                        "icf" | "noicf" | "lbr" | "nolbr" => {}
                        p if p.starts_with("icf=") => {}
                        _ => fatal!("unsupported option: /opt:{part}"),
                    }
                }
            }
            ("nxcompat", None | Some("yes")) => opts.nxcompat = true,
            ("nxcompat", Some("no")) => opts.nxcompat = false,
            ("debug", _) => debug = true,
            ("nodefaultlib", None) => opts.no_default_lib = true,
            ("nodefaultlib", Some(_)) => {}
            (n, _) if IGNORED_OPTIONS.contains(&n) => {}
            _ => {
                // An argument that looks like an option but isn't one may be
                // an absolute path to an input file.
                if Path::new(text).is_file() {
                    opts.inputs.push(PathBuf::from(arg));
                } else {
                    fatal!("unsupported option: {text}");
                }
            }
        }
    }

    // Like lld, optimize away unreferenced sections unless debugging.
    opts.gc_sections = opt_ref.unwrap_or(!debug);
    if opts.inputs.is_empty() {
        fatal!("no input files");
    }
    opts
}

/// Splits `/name:value`, `/name` or `-name` into the name and the value.
/// Returns None if the argument has no option syntax, such as a relative path.
fn split_option(arg: &str) -> Option<(&str, Option<&str>)> {
    let rest = arg.strip_prefix('/').or_else(|| arg.strip_prefix('-'))?;
    let (name, value) = match rest.split_once(':') {
        Some((n, v)) => (n, Some(v)),
        None => (rest, None),
    };
    let valid = !name.is_empty()
        && name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    valid.then_some((name, value))
}

/// Appends the arguments in `args` to `out`, replacing each `@file` with the
/// arguments in that file.
fn expand(args: &[OsString], out: &mut Vec<OsString>, depth: u32) {
    for arg in args {
        let Some(path) = arg.to_str().and_then(|s| s.strip_prefix('@')) else {
            out.push(arg.clone());
            continue;
        };
        if depth > 16 {
            fatal!("response files nested too deeply: {path}");
        }
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) => fatal!("cannot open response file {path}: {e}"),
        };
        let nested: Vec<OsString> =
            split_command_line(&text).into_iter().map(OsString::from).collect();
        expand(&nested, out, depth + 1);
    }
}

/// Splits a response file into arguments. Whitespace separates arguments,
/// and double quotes group them. A backslash is literal unless it precedes a
/// quote, as in Windows command lines.
pub(crate) fn split_command_line(text: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut in_arg = false;
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'"') => {
                chars.next();
                cur.push('"');
                in_arg = true;
            }
            '"' => {
                in_quotes = !in_quotes;
                in_arg = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if in_arg {
                    args.push(std::mem::take(&mut cur));
                    in_arg = false;
                }
            }
            c => {
                cur.push(c);
                in_arg = true;
            }
        }
    }
    if in_arg {
        args.push(cur);
    }
    args
}
