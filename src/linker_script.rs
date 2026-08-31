//! On Linux, /usr/lib/x86_64-linux-gnu/libc.so is not actually
//! a shared object file but an ASCII text file containing a linker
//! script to include a "real" libc.so file. Therefore, we need to
//! support a (very limited) subset of the linker script language.
//!
//! The supported subset also includes `OUTPUT_FORMAT`, symbol assignments,
//! version scripts and dynamic lists.

use crate::arch::Arch;
use crate::cmdline::{DefsymValue, ReaderContext};
use crate::context::Context;
use crate::elf::*;
use crate::mapped_file::{must_open_file, open_file, MappedFile};
use crate::reader;
use crate::util::{self, path_clean};
use crate::{fatal, warn};

/// A version script pattern.
#[derive(Clone, Debug)]
pub struct VersionPattern {
    pub pattern: &'static [u8],
    pub source: String,
    pub ver_str: &'static [u8],
    pub ver_idx: u16,
    pub is_cpp: bool,
}

/// A dynamic list pattern.
#[derive(Clone, Debug)]
pub struct DynamicPattern {
    pub pattern: &'static [u8],
    pub source: String,
    pub is_cpp: bool,
}

pub struct Script<'a, E: Arch> {
    ctx: &'a mut Context<E>,
    rctx: &'a mut ReaderContext,
    mf: &'static MappedFile,
    tokens: Vec<&'static [u8]>,
}

fn get_line(input: &[u8], pos: usize) -> (usize, &[u8]) {
    let start = input[..pos]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1);
    let end = input[pos..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(input.len(), |i| pos + i);
    (start, &input[start..end])
}

/// Position of a token's bytes within the script.
fn offset_of(input: &[u8], tok: &[u8]) -> usize {
    tok.as_ptr() as usize - input.as_ptr() as usize
}

fn unquote(s: &[u8]) -> &[u8] {
    match s.strip_prefix(b"\"") {
        Some(rest) => rest.strip_suffix(b"\"").unwrap_or(rest),
        None => s,
    }
}

/// Reports a syntax error, pointing at the offending token.
fn syntax_error<E: Arch>(ctx: &Context<E>, mf: &MappedFile, tok: &[u8], msg: &str) -> ! {
    let input = mf.data();
    let pos = offset_of(input, tok).min(input.len().saturating_sub(1));
    let (line_start, line) = get_line(input, pos);
    let lineno = input[..line_start].iter().filter(|&&b| b == b'\n').count() + 1;
    let label = format!("{}:{}: ", mf.name, lineno);
    let indent = "mold: fatal: ".len() + label.len();
    let column = pos - line_start;
    fatal!(
        ctx,
        "{label}{}\n{}^ {msg}",
        util::display(line),
        " ".repeat(indent + column)
    );
}

fn tokenize<E: Arch>(ctx: &Context<E>, mf: &'static MappedFile) -> Vec<&'static [u8]> {
    let mut tokens = Vec::new();
    let mut input = mf.data();

    while let Some(&c) = input.first() {
        if c.is_ascii_whitespace() {
            input = &input[1..];
            continue;
        }

        if input.starts_with(b"/*") {
            let Some(pos) = input[2..].windows(2).position(|w| w == b"*/") else {
                syntax_error(ctx, mf, input, "unclosed comment");
            };
            input = &input[pos + 4..];
            continue;
        }

        if c == b'#' {
            match input.iter().position(|&b| b == b'\n') {
                Some(pos) => input = &input[pos + 1..],
                None => break,
            }
            continue;
        }

        if c == b'"' {
            let Some(pos) = input[1..].iter().position(|&b| b == b'"') else {
                syntax_error(ctx, mf, input, "unclosed string literal");
            };
            tokens.push(&input[..pos + 2]);
            input = &input[pos + 2..];
            continue;
        }

        let is_word_char = |b: u8| b.is_ascii_alphanumeric() || b"_.$/\\~=+[]*?-!^:".contains(&b);
        let len = match input.iter().position(|&b| !is_word_char(b)) {
            Some(0) => 1,
            Some(pos) => pos,
            None => input.len(),
        };
        tokens.push(&input[..len]);
        input = &input[len..];
    }
    tokens
}

fn is_in_sysroot<E: Arch>(ctx: &Context<E>, path: &str) -> bool {
    let mut sysroot = ctx.args.sysroot.clone();
    if sysroot.starts_with('/') && !ctx.args.chroot.is_empty() {
        sysroot = format!("{}/{}", ctx.args.chroot, path_clean(&sysroot));
    }
    let path = std::path::Path::new(path);
    let sysroot = std::path::Path::new(&sysroot);
    let (Ok(path), Ok(sysroot)) = (path.canonicalize(), sysroot.canonicalize()) else {
        return false;
    };
    path != sysroot && path.starts_with(&sysroot)
}

/// Opens a file named by the script `mf`, resolving relative names
/// against the script's directory and the library search paths.
fn resolve_path<E: Arch>(
    ctx: &Context<E>,
    rctx: &ReaderContext,
    mf: &'static MappedFile,
    tok: &'static [u8],
    check_target: bool,
) -> &'static MappedFile {
    let s = String::from_utf8_lossy(unquote(tok)).into_owned();
    let diag = &ctx.diag;
    let chroot = &ctx.args.chroot;

    let open = |path: &str| -> Option<&'static MappedFile> {
        let mf = open_file(diag, chroot, path)?;
        if check_target {
            if let Some(target) =
                crate::filetype::get_machine_type(diag, &ctx.args.plugin, mf, || None)
            {
                if target != E::NAME {
                    warn!(
                        ctx,
                        "{path}: skipping incompatible file: {target} (e_machine {})",
                        E::E_MACHINE
                    );
                    return None;
                }
            }
        }
        Some(mf)
    };

    // GNU ld prepends the sysroot if a pathname starts with '/' and the
    // script being processed is in the sysroot. We do the same.
    if s.starts_with('/') && is_in_sysroot(ctx, &mf.name) {
        let path = format!("{}{s}", ctx.args.sysroot);
        return must_open_file(diag, chroot, &path);
    }

    if let Some(rest) = s.strip_prefix('=') {
        let path = if ctx.args.sysroot.is_empty() {
            rest.to_string()
        } else {
            format!("{}{rest}", ctx.args.sysroot)
        };
        return must_open_file(diag, chroot, &path);
    }

    if let Some(lib) = s.strip_prefix("-l") {
        return reader::find_library(ctx, rctx, lib);
    }

    if !s.starts_with('/') {
        let path = path_clean(&format!("{}/../{s}", mf.name));
        if let Some(mf) = open(&path) {
            return mf;
        }
    }

    if let Some(mf) = open(&s) {
        return mf;
    }

    for dir in &ctx.args.library_paths {
        if let Some(mf) = open(&format!("{dir}/{s}")) {
            return mf;
        }
    }

    syntax_error(ctx, mf, tok, &format!("library not found: {s}"));
}

/// The target a script produces output for: the one `OUTPUT_FORMAT`
/// names, or else that of the first file it names.
pub fn output_target<E: Arch>(
    ctx: &Context<E>,
    rctx: &ReaderContext,
    mf: &'static MappedFile,
) -> Option<&'static str> {
    let tokens = tokenize(ctx, mf);
    let mut tok: &[&'static [u8]] = &tokens;

    if tok.len() >= 3 && tok[0] == b"OUTPUT_FORMAT" && tok[1] == b"(" {
        if tok[2] == b"elf64-x86-64" {
            return Some("x86_64");
        }
        if tok[2] == b"elf32-i386" {
            return Some("i386");
        }
    }

    if tok.len() >= 3 && (tok[0] == b"INPUT" || tok[0] == b"GROUP") && tok[1] == b"(" {
        if tok.len() >= 5 && tok[2] == b"AS_NEEDED" && tok[3] == b"(" {
            tok = &tok[2..];
        }
        let named = resolve_path(ctx, rctx, mf, tok[2], false);
        return reader::get_machine_type(ctx, rctx, named);
    }
    None
}

impl<'a, E: Arch> Script<'a, E> {
    pub fn new(
        ctx: &'a mut Context<E>,
        rctx: &'a mut ReaderContext,
        mf: &'static MappedFile,
    ) -> Self {
        let tokens = tokenize(ctx, mf);
        Script {
            ctx,
            rctx,
            mf,
            tokens,
        }
    }

    fn error(&self, tok: &[u8], msg: &str) -> ! {
        syntax_error(self.ctx, self.mf, tok, msg)
    }

    fn skip<'t>(&self, tok: &'t [&'static [u8]], expected: &str) -> &'t [&'static [u8]] {
        match tok.first() {
            None => fatal!(
                self.ctx,
                "{}: expected '{expected}', but got EOF",
                self.mf.name
            ),
            Some(t) if *t == expected.as_bytes() => &tok[1..],
            Some(t) => self.error(t, &format!("expected '{expected}'")),
        }
    }

    /// Version scripts and dynamic lists take a quoted name literally, while
    /// an unquoted one is a glob pattern. We escape glob metacharacters in a
    /// quoted name so that the pattern matcher treats them as literals.
    fn unquote_pattern(&self, tok: &'static [u8]) -> &'static [u8] {
        if !tok.starts_with(b"\"") {
            return tok;
        }
        let s = unquote(tok);
        if !s.iter().any(|&c| matches!(c, b'*' | b'?' | b'[' | b'\\')) {
            return s;
        }
        let mut out = Vec::with_capacity(s.len() * 2);
        for &c in s {
            if matches!(c, b'*' | b'?' | b'[' | b'\\') {
                out.push(b'\\');
            }
            out.push(c);
        }
        util::leak_bytes(out)
    }

    fn read_output_format<'t>(&self, tok: &'t [&'static [u8]]) -> &'t [&'static [u8]] {
        let tok = self.skip(tok, "(");
        match tok.iter().position(|t| *t == b")") {
            Some(pos) => &tok[pos + 1..],
            None => fatal!(self.ctx, "{}: expected ')', but got EOF", self.mf.name),
        }
    }

    fn read_group<'t>(&mut self, mut tok: &'t [&'static [u8]]) -> &'t [&'static [u8]] {
        tok = self.skip(tok, "(");

        while let Some(&t) = tok.first() {
            if t == b")" {
                break;
            }
            if t == b"AS_NEEDED" {
                let orig = self.rctx.as_needed;
                self.rctx.as_needed = true;
                tok = self.read_group(&tok[1..]);
                self.rctx.as_needed = orig;
                continue;
            }

            let mf = resolve_path(self.ctx, self.rctx, self.mf, t, true);
            let mut child = self.rctx.next_child();
            reader::read_file(self.ctx, &mut child, mf);
            tok = &tok[1..];
        }

        if tok.is_empty() {
            fatal!(self.ctx, "{}: expected ')', but got EOF", self.mf.name);
        }
        &tok[1..]
    }

    pub fn parse_linker_script(&mut self) {
        let tokens = self.tokens.clone();
        let mut tok: &[&'static [u8]] = &tokens;

        while let Some(&t) = tok.first() {
            if t == b"OUTPUT_FORMAT" {
                tok = self.read_output_format(&tok[1..]);
            } else if t == b"INPUT" || t == b"GROUP" {
                tok = self.read_group(&tok[1..]);
            } else if t == b"VERSION" {
                tok = self.skip(&tok[1..], "{");
                tok = self.read_version_script(tok);
                tok = self.skip(tok, "}");
            } else if tok.len() > 3 && tok[1] == b"=" && tok[3] == b";" {
                let name = String::from_utf8_lossy(unquote(tok[0])).into_owned();
                let value = String::from_utf8_lossy(unquote(tok[2])).into_owned();
                self.ctx
                    .args
                    .defsyms
                    .push((name, DefsymValue::Symbol(value)));
                tok = &tok[4..];
            } else if t == b";" {
                tok = &tok[1..];
            } else {
                self.error(t, "unknown linker script token");
            }
        }
    }

    fn read_version_script_commands<'t>(
        &mut self,
        mut tok: &'t [&'static [u8]],
        ver_str: &'static [u8],
        ver_idx: u16,
        mut is_global: bool,
        is_cpp: bool,
    ) -> &'t [&'static [u8]] {
        while let Some(&t) = tok.first() {
            if t == b"}" {
                break;
            }

            if let Some(rest) = read_label(tok, b"global") {
                is_global = true;
                tok = rest;
                continue;
            }
            if let Some(rest) = read_label(tok, b"local") {
                is_global = false;
                tok = rest;
                continue;
            }

            if t == b"extern" {
                tok = &tok[1..];
                if tok.first() == Some(&&b"\"C\""[..]) {
                    tok = self.skip(&tok[1..], "{");
                    tok =
                        self.read_version_script_commands(tok, ver_str, ver_idx, is_global, false);
                } else {
                    tok = self.skip(tok, "\"C++\"");
                    tok = self.skip(tok, "{");
                    tok = self.read_version_script_commands(tok, ver_str, ver_idx, is_global, true);
                }
                tok = self.skip(tok, "}");
                tok = self.skip(tok, ";");
                continue;
            }

            if t == b"*" {
                self.ctx.default_version = if is_global {
                    ver_idx
                } else {
                    VER_NDX_LOCAL as u16
                };
            } else {
                let pattern = self.unquote_pattern(t);
                self.ctx.version_patterns.push(VersionPattern {
                    pattern,
                    source: self.mf.name.clone(),
                    ver_str,
                    ver_idx: if is_global {
                        ver_idx
                    } else {
                        VER_NDX_LOCAL as u16
                    },
                    is_cpp,
                });
            }

            tok = &tok[1..];
            if tok.first() == Some(&&b"}"[..]) {
                break;
            }
            tok = self.skip(tok, ";");
        }
        tok
    }

    fn read_version_script<'t>(&mut self, mut tok: &'t [&'static [u8]]) -> &'t [&'static [u8]] {
        let mut next_ver =
            VER_NDX_LAST_RESERVED as u16 + self.ctx.args.version_definitions.len() as u16 + 1;

        while let Some(&t) = tok.first() {
            if t == b"}" {
                break;
            }

            let (ver_str, ver_idx): (&'static [u8], u16) = if t == b"{" {
                (b"global", VER_NDX_GLOBAL as u16)
            } else {
                let idx = next_ver;
                next_ver += 1;
                self.ctx
                    .args
                    .version_definitions
                    .push(String::from_utf8_lossy(t).into_owned());
                tok = &tok[1..];
                (t, idx)
            };

            tok = self.skip(tok, "{");
            tok = self.read_version_script_commands(tok, ver_str, ver_idx, true, false);
            tok = self.skip(tok, "}");
            if let Some(&t) = tok.first() {
                if t != b";" {
                    // A parent version, e.g. `} VER_1.0;`
                    tok = &tok[1..];
                }
            }
            tok = self.skip(tok, ";");
        }
        tok
    }

    pub fn parse_version_script(&mut self) {
        let tokens = split_labels(&self.tokens);
        let tok = self.read_version_script(&tokens);
        if let Some(&t) = tok.first() {
            self.error(t, "trailing garbage token");
        }
    }

    fn read_dynamic_list_commands<'t>(
        &mut self,
        mut tok: &'t [&'static [u8]],
        result: &mut Vec<DynamicPattern>,
        is_cpp: bool,
    ) -> &'t [&'static [u8]] {
        while let Some(&t) = tok.first() {
            if t == b"}" {
                break;
            }

            if t == b"extern" {
                tok = &tok[1..];
                if tok.first() == Some(&&b"\"C\""[..]) {
                    tok = self.skip(&tok[1..], "{");
                    tok = self.read_dynamic_list_commands(tok, result, false);
                } else {
                    tok = self.skip(tok, "\"C++\"");
                    tok = self.skip(tok, "{");
                    tok = self.read_dynamic_list_commands(tok, result, true);
                }
                tok = self.skip(tok, "}");
                tok = self.skip(tok, ";");
                continue;
            }

            result.push(DynamicPattern {
                pattern: self.unquote_pattern(t),
                source: self.mf.name.clone(),
                is_cpp,
            });
            tok = self.skip(&tok[1..], ";");
        }
        tok
    }

    pub fn parse_dynamic_list(&mut self) -> Vec<DynamicPattern> {
        let tokens = self.tokens.clone();
        let mut result = Vec::new();

        let tok = self.skip(&tokens, "{");
        let tok = self.read_dynamic_list_commands(tok, &mut result, false);
        let tok = self.skip(tok, "}");
        let tok = self.skip(tok, ";");
        if let Some(&t) = tok.first() {
            self.error(t, "trailing garbage token");
        }
        result
    }
}

/// The tokenizer keeps a colon in a token because of the C++ scope
/// operator, so `local:*` is a single token. Split the pattern off.
fn split_labels(tokens: &[&'static [u8]]) -> Vec<&'static [u8]> {
    let mut out = Vec::with_capacity(tokens.len());
    for &tok in tokens {
        let glued = [&b"global:"[..], b"local:"].into_iter().find(|label| {
            tok.starts_with(label) && tok.len() > label.len() && tok[label.len()] != b':'
        });
        match glued {
            Some(label) => {
                let colon = label.len() - 1;
                out.extend([&tok[..colon], &tok[colon..colon + 1], &tok[colon + 1..]]);
            }
            None => out.push(tok),
        }
    }
    out
}

/// Matches a `global:` or `local:` label.
fn read_label<'t>(tok: &'t [&'static [u8]], label: &[u8]) -> Option<&'t [&'static [u8]]> {
    let first = *tok.first()?;
    let with_colon = [label, b":"].concat();
    if first == with_colon.as_slice() {
        return Some(&tok[1..]);
    }
    if first == label && tok.get(1) == Some(&&b":"[..]) {
        return Some(&tok[2..]);
    }
    None
}

pub fn parse_dynamic_list<E: Arch>(ctx: &mut Context<E>, path: &str) -> Vec<DynamicPattern> {
    let mf = must_open_file(&ctx.diag, &ctx.args.chroot.clone(), path);
    let mut rctx = ReaderContext::default();
    Script::new(ctx, &mut rctx, mf).parse_dynamic_list()
}
