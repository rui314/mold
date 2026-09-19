//! `.note.gnu.build-id`, the output file's build identifier.

use crate::arch::Arch;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;

// .note.gnu.build-id contains an identifier for an output ELF file. The
// contents of the section is usually a cryptogrpahic hash of the output
// file itself to guarantee uniqueness of build-id.
#[derive(Debug)]
pub struct BuildIdSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
    pub contents: Vec<u8>,
}

impl<E: Layout> BuildIdSection<E> {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::<E>::new(".note.gnu.build-id", SHT_NOTE, SHF_ALLOC as u64);
        hdr.shdr.sh_addralign.set(4);
        hdr.shdr.sh_size.set(1);
        Self { hdr, contents: Vec::new() }
    }
}

impl<E: Layout> Default for BuildIdSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
    let size = ctx.args.build_id.size() as u64 + 16; // +16 for the header
    ctx.buildid.as_mut().unwrap().hdr.shdr.sh_size.set(size);
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    let sec = ctx.buildid.as_ref().unwrap();
    buf.fill(0);
    E::write_u32(buf, 4); // Name size
    E::write_u32(&mut buf[4..], ctx.args.build_id.size() as u32); // Hash size
    E::write_u32(&mut buf[8..], NT_GNU_BUILD_ID);
    buf[12..16].copy_from_slice(b"GNU\0"); // Name string
    buf[16..16 + sec.contents.len()].copy_from_slice(&sec.contents); // Build ID
}
