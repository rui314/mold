//! This file contains functions to read an archive file (.a file).
//! An archive file is just a bundle of object files. It's similar to
//! tar or zip, but the contents are not compressed.
//!
//! An archive file is either "regular" or "thin". A regular archive
//! contains object files directly, while a thin archive contains only
//! pathnames. In the latter case, actual file contents have to be read
//! from given pathnames. A regular archive is sometimes called a "fat"
//! archive as opposed to "thin".
//!
//! If an archive file is given to the linker, the linker pulls out
//! object files that are needed to resolve undefined symbols. So,
//! bundling object files as an archive and giving that archive to the
//! linker has a different meaning than directly giving the same set of
//! object files to the linker. The former links only needed object
//! files, while the latter links all the given object files.
//!
//! Therefore, if you link libc.a for example, not all the libc
//! functions are linked to your binary. Instead, only object files
//! that provide functions and variables used in your program get
//! linked. To make this efficient, static library functions are
//! usually placed in separate object files in an archive file. You can
//! see the contents of libc.a by running `ar t
//! /usr/lib/x86_64-linux-gnu/libc.a`.
//!
//! GNU ar stores long member names in a string table member. BSD ar,
//! which macOS uses, stores a long name right after the member header
//! instead, and puts a symbol table in a member named __.SYMDEF. Both
//! forms are read here.

use std::path::{Path, PathBuf};

use crate::bytes::{cstr_at, os_str};
use crate::fatal;
use crate::mapped_file::{MappedFile, apply_chroot};

const HEADER_SIZE: usize = 60;

/// A parsed archive member header.
struct ArHeader<'a> {
    name: &'a [u8],
    date: u64,
    size: usize,
}

impl<'a> ArHeader<'a> {
    fn parse(bytes: &'a [u8]) -> Option<Self> {
        let bytes = bytes.get(..HEADER_SIZE)?;
        let date = parse_decimal(&bytes[16..28]) as u64;
        let size = parse_decimal(&bytes[48..58]);
        Some(ArHeader { name: &bytes[..16], date, size })
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
            return cstr_at(name, 0);
        }

        // SysV-style long filename
        if let Some(rest) = self.name.strip_prefix(b"/") {
            let offset = parse_decimal(rest);
            let start = strtab.get(offset..).unwrap_or(&[]);
            let end = memchr::memmem::find(start, b"/\n").unwrap_or(start.len());
            return &start[..end];
        }

        // Short filename, space-padded and, in the SysV form, slash-terminated
        let end = memchr::memchr(b'/', self.name).unwrap_or(self.name.len());
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

/// An archive member. A thin archive's member has no contents in the
/// archive; its name is the path of the file that has them.
pub struct Member {
    pub name: &'static [u8],
    pub data: &'static [u8],
    /// The modification time in the member header.
    pub date: u64,
}

/// Iterates over the members of the archive `data`, regular or thin,
/// skipping the symbol tables and the string table. `path` is the
/// archive's name for error messages.
pub fn members<'a>(path: &'a Path, data: &'static [u8]) -> impl Iterator<Item = Member> + 'a {
    let thin = data.starts_with(b"!<thin>\n");
    debug_assert!(thin || data.starts_with(b"!<arch>\n"));
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
                strtab = body;
                pos = body_end;
                continue;
            }
            // Skip a symbol table.
            if hdr.is_symtab() {
                pos = body_end;
                continue;
            }

            if thin && !hdr.name.starts_with(b"#1/") && !hdr.name.starts_with(b"/") {
                fatal!("{}: filename is not stored as a long filename", path.display());
            }

            // Read the name field
            let name = hdr.read_name(strtab, &mut body);

            if thin {
                // A thin archive member's contents live elsewhere; only a
                // BSD-style long name occupies space after the header.
                pos = body_start + (body_end - body_start - body.len());
                body = &[];
            } else {
                pos = body_end;
            }

            // Skip BSD archive symbol tables (__.SYMDEF, __.SYMDEF SORTED,
            // __.SYMDEF_64, ...).
            if name.starts_with(b"__.SYMDEF") {
                pos = body_end;
                continue;
            }

            return Some(Member { name, data: body, date: hdr.date });
        }
    })
}

/// Returns the paths of the members of a thin archive, which are stored
/// outside of the archive file, without opening them.
pub fn get_thin_archive_member_paths<'a>(
    chroot: &'a Path,
    mf: &'static MappedFile,
) -> impl Iterator<Item = PathBuf> + 'a {
    members(&mf.name, mf.data()).map(move |member| member_path(chroot, mf, member.name))
}

// An absolute member name is looked up in the --chroot directory. A relative
// one is relative to the archive, whose path is already in that directory.
fn member_path(chroot: &Path, mf: &MappedFile, name: &[u8]) -> PathBuf {
    let name = Path::new(os_str(name));
    if name.is_absolute() {
        apply_chroot(chroot, name).into_owned()
    } else {
        mf.name.parent().unwrap_or(Path::new(".")).join(name)
    }
}

/// Opens members as they are consumed. Parallel readers can instead schedule
/// thin-member paths on workers using get_thin_archive_member_paths().
pub fn read_archive_members<'a>(
    chroot: &'a Path,
    mf: &'static MappedFile,
) -> impl Iterator<Item = &'static MappedFile> + 'a {
    let thin = mf.data().starts_with(b"!<thin>\n");
    let base = mf.data().as_ptr() as usize;
    members(&mf.name, mf.data()).map(move |member| {
        if thin {
            mf.open_thin_member(&member_path(chroot, mf, member.name))
        } else {
            let name = PathBuf::from(os_str(member.name));
            mf.slice(name, member.data.as_ptr() as usize - base, member.data.len())
        }
    })
}
