//! This file contains functions to read an archive file (.a file).
//! An archive file is just a bundle of object files. It's similar to
//! tar or zip, but the contents are not compressed.
//!
//! If an archive file is given to the linker, the linker pulls out
//! object files that are needed to resolve undefined symbols. So,
//! bunding object files as an archive and giving that archive to the
//! linker has a different meaning than directly giving the same set of
//! object files to the linker. The former links only needed object
//! files, while the latter links all the given object files.
//!
//! A Mach-O archive is the common ar format. Apple's ar uses the BSD
//! long-name convention - a member name of "#1/<len>" means the real
//! name is the first <len> bytes of the member body - and prepends a
//! __.SYMDEF index member, which a linker that parses every member
//! eagerly can simply skip. GNU ar's SysV long names are accepted too.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::mapped_file::MappedFile;
use crate::util::os_str;

const HEADER_SIZE: usize = 60;

/// A parsed archive member header.
struct ArHeader<'a> {
    name: &'a [u8],
    size: usize,
}

impl<'a> ArHeader<'a> {
    fn parse(bytes: &'a [u8]) -> Option<Self> {
        let bytes = bytes.get(..HEADER_SIZE)?;
        let size = parse_decimal(&bytes[48..58]);
        Some(ArHeader { name: &bytes[..16], size })
    }

    fn is_strtab(&self) -> bool {
        self.name.starts_with(b"// ")
    }

    fn is_symtab(&self) -> bool {
        self.name.starts_with(b"/ ") || self.name.starts_with(b"/SYM64/ ")
    }

    /// Returns the member's file name. A BSD-style long name is stored
    /// right after the header, so `body` is advanced past it.
    fn read_name(&self, strtab: &'a [u8], body: &mut &'a [u8]) -> &'a [u8] {
        // BSD-style long filename
        if let Some(rest) = self.name.strip_prefix(b"#1/") {
            let len = parse_decimal(rest);
            let (name, remaining) = body.split_at(len.min(body.len()));
            *body = remaining;
            return name.split(|&b| b == 0).next().unwrap_or(&[]);
        }

        // SysV-style long filename
        if let Some(rest) = self.name.strip_prefix(b"/") {
            let offset = parse_decimal(rest);
            let start = strtab.get(offset..).unwrap_or(&[]);
            let end = memchr::memmem::find(start, b"/\n").unwrap_or(start.len());
            return &start[..end];
        }

        // Short filename, space-padded and (in the SysV form) slash-terminated.
        let end = self.name.iter().position(|&b| b == b'/').unwrap_or(self.name.len());
        self.name[..end].trim_ascii_end()
    }
}

/// Parses the leading decimal digits of a field, like `atoi`.
fn parse_decimal(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .skip_while(|b| b.is_ascii_whitespace())
        .take_while(|b| b.is_ascii_digit())
        .fold(0, |acc, &b| acc * 10 + (b - b'0') as usize)
}

/// Iterates over the members of an archive as (name, body) pairs, skipping
/// the symbol table and string table.
fn archive_members(
    mf: &'static MappedFile,
) -> impl Iterator<Item = (&'static [u8], &'static [u8])> {
    let data = mf.data();
    let mut pos = 8;
    let mut strtab: &'static [u8] = &[];

    std::iter::from_fn(move || {
        loop {
            if data.len() - pos < 2 {
                return None;
            }

            // Each header is aligned to a 2 byte boundary.
            if pos % 2 != 0 {
                pos += 1;
            }

            let hdr = ArHeader::parse(&data[pos..])?;
            let body_start = pos + HEADER_SIZE;
            let body_end = (body_start + hdr.size).min(data.len());
            let mut body = &data[body_start..body_end];
            pos = body_end;

            // Read a string table.
            if hdr.is_strtab() {
                strtab = body;
                continue;
            }
            // Skip a symbol table.
            if hdr.is_symtab() {
                continue;
            }

            // Read the name field
            let name = hdr.read_name(strtab, &mut body);

            // Skip BSD archive symbol tables (__.SYMDEF, __.SYMDEF SORTED,
            // __.SYMDEF_64 ...).
            if name.starts_with(b"__.SYMDEF") {
                continue;
            }

            return Some((name, body));
        }
    })
}

/// The 1-based position of an archive member among all the archive's
/// entries, symbol table included, by which ld-prime tells members apart
/// in diagnostics: "libfoo.a[2](foo.o)" is the first object after the
/// __.SYMDEF.
pub fn member_index(member: &MappedFile) -> Option<usize> {
    let data = member.parent?.data();
    let target = member.data().as_ptr();
    let mut pos = 8;
    let mut index = 0;
    while data.len().saturating_sub(pos) >= 2 {
        if pos % 2 != 0 {
            pos += 1;
        }
        let hdr = ArHeader::parse(data.get(pos..)?)?;
        index += 1;
        let body_start = pos + HEADER_SIZE;
        let body_end = (body_start + hdr.size).min(data.len());
        let mut body = &data[body_start..body_end];
        // Only a #1/ long name moves the body; the strtab is not needed.
        hdr.read_name(&[], &mut body);
        if body.as_ptr() == target {
            return Some(index);
        }
        pos = body_end;
    }
    None
}

/// Opens members as they are consumed. A member is named
/// "archive(member)", as ld64 reports it.
pub fn read_archive_members(mf: &'static MappedFile) -> impl Iterator<Item = &'static MappedFile> {
    debug_assert!(mf.data().starts_with(b"!<arch>\n"));
    let base = mf.data().as_ptr() as usize;
    archive_members(mf).map(move |(name, body)| {
        let mut full_name = OsString::from(&mf.name);
        full_name.push("(");
        full_name.push(os_str(name));
        full_name.push(")");
        mf.slice(PathBuf::from(full_name), body.as_ptr() as usize - base, body.len())
    })
}
