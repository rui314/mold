//! Compressed output sections.

use crate::chunks::{self, ChunkHeader, ChunkId};
use crate::cmdline::DebugCompression;
use crate::context::Context;
use crate::elf::*;
use crate::target::Target;
use crate::util::compress::Compressor;

// Debug sections can be compressed with zlib or zstd to reduce the
// overall size of an ELF file. CompressedSection represents a compressed
// section.
#[derive(Debug)]
pub struct CompressedSection<E: Target> {
    pub hdr: ChunkHeader<E>,
    pub chdr: ElfChdr<E>,
    pub compressor: Compressor,
    /// Retained only for sections whose contents --gdb-index reads.
    pub uncompressed_data: Option<Vec<u8>>,
}

pub fn new<E: Target>(ctx: &Context<E>, original: ChunkId) -> CompressedSection<E> {
    let hdr = ctx.chunk_header(original);

    // C++ mold uses uninitialized storage here to avoid zero-filling a
    // potentially large scratch buffer. Rust currently uses a
    // zero-initialized Vec because write_to accepts &mut [u8].
    let mut buf = vec![0u8; hdr.shdr.sh_size.get() as usize];

    // Write uncompressed contents and then compress them
    chunks::write_to(ctx, original, &mut buf);

    let (kind, compressor) = match ctx.args.compress_debug_sections {
        DebugCompression::Zlib(level) => (ELFCOMPRESS_ZLIB, Compressor::zlib(&buf, level)),
        DebugCompression::Zstd(level) => (ELFCOMPRESS_ZSTD, Compressor::zstd(&buf, level)),
        DebugCompression::None => unreachable!("debug compression is disabled"),
    };

    // Compute header field values
    let mut chdr = ElfChdr::<E>::default();
    chdr.set_ch_type(kind);
    chdr.set_ch_size(hdr.shdr.sh_size.get());
    chdr.set_ch_addralign(hdr.shdr.sh_addralign.get());
    let flags = hdr.shdr.sh_flags.get() | SHF_COMPRESSED as u64;
    let size = (std::mem::size_of::<ElfChdr<E>>() + compressor.compressed_size()) as u64;
    let mut new_hdr = ChunkHeader::<E>::with_name(hdr.name, hdr.shdr.sh_type.get(), flags);
    new_hdr.shndx = hdr.shndx;
    new_hdr.shdr = hdr.shdr;
    new_hdr.shdr.sh_flags.set(flags);
    new_hdr.shdr.sh_addralign.set(1);
    new_hdr.shdr.sh_size.set(size);

    let keep_contents = ctx.args.gdb_index && crate::gdb_index::needs_section_contents(hdr.name);
    CompressedSection {
        hdr: new_hdr,
        chdr,
        compressor,
        uncompressed_data: keep_contents.then_some(buf),
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, i: u32, buf: &mut [u8]) {
    let sec = &ctx.compressed_sections[i as usize];
    sec.chdr.write(buf);
    sec.compressor.write_to(&mut buf[std::mem::size_of::<ElfChdr<E>>()..]);
}
