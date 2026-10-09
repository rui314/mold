//! `.gnu_debuglink`, the pathname and checksum of separate debug information.

use mold_common::util::align_to;
use mold_common::util::write_cstr;

use crate::arch::Target;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;

// The .gnu_debuglink section contains the pathname and CRC32 checksum of
// a separate debug info file. gdb can use the section to read debug info
// from the external file.
#[derive(Debug)]
pub struct GnuDebuglinkSection<E: Target> {
    pub hdr: ChunkHeader<E>,
    pub crc32: u32,
}

impl<E: Target> GnuDebuglinkSection<E> {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::<E>::new(".gnu_debuglink", SHT_PROGBITS, 0);
        hdr.shdr.sh_addralign.set(4);
        Self { hdr, crc32: 0 }
    }
}

impl<E: Target> Default for GnuDebuglinkSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn update_shdr<E: Target>(ctx: &mut Context<E>) {
    let filename = ctx.args.separate_debug_file.file_name().unwrap_or_default().as_encoded_bytes();
    let size = align_to(filename.len() as u64 + 1, 4) + 4;
    let sec = ctx.gnu_debuglink.as_mut().unwrap();
    sec.hdr.shdr.sh_size.set(size);
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let sec = ctx.gnu_debuglink.as_ref().unwrap();
    buf.fill(0);
    let filename = ctx.args.separate_debug_file.file_name().unwrap_or_default();
    write_cstr(buf, filename.as_encoded_bytes());
    let n = buf.len();
    E::write_u32(&mut buf[n - 4..], sec.crc32);
}
