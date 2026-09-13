//! On Linux, /usr/lib/x86_64-linux-gnu/libc.so is not actually
//! a shared object file but an ASCII text file containing a linker
//! script to include a "real" libc.so file. Therefore, we need to
//! support a (very limited) subset of the linker script language.
//!
//! The supported subset also includes `OUTPUT_FORMAT`, symbol assignments,
//! version scripts and dynamic lists.

use std::path::{Path, PathBuf};

use crate::arch::Arch;
use crate::cmdline::{DefsymValue, ReaderContext};
use crate::context::Context;
use crate::elf::*;
use crate::mapped_file::{must_open_file, open_file, MappedFile};
use crate::reader;
use crate::util;
use crate::{fatal, warn};

/// A version script pattern.
#[derive(Clone, Debug)]
pub struct VersionPattern {
    pub pattern: &'static [u8],
    pub source: &'static Path,
    pub ver_str: &'static [u8],
    pub ver_idx: u16,
    pub is_cpp: bool,
}

/// A dynamic list pattern.
#[derive(Clone, Debug)]
pub struct DynamicPattern {
    pub pattern: &'static [u8],
    pub source: &'static Path,
    pub is_cpp: bool,
}

pub struct Script<'a, E: Arch> {
    ctx: &'a mut Context<E>,
    rctx: &'a mut ReaderContext,
    mf: &'static MappedFile,
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
fn syntax_error(mf: &MappedFile, tok: &[u8], msg: &str) -> ! {
    let input = mf.data();
    let pos = offset_of(input, tok).min(input.len().saturating_sub(1));
    let (line_start, line) = get_line(input, pos);
    let lineno = input[..line_start].iter().filter(|&&b| b == b'\n').count() + 1;
    let label = format!("{}:{}: ", mf.name.display(), lineno);
    let indent = "mold: fatal: ".len() + label.len();
    let column = pos - line_start;
    fatal!(
        "{label}{}\n{}^ {msg}",
        util::display(line),
        " ".repeat(indent + column)
    );
}

fn tokenize(mf: &'static MappedFile) -> Vec<&'static [u8]> {
    let mut tokens = Vec::new();
    let mut input = mf.data();

    while let Some(&c) = input.first() {
        // C's isspace also accepts vertical tabs.
        if c.is_ascii_whitespace() || c == b'\x0b' {
            input = &input[1..];
            continue;
        }

        if input.starts_with(b"/*") {
            let Some(pos) = input[2..].windows(2).position(|w| w == b"*/") else {
                syntax_error(mf, input, "unclosed comment");
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
                syntax_error(mf, input, "unclosed string literal");
            };
            tokens.push(&input[..pos + 2]);
            input = &input[pos + 2..];
            continue;
        }

        let is_word_char = |b: u8| b >= 0x80 || b.is_ascii_alphanumeric() || b"_.$/\\~=+[]*?-!^:".contains(&b);
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

fn is_in_sysroot<E: Arch>(ctx: &Context<E>, path: &Path) -> bool {
    let mut sysroot = ctx.args.sysroot.clone();
    if sysroot.is_absolute() && !ctx.args.chroot.as_os_str().is_empty() {
        sysroot = ctx
            .args
            .chroot
            .join(util::clean_path(&sysroot).strip_prefix("/").unwrap());
    }
    let (Ok(path), Ok(sysroot)) = (path.canonicalize(), sysroot.canonicalize()) else {
        return false;
    };
    path != sysroot && path.starts_with(&sysroot)
}

/// Opens a file named by a script, retaining the pathname's original bytes.
fn resolve_path<E: Arch>(
    ctx: &Context<E>,
    rctx: &ReaderContext,
    mf: &'static MappedFile,
    tok: &'static [u8],
    check_target: bool,
) -> &'static MappedFile {
    let s = unquote(tok);
    let name = Path::new(util::os_str(s));
    let chroot = &ctx.args.chroot;
    let open = |path: &Path| -> Option<&'static MappedFile> {
        let mf = open_file(chroot, path)?;
        if check_target {
            if let Some(target) = reader::get_machine_type(ctx, rctx, mf) {
                if target != E::NAME {
                    warn!("{}: skipping incompatible file: {target} (e_machine {})",
                        path.display(), E::E_MACHINE);
                    return None;
                }
            }
        }
        Some(mf)
    };
    let in_sysroot = |suffix: &[u8]| {
        let mut path = ctx.args.sysroot.as_os_str().to_os_string();
        path.push(util::os_str(suffix));
        PathBuf::from(path)
    };

    // Absolute names in a script within the sysroot are relative to that root.
    if name.is_absolute() && is_in_sysroot(ctx, &mf.name) {
        return must_open_file(chroot, in_sysroot(s));
    }
    if let Some(rest) = s.strip_prefix(b"=") {
        return must_open_file(chroot, in_sysroot(rest));
    }
    if let Some(lib) = s.strip_prefix(b"-l") {
        return reader::find_library(ctx, rctx, util::os_str(lib));
    }
    if !name.is_absolute() {
        let path = util::clean_path(&mf.name.parent().unwrap_or(Path::new(".")).join(name));
        if let Some(mf) = open(&path) {
            return mf;
        }
    }
    if let Some(mf) = open(name) {
        return mf;
    }
    for dir in &ctx.args.library_paths {
        if let Some(mf) = open(&dir.join(name.strip_prefix("/").unwrap_or(name))) {
            return mf;
        }
    }
    syntax_error(mf, tok, &format!("library not found: {}", util::display(s)));
}

/// The target a script produces output for: the one `OUTPUT_FORMAT`
/// names, or else that of the first file it names.
pub fn output_target<E: Arch>(
    ctx: &Context<E>,
    rctx: &ReaderContext,
    mf: &'static MappedFile,
) -> Option<&'static str> {
    let tokens = tokenize(mf);
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
        Script { ctx, rctx, mf }
    }

    fn error(&self, tok: &[u8], msg: &str) -> ! {
        syntax_error(self.mf, tok, msg)
    }

    fn skip<'t>(&self, tok: &'t [&'static [u8]], expected: &str) -> &'t [&'static [u8]] {
        match tok.first() {
            None => fatal!("{}: expected '{expected}', but got EOF", self.mf.name.display()),
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
            None => fatal!("{}: expected ')', but got EOF", self.mf.name.display()),
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
            fatal!("{}: expected ')', but got EOF", self.mf.name.display());
        }
        &tok[1..]
    }

    pub fn parse_linker_script(&mut self) {
        let tokens = tokenize(self.mf);
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
                let name = unquote(tok[0]).to_vec();
                let value = unquote(tok[2]).to_vec();
                let value = DefsymValue::Symbol(value);
                self.ctx.args.defsyms.push((name, value));
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

            // Colons remain in tokens for C++ names and input paths. Strip
            // attached labels only while parsing version commands.
            let mut t = t;
            loop {
                if let Some(rest) = t.strip_prefix(b"global:") {
                    is_global = true;
                    t = rest;
                } else if let Some(rest) = t.strip_prefix(b"local:") {
                    is_global = false;
                    t = rest;
                } else {
                    break;
                }
            }
            if t.is_empty() {
                tok = &tok[1..];
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

            let idx = if is_global {
                ver_idx
            } else {
                VER_NDX_LOCAL as u16
            };
            if t == b"*" {
                self.ctx.default_version = idx;
            } else {
                let pattern = self.unquote_pattern(t);
                self.ctx.version_patterns.push(VersionPattern {
                    pattern,
                    source: &self.mf.name,
                    ver_str,
                    ver_idx: idx,
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
                self.ctx.args.version_definitions.push(t.to_vec());
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
        let tokens = tokenize(self.mf);
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
                source: &self.mf.name,
                is_cpp,
            });
            tok = self.skip(&tok[1..], ";");
        }
        tok
    }

    pub fn parse_dynamic_list(&mut self) -> Vec<DynamicPattern> {
        let tokens = tokenize(self.mf);
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

/// Matches a `global:` or `local:` label.
fn read_label<'t>(tok: &'t [&'static [u8]], label: &[u8]) -> Option<&'t [&'static [u8]]> {
    let first = *tok.first()?;
    if first.strip_suffix(b":") == Some(label) {
        return Some(&tok[1..]);
    }
    if first == label && tok.get(1) == Some(&&b":"[..]) {
        return Some(&tok[2..]);
    }
    None
}

pub fn parse_dynamic_list<E: Arch>(ctx: &mut Context<E>, path: &Path) -> Vec<DynamicPattern> {
    let mf = must_open_file(&ctx.args.chroot, path);
    let mut rctx = ReaderContext::default();
    Script::new(ctx, &mut rctx, mf).parse_dynamic_list()
}
