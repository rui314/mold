//! Response files. If a command line argument is in the form of
//! `@path/to/some/file` (i.e. it starts with an at sign), the linker reads
//! the given file and interprets its contents as a list of command line
//! arguments. A file containing command line arguments is called a
//! "response file".
//!
//! A response file is often used to pass a very large number of arguments
//! to the linker without exceeding the kernel's command line length limit.

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::path::Path;

use bstr::ByteVec;

use crate::bytes::{display, is_space, os_str};
use crate::fatal;
use crate::mapped_file::MappedFile;

/// Replaces each "@path/to/some/text/file" argument with the arguments
/// the file holds. For the Mach-O linker (`macho`), an argument starting
/// with "@rpath", "@loader_path" or "@executable_path" is a dylib path,
/// not a response file, wherever it is, even an option's argument.
pub fn expand_response_files(argv: Vec<OsString>, macho: bool) -> Vec<Cow<'static, OsStr>> {
    let mut args = Vec::new();
    for arg in argv {
        if let Some(path) = response_file(arg.as_encoded_bytes(), macho) {
            args.extend(read_response_file(path, 1, macho));
        } else {
            args.push(Cow::Owned(arg));
        }
    }
    args
}

/// The response file an argument names, if it names one (see
/// expand_response_files).
fn response_file(arg: &[u8], macho: bool) -> Option<&Path> {
    const DYLIB_PATHS: [&[u8]; 3] = [b"@rpath", b"@loader_path", b"@executable_path"];
    if macho && DYLIB_PATHS.iter().any(|prefix| arg.starts_with(prefix)) {
        return None;
    }
    arg.strip_prefix(b"@").map(|path| Path::new(os_str(path)))
}

/// Opens a response file, tokenizes its contents, and returns the tokens.
fn read_response_file(path: &Path, depth: usize, macho: bool) -> Vec<Cow<'static, OsStr>> {
    if depth > 10 {
        fatal!("{}: response file nesting too deep", path.display());
    }

    let mf = MappedFile::must_open(path);
    mf.set_dependency(false);
    let data = mf.data();

    // Arguments are passed on as C strings, e.g. to the LTO plugin, so they
    // must not contain a NUL byte. Arguments given by the OS never do.
    if data.contains(&0) {
        fatal!("{}: response file contains a NUL byte", path.display());
    }

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
        if let Some(nested) = response_file(&tok, macho) {
            expanded.extend(read_response_file(nested, depth + 1, macho));
        } else {
            expanded.push(match tok {
                Cow::Borrowed(bytes) => Cow::Borrowed(os_str(bytes)),
                Cow::Owned(bytes) => Cow::Owned(
                    bytes
                        .into_os_string()
                        .unwrap_or_else(|e| fatal!("invalid OS string: {}", display(e.as_bytes()))),
                ),
            });
        }
    }
    expanded
}
