//! `.gnu_debuglink`, the pathname and checksum of separate debug information.

use crate::arch::Arch;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::util::align_to;
use crate::util::endian::Endian;
use crate::util::{path_filename, write_cstr};

// .gnu_debuglink section contains a pathname and its CRC32 checksum for a
// separate debug info file. gdb can read the section to read debug info
// from an external file.
#[derive(Debug)]
pub struct GnuDebuglinkSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
    pub filename: String,
    pub crc32: u32,
}

impl<E: Layout> GnuDebuglinkSection<E> {
    pub fn new() -> GnuDebuglinkSection<E> {
        let mut hdr = ChunkHeader::<E>::new(".gnu_debuglink", SHT_PROGBITS, 0);
        hdr.shdr.sh_addralign.set(4);
        GnuDebuglinkSection {
            hdr,
            filename: String::new(),
            crc32: 0,
        }
    }
}

impl<E: Layout> Default for GnuDebuglinkSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
    let filename = path_filename(&ctx.args.separate_debug_file);
    let size = align_to(filename.len() as u64 + 1, 4) + 4;
    let sec = ctx.gnu_debuglink.as_mut().unwrap();
    sec.hdr.shdr.sh_size.set(size);
    sec.filename = filename;
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    let sec = ctx.gnu_debuglink.as_ref().unwrap();
    buf.fill(0);
    write_cstr(buf, sec.filename.as_bytes());
    let n = buf.len();
    E::Endian::write_u32(&mut buf[n - 4..], sec.crc32);
}
