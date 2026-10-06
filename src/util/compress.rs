//! This file implements a multi-threaded zlib and zstd compression
//! routine.
//!
//! zlib-compressed data can be merged just by concatenation as long as
//! each piece of data is flushed with Z_SYNC_FLUSH. In this file, we
//! split input data into multiple shards, compress them individually
//! and concatenate them. We then append a header, a trailer and a
//! checksum so that the concatenated data is valid zlib-format data.
//!
//! zstd-compressed data can be merged in the same way.
//!
//! Using threads to compress data has a downside. Since the dictionary
//! is reset on boundaries of shards, compression ratio is sacrificed
//! a little bit. However, if a shard size is large enough, that loss
//! is negligible in practice.

use std::io::{ErrorKind, Read};

use flate2::FlushDecompress;
use rayon::prelude::*;
use zlib_rs::adler32::{adler32, adler32_combine};

use crate::util::worker_local::WorkerLocal;

const SHARD_SIZE: usize = 1024 * 1024;

/// Compresses data with zlib or zstd at a fixed level, using all threads.
pub struct Compressor(Method);

enum Method {
    Zlib(u32),
    Zstd(i32, ZstdContexts),
}

// Creating a zstd context costs nearly as much as compressing a shard at
// higher levels, so each Rayon worker reuses one for all shards.
type ZstdContexts = WorkerLocal<Option<zstd::bulk::Compressor<'static>>>;

/// Compressed data, kept as shards until it is written to the output.
pub enum CompressedData {
    Zlib { shards: Vec<Vec<u8>>, checksum: u32 },
    Zstd { shards: Vec<Vec<u8>> },
}

impl std::fmt::Debug for CompressedData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CompressedData({} bytes)", self.compressed_size())
    }
}

/// Compresses a shard as a raw deflate stream ending with a sync flush,
/// so that the stream ends on a byte boundary and can be concatenated.
fn zlib_compress_shard(input: &[u8], level: u32) -> Vec<u8> {
    // Initialize zlib stream. Since debug info is generally compressed
    // pretty well with lower compression levels, the default level is 1.
    let mut stream = libz_rs_sys::z_stream::default();
    // SAFETY: stream is a valid z_stream with the default allocator. It
    // stays at the same address until deflateEnd.
    let status = unsafe {
        libz_rs_sys::deflateInit2_(
            &mut stream,
            level as i32,
            libz_rs_sys::Z_DEFLATED,
            -15,
            8,
            libz_rs_sys::Z_DEFAULT_STRATEGY,
            libz_rs_sys::zlibVersion(),
            size_of::<libz_rs_sys::z_stream>() as i32,
        )
    };
    assert_eq!(status, libz_rs_sys::Z_OK);

    // Set an input buffer
    stream.avail_in = input.len() as u32;
    stream.next_in = input.as_ptr();

    // Set an output buffer. deflateBound() returns an upper bound
    // on the compression size. +16 for Z_SYNC_FLUSH.
    // SAFETY: stream is initialized and remains valid through deflateEnd.
    let bound = unsafe { libz_rs_sys::deflateBound(&mut stream, stream.avail_in.into()) } as usize;
    let mut out = vec![0; bound + 16];

    // Compress data. It writes all compressed bytes except the last
    // partial byte, so up to 7 bits can be held to be written to the
    // buffer.
    stream.avail_out = out.len() as u32;
    stream.next_out = out.as_mut_ptr();
    // SAFETY: the input and output buffers remain alive and the output has
    // deflateBound() + 16 bytes of space.
    let status = unsafe { libz_rs_sys::deflate(&mut stream, libz_rs_sys::Z_BLOCK) };
    assert_eq!(status, libz_rs_sys::Z_OK);

    // This is a workaround for libbacktrace before 2022-04-06.
    //
    // Zlib is a bit stream, and what Z_SYNC_FLUSH does is to write a
    // three-bit value indicating the start of an uncompressed data
    // block followed by four bytes 00 00 ff ff which indicate that
    // the length of the block is zero. libbacktrace uses its own zlib
    // inflate routine, and it had a bug that if that particular
    // three-bit value happens to end at a byte boundary, it accidentally
    // skipped the next byte.
    //
    // In order to avoid triggering that bug, we should avoid calling
    // deflate() with Z_SYNC_FLUSH if the current bit position is 5.
    // If it's 5, we insert an empty block consisting of 10 bits so
    // that the bit position is 7 in the next byte.
    //
    // https://github.com/ianlancetaylor/libbacktrace/pull/87
    let mut nbits = 0;
    // SAFETY: stream is initialized and nbits is a valid output pointer.
    let status =
        unsafe { libz_rs_sys::deflatePending(&mut stream, std::ptr::null_mut(), &raw mut nbits) };
    assert_eq!(status, libz_rs_sys::Z_OK);
    if nbits == 5 {
        // SAFETY: stream is initialized and has enough pending-buffer space.
        let status = unsafe { libz_rs_sys::deflatePrime(&mut stream, 10, 2) };
        assert_eq!(status, libz_rs_sys::Z_OK);
    }
    // SAFETY: stream and its input and output buffers remain valid.
    let status = unsafe { libz_rs_sys::deflate(&mut stream, libz_rs_sys::Z_SYNC_FLUSH) };
    assert_eq!(status, libz_rs_sys::Z_OK);

    let len = out.len() - stream.avail_out as usize;
    // SAFETY: stream was initialized successfully and is no longer used.
    unsafe { libz_rs_sys::deflateEnd(&mut stream) };
    out.truncate(len);
    out
}

impl Compressor {
    pub fn zlib(level: u32) -> Self {
        Self(Method::Zlib(level))
    }

    pub fn zstd(level: i32) -> Self {
        Self(Method::Zstd(level, WorkerLocal::new(|| None)))
    }

    pub fn compress(&self, input: &[u8]) -> CompressedData {
        match &self.0 {
            Method::Zlib(level) => zlib_compress(input, *level),
            Method::Zstd(level, contexts) => zstd_compress(input, *level, contexts),
        }
    }
}

fn zlib_compress(input: &[u8], level: u32) -> CompressedData {
    // Compress each shard
    let (shards, adlers): (Vec<Vec<u8>>, Vec<u32>) = input
        .par_chunks(SHARD_SIZE)
        .map(|shard| (zlib_compress_shard(shard, level), adler32(1, shard)))
        .unzip();

    // Combine checksums
    let mut checksum = adlers.first().copied().unwrap_or(1);
    for (adler, shard) in adlers.iter().zip(input.chunks(SHARD_SIZE)).skip(1) {
        checksum = adler32_combine(checksum, *adler, shard.len() as u64);
    }
    CompressedData::Zlib { shards, checksum }
}

fn zstd_compress(input: &[u8], level: i32, contexts: &ZstdContexts) -> CompressedData {
    // Compress each shard
    let shards = input
        .par_chunks(SHARD_SIZE)
        .map(|shard| {
            let mut slot = contexts.get();
            let cctx = slot.get_or_insert_with(|| {
                zstd::bulk::Compressor::new(level).expect("cannot create a zstd context")
            });
            cctx.compress(shard).expect("zstd compression failed")
        })
        .collect();
    CompressedData::Zstd { shards }
}

impl CompressedData {
    pub fn compressed_size(&self) -> usize {
        // Compute the total size
        match self {
            // the header and the trailer
            Self::Zlib { shards, .. } => 8 + shards.iter().map(Vec::len).sum::<usize>(),
            Self::Zstd { shards } => shards.iter().map(Vec::len).sum(),
        }
    }

    pub fn write_to(&self, buf: &mut [u8]) {
        match self {
            Self::Zlib { shards, checksum } => {
                // Write a zlib-format header
                buf[0] = 0x78;
                buf[1] = 0x9c;

                // Copy compressed data
                // +2 for the header
                let mut pos = 2;
                for shard in shards {
                    buf[pos..pos + shard.len()].copy_from_slice(shard);
                    pos += shard.len();
                }
                // Write a trailer
                // An empty final block, then the Adler-32 checksum.
                buf[pos] = 3;
                buf[pos + 1] = 0;
                buf[pos + 2..pos + 6].copy_from_slice(&checksum.to_be_bytes());
            }
            Self::Zstd { shards } => {
                // Copy compressed data
                let mut pos = 0;
                for shard in shards {
                    buf[pos..pos + shard.len()].copy_from_slice(shard);
                    pos += shard.len();
                }
            }
        }
    }
}

/// Decompresses a zlib stream into a buffer of known size.
pub fn zlib_decompress(input: &[u8], out: &mut [u8]) -> Result<(), String> {
    let mut decompress = flate2::Decompress::new(true);
    loop {
        let in_pos = decompress.total_in() as usize;
        let out_pos = decompress.total_out() as usize;
        if out_pos >= out.len() {
            return Ok(());
        }
        let status = decompress
            .decompress(
                &input[in_pos.min(input.len())..],
                &mut out[out_pos..],
                FlushDecompress::None,
            )
            .map_err(|e| e.to_string())?;
        match status {
            flate2::Status::StreamEnd => break,
            flate2::Status::BufError if decompress.total_in() as usize >= input.len() => break,
            _ => {}
        }
    }
    if (decompress.total_out() as usize) < out.len() {
        return Err("premature end of input".to_string());
    }
    Ok(())
}

/// Decompresses a zstd stream, stopping when the requested output is full.
/// DWARF classification uses this to read just the unit header.
pub fn zstd_decompress(input: &[u8], out: &mut [u8]) -> Result<(), String> {
    let mut decoder = zstd::stream::read::Decoder::with_buffer(input).map_err(|e| e.to_string())?;
    decoder.read_exact(out).map_err(|e| {
        if e.kind() == ErrorKind::UnexpectedEof {
            "premature end of input".to_string()
        } else {
            e.to_string()
        }
    })
}
