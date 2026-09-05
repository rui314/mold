//! Compressed output sections.

use crate::arch::Arch;
use crate::chunks::{self, ChunkHeader, ChunkId};
use crate::context::Context;
use crate::elf::*;
use crate::util::compress::Compressor;

// Debug sections can be compressed with zlib or zstd to reduce the
// overall size of an ELF file. CompressedSection represents a compressed
// section.
#[derive(Debug)]
pub struct CompressedSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
    pub chdr: ElfChdr<E>,
    pub compressor: Compressor,
    /// Kept for --gdb-index, which reads the uncompressed contents.
    pub uncompressed_data: Option<Vec<u8>>,
    pub original: ChunkId,
}

impl std::fmt::Debug for Compressor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Compressor({} bytes)", self.compressed_size())
    }
}

pub fn new<E: Arch>(ctx: &Context<E>, original: ChunkId) -> CompressedSection<E> {
    let hdr = ctx.chunk_header(original);

    // C++ mold uses uninitialized storage here to avoid zero-filling a
    // potentially large scratch buffer. Rust currently uses a
    // zero-initialized Vec because write_to accepts &mut [u8].
    let mut buf = vec![0u8; hdr.shdr.sh_size.get() as usize];

    // Write uncompressed contents and then compress them
    chunks::write_to(ctx, original, &mut buf);

    let level = ctx.args.compress_debug_sections_level;
    let compressor = if ctx.args.compress_debug_sections == ELFCOMPRESS_ZLIB {
        Compressor::zlib(&buf, level as u32)
    } else {
        Compressor::zstd(&buf, level as i32)
    };

    // Compute header field values
    let mut chdr = ElfChdr::<E>::default();
    chdr.ch_type_mut().set(ctx.args.compress_debug_sections);
    chdr.ch_size_mut().set(hdr.shdr.sh_size.get());
    chdr.ch_addralign_mut().set(hdr.shdr.sh_addralign.get());
    let flags = hdr.shdr.sh_flags.get() | SHF_COMPRESSED as u64;
    let size = (std::mem::size_of::<ElfChdr<E>>() + compressor.compressed_size()) as u64;
    let mut new_hdr = ChunkHeader::<E>::with_name(hdr.name, hdr.shdr.sh_type.get(), flags);
    new_hdr.shndx = hdr.shndx;
    new_hdr.is_compressed = true;
    new_hdr.shdr = hdr.shdr;
    new_hdr.shdr.sh_flags.set(flags);
    new_hdr.shdr.sh_addralign.set(1);
    new_hdr.shdr.sh_size.set(size);

    // We can discard the uncompressed contents unless --gdb-index is given
    CompressedSection {
        hdr: new_hdr,
        chdr,
        compressor,
        uncompressed_data: ctx.args.gdb_index.then_some(buf),
        original,
    }
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, i: u32, buf: &mut [u8]) {
    let sec = &ctx.compressed_sections[i as usize];
    sec.chdr.write(buf);
    sec.compressor
        .write_to(&mut buf[std::mem::size_of::<ElfChdr<E>>()..]);
}
