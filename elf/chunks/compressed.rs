//! Compressed output sections.

use crate::arch::Target;
use crate::chunks::{self, ChunkHeader, ChunkId};
use crate::context::Context;
use crate::elf::*;
use crate::util::compress::{CompressedData, Compressor};

// Debug sections can be compressed with zlib or zstd to reduce the
// overall size of an ELF file. CompressedSection represents a compressed
// section.
#[derive(Debug)]
pub struct CompressedSection<E: Target> {
    pub hdr: ChunkHeader<E>,
    pub chdr: ElfChdr<E>,
    pub data: CompressedData,
    /// Retained only for sections whose contents --gdb-index reads.
    pub uncompressed_data: Option<Vec<u8>>,
}

pub fn new<E: Target>(
    ctx: &Context<E>,
    original: ChunkId,
    compressor: &Compressor,
) -> CompressedSection<E> {
    let hdr = ctx.chunk_header(original);

    // C++ mold uses uninitialized storage here to avoid zero-filling a
    // potentially large scratch buffer. Rust currently uses a
    // zero-initialized Vec because write_to accepts &mut [u8].
    let mut buf = vec![0u8; hdr.shdr.sh_size.get() as usize];

    // Write uncompressed contents and then compress them
    chunks::write_to(ctx, original, &mut buf);

    let data = compressor.compress(&buf);

    // Compute header field values
    let mut chdr = ElfChdr::<E>::default();
    chdr.set_ch_type(match data {
        CompressedData::Zlib { .. } => ELFCOMPRESS_ZLIB,
        CompressedData::Zstd { .. } => ELFCOMPRESS_ZSTD,
    });
    chdr.set_ch_size(hdr.shdr.sh_size.get());
    chdr.set_ch_addralign(hdr.shdr.sh_addralign.get());
    let flags = hdr.shdr.sh_flags.get() | SHF_COMPRESSED as u64;
    let size = (size_of::<ElfChdr<E>>() + data.compressed_size()) as u64;
    let mut new_hdr = ChunkHeader::<E>::with_name(hdr.name, hdr.shdr.sh_type.get(), flags);
    new_hdr.shndx = hdr.shndx;
    new_hdr.shdr = hdr.shdr;
    new_hdr.shdr.sh_flags.set(flags);
    new_hdr.shdr.sh_addralign.set(1);
    new_hdr.shdr.sh_size.set(size);

    let keep_contents = ctx.args.gdb_index && crate::gdb_index::needs_section_contents(hdr.name);
    CompressedSection { hdr: new_hdr, chdr, data, uncompressed_data: keep_contents.then_some(buf) }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, i: u32, buf: &mut [u8]) {
    let sec = &ctx.compressed_sections[i as usize];
    sec.chdr.write(buf);
    sec.data.write_to(&mut buf[size_of::<ElfChdr<E>>()..]);
}
