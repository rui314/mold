//! `.note.package`, package metadata.

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::target::Target;
use crate::util::align_to;
use crate::util::write_cstr;

// .note.package is an optional hint section that can contain arbitrary
// string. Package managers, such as dpkg or rpm, uses the section to
// embed package metadata into each ELF file so that it is easy to find
// the origin of an ELF file without any additional information.
pub fn new_header<E: Layout>() -> ChunkHeader<E> {
    let mut hdr = ChunkHeader::<E>::new(".note.package", SHT_NOTE, SHF_ALLOC as u64);
    hdr.shdr.sh_addralign.set(4);
    hdr
}

pub fn update_shdr<E: Target>(ctx: &mut Context<E>) {
    if !ctx.args.package_metadata.is_empty() {
        // +17 is for the header and the NUL terminator
        let size = align_to(ctx.args.package_metadata.len() as u64 + 17, 4);
        ctx.note_package.shdr.sh_size.set(size);
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let content_size = ctx.note_package.shdr.sh_size.get() as u32 - 16;
    buf.fill(0);
    E::write_u32(buf, 4); // Name size
    E::write_u32(&mut buf[4..], content_size);
    E::write_u32(&mut buf[8..], NT_FDO_PACKAGING_METADATA);
    buf[12..16].copy_from_slice(b"FDO\0");
    write_cstr(&mut buf[16..], &ctx.args.package_metadata); // Content
}
