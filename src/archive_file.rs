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

use std::path::{Path, PathBuf};

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
    fn read_name(&self, strtab: &[u8], body: &mut &'a [u8]) -> PathBuf {
        // BSD-style long filename
        if let Some(rest) = self.name.strip_prefix(b"#1/") {
            let len = parse_decimal(rest);
            let (name, remaining) = body.split_at(len.min(body.len()));
            *body = remaining;
            let name = name.split(|&b| b == 0).next().unwrap_or(&[]);
            return PathBuf::from(util::os_str(name));
        }

        // SysV-style long filename
        if let Some(rest) = self.name.strip_prefix(b"/") {
            let offset = parse_decimal(rest);
            let start = strtab.get(offset..).unwrap_or(&[]);
            let end = memchr::memmem::find(start, b"/\n").unwrap_or(start.len());
            return PathBuf::from(util::os_str(&start[..end]));
        }

        // Short fileanme
        let end = self
            .name
            .iter()
            .position(|&b| b == b'/')
            .unwrap_or(self.name.len());
        PathBuf::from(util::os_str(&self.name[..end]))
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
    thin: bool,
) -> impl Iterator<Item = (PathBuf, &'static [u8])> {
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
                fatal!(
                    "{}: filename is not stored as a long filename",
                    mf.name.display()
                );
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

            // Skip BSD archive symbol tables.
            if name == Path::new("__.SYMDEF") || name == Path::new("__.SYMDEF SORTED") {
                pos = body_end;
                continue;
            }

            return Some((name, body));
        }
    })
}

/// Returns the paths of the members of a thin archive, which are stored
/// outside of the archive file, without opening them.
pub fn get_thin_archive_member_paths(mf: &'static MappedFile) -> impl Iterator<Item = PathBuf> {
    archive_members(mf, true).map(move |(name, _)| {
        if name.is_absolute() {
            name
        } else {
            mf.name.parent().unwrap_or(Path::new(".")).join(name)
        }
    })
}

pub fn read_thin_archive_members<'a>(
    chroot: &'a Path,
    mf: &'static MappedFile,
) -> impl Iterator<Item = &'static MappedFile> + 'a {
    get_thin_archive_member_paths(mf).map(move |path| mf.open_thin_member(chroot, &path))
}

pub fn read_fat_archive_members(
    mf: &'static MappedFile,
) -> impl Iterator<Item = &'static MappedFile> {
    let base = mf.data().as_ptr() as usize;
    archive_members(mf, false).map(move |(name, body)| {
        let start = body.as_ptr() as usize - base;
        mf.slice(name, start, body.len())
    })
}

pub fn read_archive_members(chroot: &Path, mf: &'static MappedFile) -> Vec<&'static MappedFile> {
    if mf.data().starts_with(b"!<arch>\n") {
        read_fat_archive_members(mf).collect()
    } else {
        debug_assert!(mf.data().starts_with(b"!<thin>\n"));
        read_thin_archive_members(chroot, mf).collect()
    }
}
