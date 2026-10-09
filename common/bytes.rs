//! Byte strings: names that need not be UTF-8, and the text of response
//! files, linker scripts and symbol lists.

/// Whether a byte is white space as isspace() takes it in the C locale,
/// without the function call that a tokenizer would otherwise make for
/// every byte of a response file.
pub fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// A byte string without the white space (see is_space) it starts and
/// ends with.
pub fn trim_space(mut bytes: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = bytes
        && is_space(*first)
    {
        bytes = rest;
    }
    while let [rest @ .., last] = bytes
        && is_space(*last)
    {
        bytes = rest;
    }
    bytes
}

/// The lines of a text file, as str::lines splits a string: at each
/// '\n', a '\r' before it dropped, the last line ended or not. Lists
/// of names (symbols, files) are bytes, whatever their encoding.
pub fn lines(text: &[u8]) -> impl Iterator<Item = &[u8]> {
    let n = if text.is_empty() { 0 } else { usize::MAX };
    let text = text.strip_suffix(b"\n").unwrap_or(text);
    let lines = text.split(|&c| c == b'\n').take(n);
    lines.map(|line| line.strip_suffix(b"\r").unwrap_or(line))
}

/// Splits a byte string at the first `sep`, as str::split_once does a
/// string.
pub fn split_once(bytes: &[u8], sep: u8) -> Option<(&[u8], &[u8])> {
    let i = memchr::memchr(sep, bytes)?;
    Some((&bytes[..i], &bytes[i + 1..]))
}

/// Writes a NUL-terminated string and returns the number of bytes written.
pub fn write_cstr(buf: &mut [u8], s: &[u8]) -> usize {
    buf[..s.len()].copy_from_slice(s);
    buf[s.len()] = 0;
    s.len() + 1
}

/// Returns the NUL-terminated string starting at `offset` in a string table.
/// The result excludes the terminator. A missing terminator yields the rest
/// of the table.
#[inline]
pub fn cstr_at(table: &[u8], offset: usize) -> &[u8] {
    let rest = table.get(offset..).unwrap_or(&[]);
    let end = memchr::memchr(0, rest).unwrap_or(rest.len());
    &rest[..end]
}

/// Converts bytes from a response file or linker script to an OS string.
/// Unix paths can contain arbitrary non-NUL bytes.
pub fn os_str(bytes: &[u8]) -> &std::ffi::OsStr {
    use bstr::ByteSlice;
    bytes.to_os_str().unwrap_or_else(|_| crate::fatal!("invalid OS string: {}", display(bytes)))
}

/// Formats a byte string for diagnostics, replacing invalid UTF-8.
pub fn display(bytes: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(bytes)
}
