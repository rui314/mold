//! This file contains functions to create a tar file.

use std::fs::File;
use std::io::{self, Seek, SeekFrom, Write};

use super::{align_to, path_clean};

const BLOCK_SIZE: u64 = 512;

// TarWriter creates the archive used by --repro.
//
// If you pass `--repro` to mold, mold collects all input files and puts them
// into `<output-file-path>.repro.tar`, making it easy to run the same command
// with the same command-line arguments.
//
/// A tar file consists of one or more Ustar header followed by data.
/// Each Ustar header represents a single file in an archive.
///
/// tar is an old file format, and its `name` field is only 100 bytes long.
/// If `name` is longer than 100 bytes, we can emit a PAX header before a
/// Ustar header to store a long filename.
///
/// For simplicity, we always emit a PAX header even for a short filename.
pub struct TarWriter {
    out: File,
    basedir: String,
}

fn ustar_header(name: &[u8], mode: &[u8], size: u64, typeflag: u8) -> [u8; 512] {
    let mut hdr = [0u8; 512];
    hdr[..name.len()].copy_from_slice(name);
    hdr[100..100 + mode.len()].copy_from_slice(mode);
    let size = format!("{size:011o}");
    hdr[124..124 + size.len()].copy_from_slice(size.as_bytes());
    hdr[148..156].fill(b' ');
    hdr[156] = typeflag;
    hdr[257..262].copy_from_slice(b"ustar");
    hdr[263..265].copy_from_slice(b"00");

    // Compute checksum. The field is six octal digits, a NUL byte and a
    // trailing space.
    let checksum: u32 = hdr.iter().map(|&b| b as u32).sum();
    let checksum = format!("{checksum:06o}\0");
    hdr[148..148 + checksum.len()].copy_from_slice(checksum.as_bytes());
    hdr
}

/// Construct a string which contains something like
/// "16 path=foo/bar\n" where 16 is the size of the string
/// including the size string itself.
fn encode_path(basedir: &str, path: &str) -> String {
    let path = path_clean(&format!("{basedir}/{path}"));
    let len = " path=\n".len() + path.len();
    let total = len + len.to_string().len();
    let total = len + total.to_string().len();
    format!("{total} path={path}\n")
}

impl TarWriter {
    pub fn open(output_path: &str, basedir: &str) -> io::Result<TarWriter> {
        Ok(TarWriter {
            out: File::create(output_path)?,
            basedir: basedir.to_string(),
        })
    }

    pub fn append(&mut self, path: &str, data: &[u8]) -> io::Result<()> {
        let attr = encode_path(&self.basedir, path);
        // Write PAX header
        self.out
            .write_all(&ustar_header(b"/", b"", attr.len() as u64, b'x'))?;
        // Write pathname
        self.out.write_all(attr.as_bytes())?;
        self.pad()?;

        // Write Ustar header
        self.out
            .write_all(&ustar_header(b"", b"0000664", data.len() as u64, b'0'))?;
        // Write file contents
        self.out.write_all(data)?;
        self.pad()?;

        // A tar file must ends with two empty blocks
        let pos = self.out.stream_position()?;
        self.out.set_len(pos + BLOCK_SIZE * 2)
    }

    fn pad(&mut self) -> io::Result<()> {
        let pos = self.out.stream_position()?;
        self.out.seek(SeekFrom::Start(align_to(pos, BLOCK_SIZE)))?;
        Ok(())
    }
}
