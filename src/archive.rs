//! Archive (`.a`) file reading.
//!
//! An archive is a bundle of object files, similar to tar but without
//! compression. A regular ("fat") archive contains the object files
//! themselves; a thin archive contains only their paths.
//!
//! Giving an archive to the linker means something different from giving
//! its members directly: only the members needed to resolve undefined
//! symbols are linked. That is why linking `libc.a` doesn't pull the whole
//! C library into an executable.

use crate::diagnostics::Diagnostics;
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

        // Short filename
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
fn for_each_member(
    diag: &Diagnostics,
    mf: &'static MappedFile,
    thin: bool,
    mut f: impl FnMut(String, &'static [u8]),
) {
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

        if hdr.is_strtab() {
            strtab = body;
            pos = body_end;
            continue;
        }
        if hdr.is_symtab() {
            pos = body_end;
            continue;
        }

        if thin && !hdr.name.starts_with(b"#1/") && !hdr.name.starts_with(b"/") {
            fatal!(
                diag,
                "{}: filename is not stored as a long filename",
                mf.name
            );
        }

        let name = hdr.read_name(strtab, &mut body);

        if thin {
            // A thin archive member's contents live elsewhere; only a
            // BSD-style long name occupies space after the header.
            pos = body_start + (body_end - body_start - body.len());
        } else {
            pos = body_end;
        }

        // Skip the symbol table
        if name == "__.SYMDEF" || name == "__.SYMDEF SORTED" {
            continue;
        }

        f(name, body);
    }
}

/// Returns the paths of the members of a thin archive without opening them.
pub fn get_thin_archive_member_paths(diag: &Diagnostics, mf: &'static MappedFile) -> Vec<String> {
    let mut paths = Vec::new();
    for_each_member(diag, mf, true, |name, _| {
        if name.starts_with('/') {
            paths.push(name);
        } else {
            paths.push(format!("{}/{}", util::path_dirname(&mf.name), name));
        }
    });
    paths
}

pub fn read_thin_archive_members(
    diag: &Diagnostics,
    mf: &'static MappedFile,
) -> Vec<&'static MappedFile> {
    get_thin_archive_member_paths(diag, mf)
        .into_iter()
        .map(|path| {
            let member = MappedFile::must_open(diag, &path);
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

pub fn read_fat_archive_members(
    diag: &Diagnostics,
    mf: &'static MappedFile,
) -> Vec<&'static MappedFile> {
    let mut members = Vec::new();
    let base = mf.data().as_ptr() as usize;
    for_each_member(diag, mf, false, |name, body| {
        let start = body.as_ptr() as usize - base;
        members.push(mf.slice(name, start, body.len()));
    });
    members
}

pub fn read_archive_members(
    diag: &Diagnostics,
    mf: &'static MappedFile,
) -> Vec<&'static MappedFile> {
    if mf.data().starts_with(b"!<arch>\n") {
        read_fat_archive_members(diag, mf)
    } else {
        debug_assert!(mf.data().starts_with(b"!<thin>\n"));
        read_thin_archive_members(diag, mf)
    }
}
