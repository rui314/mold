//! The ad-hoc code signature: SHA-256 page hashes in a code directory, the
//! last chunk of the file.

use std::path::Path;

use rayon::prelude::*;

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::macho::*;
use crate::target::Target;
use crate::util::align_to;

/// The ad-hoc code signature. Must be the last chunk in the file.
#[derive(Debug)]
pub struct CodeSignatureSection {
    pub hdr: ChunkHeader,
}

impl CodeSignatureSection {
    pub fn new() -> Self {
        Self { hdr: ChunkHeader::linkedit() }
    }
}

impl Default for CodeSignatureSection {
    fn default() -> Self {
        Self::new()
    }
}

/// The length of the signature's superblob: its header and one blob
/// index, then the code directory - its fixed part, the NUL-terminated
/// identifier and the page hashes right after it, unpadded as ld-prime
/// writes them - for a signature placed at `fileoff`.
fn superblob_size(output: &Path, fileoff: u64) -> u64 {
    let nblocks = fileoff.div_ceil(CS_PAGE_SIZE);
    12 + 8 + 88 + file_basename(output).len() as u64 + 1 + nblocks * SHA256_SIZE as u64
}

/// Returns the size of the code signature given the file offset it will
/// be placed at: the superblob, zero-padded to 8 bytes.
pub fn size(output: &Path, fileoff: u64) -> u64 {
    align_to(superblob_size(output, fileoff), 8)
}

/// The signature's identifier: the output's leaf name, as bytes.
fn file_basename(path: &Path) -> &[u8] {
    path.file_name().map_or(&[][..], |name| std::os::unix::ffi::OsStrExt::as_bytes(name))
}

fn push_be32(buf: &mut Vec<u8>, val: u32) {
    buf.extend_from_slice(&val.to_be_bytes());
}

fn push_be64(buf: &mut Vec<u8>, val: u64) {
    buf.extend_from_slice(&val.to_be_bytes());
}

/// SHA256 of every code-signature page (4KiB) of `data`, computed in
/// parallel: the hashes are independent. The last page may be short.
/// These are the code directory's page hashes, and the UUID is derived
/// from them too.
pub fn page_hashes(data: &[u8]) -> Vec<[u8; SHA256_SIZE]> {
    let page = CS_PAGE_SIZE as usize;
    data.par_chunks(page)
        .map(|chunk| {
            let mut hash = [0; SHA256_SIZE];
            crate::util::sha256(chunk, &mut hash);
            hash
        })
        .collect()
}

/// Recomputes the hashes of the pages that overlap `data[range]`, after
/// those bytes changed.
pub fn rehash_pages(data: &[u8], hashes: &mut [[u8; SHA256_SIZE]], range: std::ops::Range<usize>) {
    let page = CS_PAGE_SIZE as usize;
    let first = range.start / page;
    let last = range.end.div_ceil(page).min(hashes.len());
    for (i, hash) in hashes[first..last].iter_mut().enumerate() {
        let start = (first + i) * page;
        let end = (start + page).min(data.len());
        crate::util::sha256(&data[start..end], hash);
    }
}

/// Writes the ad-hoc code signature at the code signature chunk's
/// offset, from the page hashes of the file contents before it
/// (page_hashes, brought up to date after the header's last change).
///
/// On ARM64 macOS a code signature is mandatory: the kernel refuses to
/// run an executable without one. The signature we create is just SHA256
/// hashes of every page, marked ad-hoc and linker-signed; no signing
/// identity is involved.
pub fn write<E: Target>(ctx: &Context<E>, buf: &mut [u8], hashes: &[[u8; SHA256_SIZE]]) {
    let cs_off = ctx.code_signature.hdr.fileoff;
    let ident = file_basename(&ctx.args.output);
    let ident_size = ident.len() as u64 + 1;
    let nblocks = cs_off.div_ceil(CS_PAGE_SIZE);
    let cd_size = 88 + ident_size + nblocks * SHA256_SIZE as u64;

    // (__TEXT, but for a -static image's renamed by -rename_segment.)
    let text = ctx.segments.iter().find(|s| s.name == ctx.mach_header.hdr.segname).unwrap();
    // The executable segment's limit, as ld-prime sets it: the size of
    // __TEXT,__text (0 without one), not of the whole segment.
    let text_size = ctx
        .chunks
        .iter()
        .map(|&id| ctx.chunk_header(id))
        .find(|hdr| hdr.segname == "__TEXT" && hdr.sectname == "__text")
        .map_or(0, |hdr| hdr.size);

    // All code signature fields are big-endian.
    let mut sig = Vec::with_capacity(ctx.code_signature.hdr.size as usize);

    // The superblob header and the index of its single blob, the code
    // directory.
    push_be32(&mut sig, CSMAGIC_EMBEDDED_SIGNATURE);
    push_be32(&mut sig, superblob_size(&ctx.args.output, cs_off) as u32);
    push_be32(&mut sig, 1);
    push_be32(&mut sig, CSSLOT_CODEDIRECTORY);
    push_be32(&mut sig, 20);

    // The code directory.
    push_be32(&mut sig, CSMAGIC_CODEDIRECTORY);
    push_be32(&mut sig, cd_size as u32);
    push_be32(&mut sig, CS_SUPPORTSEXECSEG); // version
    push_be32(&mut sig, CS_ADHOC | CS_LINKER_SIGNED); // flags
    push_be32(&mut sig, (88 + ident_size) as u32); // hash offset
    push_be32(&mut sig, 88); // identifier offset
    push_be32(&mut sig, 0); // special slots
    push_be32(&mut sig, nblocks as u32); // code slots
    push_be32(&mut sig, cs_off as u32); // code limit
    sig.push(SHA256_SIZE as u8);
    sig.push(CS_HASHTYPE_SHA256);
    sig.push(0); // platform
    sig.push(CS_PAGE_SIZE.trailing_zeros() as u8);
    push_be32(&mut sig, 0); // spare2
    push_be32(&mut sig, 0); // scatter offset
    push_be32(&mut sig, 0); // team offset
    push_be32(&mut sig, 0); // spare3
    push_be64(&mut sig, 0); // code limit 64
    push_be64(&mut sig, text.cmd.fileoff); // exec segment base
    push_be64(&mut sig, text_size); // exec segment limit
    let exec_seg_flags =
        if ctx.args.output_type == MH_EXECUTE { CS_EXECSEG_MAIN_BINARY } else { 0 };
    push_be64(&mut sig, exec_seg_flags); // exec segment flags

    sig.extend_from_slice(ident);
    sig.push(0);

    debug_assert_eq!(hashes.len() as u64, nblocks);
    for hash in hashes {
        sig.extend_from_slice(hash);
    }
    // The chunk is 8-byte aligned in size; the superblob's length
    // above leaves the padding out.
    sig.resize(ctx.code_signature.hdr.size as usize, 0);

    debug_assert_eq!(sig.len() as u64, ctx.code_signature.hdr.size);
    buf[cs_off as usize..cs_off as usize + sig.len()].copy_from_slice(&sig);
}
