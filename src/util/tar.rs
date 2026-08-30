//! A minimal tar writer, used by `--repro` to bundle input files.

use std::fs::File;
use std::io::{self, Seek, SeekFrom, Write};

use super::{align_to, path_clean};

const BLOCK_SIZE: u64 = 512;

/// A tar file consists of one or more ustar headers each followed by file
/// data. The `name` field of a ustar header is only 100 bytes long, so a
/// PAX header carrying the full path is emitted before every entry.
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

    let checksum: u32 = hdr.iter().map(|&b| b as u32).sum();
    let checksum = format!("{checksum:06o}\0");
    hdr[148..148 + checksum.len()].copy_from_slice(checksum.as_bytes());
    hdr
}

/// Builds a PAX extended header record for a path: "N path=...\n" where N
/// is the length of the whole record including N itself.
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
        self.out
            .write_all(&ustar_header(b"/", b"", attr.len() as u64, b'x'))?;
        self.out.write_all(attr.as_bytes())?;
        self.pad()?;

        self.out
            .write_all(&ustar_header(b"", b"0000664", data.len() as u64, b'0'))?;
        self.out.write_all(data)?;
        self.pad()?;

        // A tar file must end with two empty blocks.
        let pos = self.out.stream_position()?;
        self.out.set_len(pos + BLOCK_SIZE * 2)
    }

    fn pad(&mut self) -> io::Result<()> {
        let pos = self.out.stream_position()?;
        self.out.seek(SeekFrom::Start(align_to(pos, BLOCK_SIZE)))?;
        Ok(())
    }
}
