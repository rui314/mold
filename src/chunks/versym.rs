//! `.gnu.version`, symbol version indices.

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::target::Target;

// .gnu.version section contains version indices as a parallel array for
// .dynsym. If a dynamic symbol is a defined one, its version information
// is in .gnu.version_d. Otherwise, it's in .gnu.version_r.
//
// .gnu.version contains a parallel table for .dynsym to specify symbol
// versions of undefined symbols. A symbol having an entry in .gnu.version
// must be resolved to a symbol with the exact same version string at
// runtime.
#[derive(Debug)]
pub struct VersymSection<E: Target> {
    pub hdr: ChunkHeader<E>,
    pub contents: Vec<u16>,
}

impl<E: Target> VersymSection<E> {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::<E>::new(".gnu.version", SHT_GNU_VERSYM, SHF_ALLOC as u64);
        hdr.shdr.sh_entsize.set(2);
        hdr.shdr.sh_addralign.set(2);
        Self { hdr, contents: Vec::new() }
    }
}

impl<E: Target> Default for VersymSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn update_shdr<E: Target>(ctx: &mut Context<E>) {
    let size = ctx.versym.contents.len() as u64 * 2;
    ctx.versym.hdr.shdr.sh_size.set(size);
    ctx.versym.hdr.shdr.sh_link.set(ctx.dynsym.hdr.shndx);
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    for (i, &v) in ctx.versym.contents.iter().enumerate() {
        E::write_u16(&mut buf[i * 2..], v);
    }
}
