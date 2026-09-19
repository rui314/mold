//! The smaller synthetic chunks: -sectcreate sections, __init_offsets,
//! and the LC_FUNCTION_STARTS, LC_DATA_IN_CODE and LC_CODE_SIGNATURE
//! tables in __LINKEDIT.

use crate::macho::arch::Arch;
use crate::macho::context::Context;
use crate::macho::format::*;
use crate::macho::input_files::FileId;
use crate::macho::output_chunks::ChunkHeader;
use crate::util::{align_to, encode_uleb};

/// A section created from a file by -sectcreate, or an empty one for
/// -add_empty_section and for a section only a boundary symbol names.
#[derive(Debug)]
pub struct SectCreateSection {
    pub hdr: ChunkHeader,
    pub contents: &'static [u8],
}

impl SectCreateSection {
    pub fn new(
        segname: &'static str,
        sectname: &str,
        contents: &'static [u8],
    ) -> SectCreateSection {
        let mut hdr = ChunkHeader::new(segname, sectname);
        hdr.size = contents.len() as u64;
        SectCreateSection { hdr, contents }
    }
}

pub mod sectcreate {
    use super::*;

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, idx: u32, buf: &mut [u8]) {
        let data = ctx.sectcreate_sections[idx as usize].contents;
        buf[..data.len()].copy_from_slice(data);
    }
}

/// __TEXT,__init_offsets: 32-bit image-relative initializer offsets,
/// replacing __mod_init_func's absolute pointers.
#[derive(Debug)]
pub struct InitOffsetsSection {
    pub hdr: ChunkHeader,
    /// Initializer targets in run order: the subsection and offset of
    /// each initializer function.
    pub init_funcs: Vec<(usize, u64)>,
}

impl InitOffsetsSection {
    pub fn new() -> InitOffsetsSection {
        let mut hdr = ChunkHeader::new("__TEXT", "__init_offsets");
        hdr.flags = S_INIT_FUNC_OFFSETS;
        hdr.p2align = 2;
        InitOffsetsSection { hdr, init_funcs: Vec::new() }
    }
}

pub mod init_offsets {
    use super::*;

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        for (i, &(isec, off)) in ctx.init_offsets.init_funcs.iter().enumerate() {
            let val = (ctx.isec_addr(isec) + off - ctx.args.pagezero_size) as u32;
            buf[i * 4..i * 4 + 4].copy_from_slice(&val.to_le_bytes());
        }
    }
}

/// LC_FUNCTION_STARTS data: delta-encoded function addresses, used by
/// debuggers and crash reporters.
#[derive(Debug)]
pub struct FunctionStartsSection {
    pub hdr: ChunkHeader,
    /// The encoded table, built during layout.
    pub contents: Vec<u8>,
}

impl FunctionStartsSection {
    pub fn new() -> FunctionStartsSection {
        FunctionStartsSection { hdr: ChunkHeader::linkedit(), contents: Vec::new() }
    }
}

pub mod function_starts {
    use super::*;

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        let data = &ctx.function_starts.contents;
        buf[..data.len()].copy_from_slice(data);
    }
}

/// LC_DATA_IN_CODE: ranges inside __text that hold data (jump tables,
/// inline constants), so disassemblers and the signature verifier can
/// treat them as bytes.
#[derive(Debug)]
pub struct DataInCodeSection {
    pub hdr: ChunkHeader,
    /// The entries (fileoff, length, kind), built once when layout
    /// reaches __LINKEDIT.
    pub entries: Vec<(u32, u16, u16)>,
}

impl DataInCodeSection {
    pub fn new() -> DataInCodeSection {
        DataInCodeSection { hdr: ChunkHeader::linkedit(), entries: Vec::new() }
    }
}

pub mod data_in_code {
    use super::*;

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        let mut p = 0;
        for &(off, len, kind) in &ctx.data_in_code.entries {
            buf[p..p + 4].copy_from_slice(&off.to_le_bytes());
            buf[p + 4..p + 6].copy_from_slice(&len.to_le_bytes());
            buf[p + 6..p + 8].copy_from_slice(&kind.to_le_bytes());
            p += 8;
        }
    }
}

/// The ad-hoc code signature. Must be the last chunk in the file.
#[derive(Debug)]
pub struct CodeSignatureSection {
    pub hdr: ChunkHeader,
}

impl CodeSignatureSection {
    pub fn new() -> CodeSignatureSection {
        CodeSignatureSection { hdr: ChunkHeader::linkedit() }
    }
}

/// Returns the size of the code signature given the file offset it will
/// be placed at.
pub fn code_signature_size(output: &str, fileoff: u64) -> u64 {
    let ident_size = align_to(file_basename(output).len() as u64 + 1, 16);
    let nblocks = fileoff.div_ceil(CS_PAGE_SIZE);
    // Superblob header, one blob index, the code directory, the
    // identifier and the page hashes.
    12 + 8 + 88 + ident_size + nblocks * SHA256_SIZE as u64
}

fn file_basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap()
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
    use rayon::prelude::*;
    let page = CS_PAGE_SIZE as usize;
    data.par_chunks(page)
        .map(|chunk| {
            let mut hash = [0; SHA256_SIZE];
            crate::macho::util::sha256(chunk, &mut hash);
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
    for i in first..last {
        let start = i * page;
        let end = (start + page).min(data.len());
        crate::macho::util::sha256(&data[start..end], &mut hashes[i]);
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
pub fn write_code_signature<E: Arch>(
    ctx: &Context<E>,
    buf: &mut [u8],
    hashes: &[[u8; SHA256_SIZE]],
) {
    let cs_off = ctx.code_signature.hdr.fileoff;
    let ident = file_basename(&ctx.args.output);
    let ident_size = align_to(ident.len() as u64 + 1, 16);
    let nblocks = cs_off.div_ceil(CS_PAGE_SIZE);
    let cd_size = 88 + ident_size + nblocks * SHA256_SIZE as u64;

    let text = ctx.segments.iter().find(|s| s.name == "__TEXT").unwrap();

    // All code signature fields are big-endian.
    let mut sig = Vec::with_capacity(ctx.code_signature.hdr.size as usize);

    // The superblob header and the index of its single blob, the code
    // directory.
    push_be32(&mut sig, CSMAGIC_EMBEDDED_SIGNATURE);
    push_be32(&mut sig, ctx.code_signature.hdr.size as u32);
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
    push_be64(&mut sig, text.cmd.filesize); // exec segment limit
    let exec_seg_flags =
        if ctx.args.output_type == MH_EXECUTE { CS_EXECSEG_MAIN_BINARY } else { 0 };
    push_be64(&mut sig, exec_seg_flags); // exec segment flags

    sig.extend_from_slice(ident.as_bytes());
    sig.resize(sig.len() + ident_size as usize - ident.len(), 0);

    debug_assert_eq!(hashes.len() as u64, nblocks);
    for hash in hashes {
        sig.extend_from_slice(hash);
    }

    debug_assert_eq!(sig.len() as u64, ctx.code_signature.hdr.size);
    buf[cs_off as usize..cs_off as usize + sig.len()].copy_from_slice(&sig);
}

/// Live data-in-code entries as (subsection, offset within it,
/// length, kind). In an object, an entry's offset is an address in
/// the object's own address space (sections there are laid out from
/// zero), which find_subsec maps to the owning subsection - entries
/// whose subsection was dead-stripped vanish with it.
/// Builds the LC_DATA_IN_CODE entries. Runs when layout reaches
/// __LINKEDIT: the __text file offsets the entries record are final by
/// then, so the table is built exactly once (sold builds its contents
/// in compute_size the same way) and copied out verbatim.
pub fn build_data_in_code<E: Arch>(ctx: &Context<E>) -> Vec<(u32, u16, u16)> {
    let mut out: Vec<(u32, u16, u16)> = Vec::new();
    for obj in &ctx.objs {
        if !obj.is_alive {
            continue;
        }
        for &(off, len, kind) in &obj.dice {
            let Some((isec, off_in)) =
                crate::macho::input_files::find_subsec(&ctx.isecs, &obj.subsecs, off as u64)
            else {
                continue;
            };
            let isec = &ctx.isecs[ctx.resolve_isec(isec as usize)];
            if isec.is_alive() {
                let fileoff = ctx.chunk_header(isec.output_section().unwrap()).fileoff
                    + isec.offset as u64
                    + off_in;
                out.push((fileoff as u32, len, kind));
            }
        }
    }
    out.sort_unstable();
    out
}

pub fn build_function_starts<E: Arch>(ctx: &Context<E>) -> Vec<u8> {
    if !ctx.args.function_starts {
        return Vec::new();
    }
    use rayon::prelude::*;
    let mut addrs: Vec<u64> = ctx
        .symbols
        .syms
        .par_iter()
        .filter_map(|sym| {
            if !matches!(sym.file(), Some(FileId::Obj(_))) {
                return None;
            }
            let isec = &ctx.isecs[ctx.resolve_isec(sym.input_section()? as usize)];
            if isec.is_alive()
                && ctx.hdr_of(isec).segname() == "__TEXT"
                && ctx.hdr_of(isec).sectname() == "__text"
            {
                Some(
                    ctx.chunk_header(isec.output_section().unwrap()).addr
                        + isec.offset as u64
                        + sym.value,
                )
            } else {
                None
            }
        })
        .collect();
    if addrs.is_empty() {
        return Vec::new();
    }
    addrs.par_sort_unstable();
    addrs.dedup();

    let mut buf = Vec::new();
    let mut last = ctx.args.pagezero_size;
    for addr in addrs {
        encode_uleb(&mut buf, addr - last);
        last = addr;
    }
    buf.push(0);
    while buf.len() % 8 != 0 {
        buf.push(0);
    }
    buf
}
