//! This file contains functions to read an archive file (.a file).
//! An archive file is just a bundle of object files. It's similar to
//! tar or zip, but the contents are not compressed.
//!
//! An archive file is either "regular" or "thin". A regular archive
//! contains object files directly, while a thin archive contains only
//! pathnames. In the latter case, actual file contents have to be read
//! from given pathnames. A regular archive is sometimes called "fat"
//! archive as opposed to "thin".
//!
//! If an archive file is given to the linker, the linker pulls out
//! object files that are needed to resolve undefined symbols. So,
//! bunding object files as an archive and giving that archive to the
//! linker has a different meaning than directly giving the same set of
//! object files to the linker. The former links only needed object
//! files, while the latter links all the given object files.
//!
//! Therefore, if you link libc.a for example, not all the libc
//! functions are linked to your binary. Instead, only object files
//! that provides functions and variables used in your program get
//! linked. To make this efficient, static library functions are
//! usually separated to each object file in an archive file. You can
//! see the contents of libc.a by running `ar t
//! /usr/lib/x86_64-linux-gnu/libc.a`.

use crate::fatal;
use crate::mapped_file::MappedFile;
use crate::util;

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
        Some(ArHeader {
            name: &bytes[..16],
            size,
        })
    }

    fn is_strtab(&self) -> bool {
        self.name.starts_with(b"// ")
    }

    fn is_symtab(&self) -> bool {
        self.name.starts_with(b"/ ") || self.name.starts_with(b"/SYM64/ ")
    }

    /// Returns the member's file name. A BSD-style long name is stored
    /// right after the header, so `body` is advanced past it.
    fn read_name(&self, strtab: &[u8], body: &mut &'a [u8]) -> String {
        // BSD-style long filename
        if let Some(rest) = self.name.strip_prefix(b"#1/") {
            let len = parse_decimal(rest);
            let (name, remaining) = body.split_at(len.min(body.len()));
            *body = remaining;
            let name = name.split(|&b| b == 0).next().unwrap_or(&[]);
            return String::from_utf8_lossy(name).into_owned();
        }

        // SysV-style long filename
        if let Some(rest) = self.name.strip_prefix(b"/") {
            let offset = parse_decimal(rest);
            let start = strtab.get(offset..).unwrap_or(&[]);
            let end = start
                .windows(2)
                .position(|w| w == b"/\n")
                .unwrap_or(start.len());
            return String::from_utf8_lossy(&start[..end]).into_owned();
        }

        // Short fileanme
        let end = self
            .name
            .iter()
            .position(|&b| b == b'/')
            .unwrap_or(self.name.len());
        String::from_utf8_lossy(&self.name[..end]).into_owned()
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

/// Iterates over the members of an archive as (header, name, body)
/// triples, skipping the symbol table and string table.
fn for_each_member(mf: &'static MappedFile, thin: bool, mut f: impl FnMut(String, &'static [u8])) {
    let data = mf.data();
    let mut pos = 8;
    let mut strtab: &[u8] = &[];

    while data.len() - pos >= 2 {
        // Each header is aligned to a 2 byte boundary.
        if pos % 2 != 0 {
            pos += 1;
        }

        let Some(hdr) = ArHeader::parse(&data[pos..]) else {
            break;
        };
        let body_start = pos + HEADER_SIZE;
        let body_end = (body_start + hdr.size).min(data.len());
        let mut body = &data[body_start..body_end];

        // Read a string table.
        if hdr.is_strtab() {
            // Read if string table
            strtab = body;
            pos = body_end;
            continue;
        }
        // Skip a symbol table.
        if hdr.is_symtab() {
            // Skip if symbol table
            pos = body_end;
            continue;
        }

        if thin && !hdr.name.starts_with(b"#1/") && !hdr.name.starts_with(b"/") {
            fatal!("{}: filename is not stored as a long filename", mf.name);
        }

        // Read the name field
        let name = hdr.read_name(strtab, &mut body);

        if thin {
            // A thin archive member's contents live elsewhere; only a
            // BSD-style long name occupies space after the header.
            pos = body_start + (body_end - body_start - body.len());
        } else {
            pos = body_end;
        }

        // Thin-archive counterpart:
        // Skip if symbol table
        // Fat-archive counterpart:
        // Skip if symbol table
        if name == "__.SYMDEF" || name == "__.SYMDEF SORTED" {
            pos = body_end;
            continue;
        }

        f(name, body);
    }
}

/// Returns the paths of the members of a thin archive, which are stored
/// outside of the archive file, without opening them.
pub fn get_thin_archive_member_paths(mf: &'static MappedFile) -> Vec<String> {
    let mut paths = Vec::new();
    for_each_member(mf, true, |name, _| {
        if name.starts_with('/') {
            paths.push(name);
        } else {
            paths.push(format!("{}/{}", util::path_dirname(&mf.name), name));
        }
    });
    paths
}

pub fn read_thin_archive_members(mf: &'static MappedFile) -> Vec<&'static MappedFile> {
    get_thin_archive_member_paths(mf)
        .into_iter()
        .map(|path| {
            let member = MappedFile::must_open(&path);
            util::leak(MappedFile {
                name: member.name.clone(),
                data: member.data,
                given_fullpath: true,
                parent: None,
                thin_parent: Some(mf),
                is_dependency: std::sync::atomic::AtomicBool::new(true),
            })
        })
        .collect::<Vec<_>>()
}

pub fn read_fat_archive_members(mf: &'static MappedFile) -> Vec<&'static MappedFile> {
    let mut members = Vec::new();
    let base = mf.data().as_ptr() as usize;
    for_each_member(mf, false, |name, body| {
        let start = body.as_ptr() as usize - base;
        members.push(mf.slice(name, start, body.len()));
    });
    members
}

pub fn read_archive_members(mf: &'static MappedFile) -> Vec<&'static MappedFile> {
    if mf.data().starts_with(b"!<arch>\n") {
        read_fat_archive_members(mf)
    } else {
        debug_assert!(mf.data().starts_with(b"!<thin>\n"));
        read_thin_archive_members(mf)
    }
}
