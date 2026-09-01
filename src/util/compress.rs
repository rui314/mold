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

use std::mem::MaybeUninit;

use flate2::FlushDecompress;
use rayon::prelude::*;

const SHARD_SIZE: usize = 1024 * 1024;

// libz-sys exposes the zlib 1.2.3.4 API, but deflatePending was added in
// zlib 1.2.5.1. C++ mold requires and calls this function directly too.
unsafe extern "C" {
    #[link_name = "deflatePending"]
    fn deflate_pending(
        stream: libz_sys::z_streamp,
        pending: *mut std::ffi::c_uint,
        bits: *mut std::ffi::c_int,
    ) -> std::ffi::c_int;
}

pub enum Compressor {
    Zlib { shards: Vec<Vec<u8>>, checksum: u32 },
    Zstd { shards: Vec<Vec<u8>> },
}

fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for chunk in data.chunks(5552) {
        for &byte in chunk {
            a += byte as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

/// Combines two Adler-32 checksums, where `len2` is the length of the
/// second input.
fn adler32_combine(adler1: u32, adler2: u32, len2: u64) -> u32 {
    const BASE: u64 = 65521;
    let rem = len2 % BASE;
    let mut sum1 = (adler1 & 0xffff) as u64;
    let mut sum2 = rem * sum1 % BASE;
    sum1 += (adler2 & 0xffff) as u64 + BASE - 1;
    sum2 += ((adler1 >> 16) & 0xffff) as u64 + ((adler2 >> 16) & 0xffff) as u64 + BASE - rem;
    if sum1 >= BASE {
        sum1 -= BASE;
    }
    if sum1 >= BASE {
        sum1 -= BASE;
    }
    if sum2 >= BASE << 1 {
        sum2 -= BASE << 1;
    }
    if sum2 >= BASE {
        sum2 -= BASE;
    }
    (sum1 | (sum2 << 16)) as u32
}

/// Compresses a shard as a raw deflate stream ending with a sync flush,
/// so that the stream ends on a byte boundary and can be concatenated.
fn zlib_compress(input: &[u8], level: u32) -> Vec<u8> {
    // Initialize zlib stream. Since debug info is generally compressed
    // pretty well with lower compression levels, the default level is 1.
    let mut stream = MaybeUninit::<libz_sys::z_stream>::zeroed();
    // SAFETY: deflateInit2_ initializes the zeroed stream using its default
    // allocator. The stream remains at a stable address until deflateEnd.
    let status = unsafe {
        libz_sys::deflateInit2_(
            stream.as_mut_ptr(),
            level as i32,
            libz_sys::Z_DEFLATED,
            -15,
            8,
            libz_sys::Z_DEFAULT_STRATEGY,
            libz_sys::zlibVersion(),
            std::mem::size_of::<libz_sys::z_stream>() as i32,
        )
    };
    assert_eq!(status, libz_sys::Z_OK);
    // SAFETY: deflateInit2_ returned Z_OK, so the stream is initialized.
    let stream = unsafe { stream.assume_init_mut() };

    // Set an input buffer
    stream.avail_in = input.len() as u32;
    stream.next_in = input.as_ptr().cast_mut();

    // Set an output buffer. deflateBound() returns an upper bound
    // on the compression size. +16 for Z_SYNC_FLUSH.
    // SAFETY: stream is initialized and remains valid through deflateEnd.
    let bound = unsafe { libz_sys::deflateBound(stream, stream.avail_in.into()) } as usize;
    let mut out = vec![0; bound + 16];

    // Compress data. It writes all compressed bytes except the last
    // partial byte, so up to 7 bits can be held to be written to the
    // buffer.
    stream.avail_out = out.len() as u32;
    stream.next_out = out.as_mut_ptr();
    // SAFETY: the input and output buffers remain alive and the output has
    // deflateBound() + 16 bytes of space.
    let status = unsafe { libz_sys::deflate(stream, libz_sys::Z_BLOCK) };
    assert_eq!(status, libz_sys::Z_OK);

    // This is a workaround for libbacktrace before 2022-04-06.
    //
    // Zlib is a bit stream, and what Z_SYNC_FLUSH does is to write a
    // three bit value indicating the start of an uncompressed data
    // block followed by four byte data 00 00 ff ff which indicates that
    // the length of the block is zero. libbacktrace uses its own zlib
    // inflate routine, and it had a bug that if that particular three
    // bit value happens to end at a byte boundary, it accidentally
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
    let status = unsafe { deflate_pending(stream, std::ptr::null_mut(), &mut nbits) };
    assert_eq!(status, libz_sys::Z_OK);
    if nbits == 5 {
        // SAFETY: stream is initialized and has enough pending-buffer space.
        let status = unsafe { libz_sys::deflatePrime(stream, 10, 2) };
        assert_eq!(status, libz_sys::Z_OK);
    }
    // SAFETY: stream and its input and output buffers remain valid.
    let status = unsafe { libz_sys::deflate(stream, libz_sys::Z_SYNC_FLUSH) };
    assert_eq!(status, libz_sys::Z_OK);

    let len = out.len() - stream.avail_out as usize;
    // SAFETY: stream was initialized successfully and is no longer used.
    unsafe { libz_sys::deflateEnd(stream) };
    out.truncate(len);
    out
}

impl Compressor {
    pub fn zlib(input: &[u8], level: u32) -> Compressor {
        let inputs: Vec<&[u8]> = input.chunks(SHARD_SIZE).collect();

        // Compress each shard
        let (shards, adlers): (Vec<Vec<u8>>, Vec<u32>) = inputs
            .par_iter()
            .map(|shard| (zlib_compress(shard, level), adler32(shard)))
            .unzip();

        // Combine checksums
        let mut checksum = adlers.first().copied().unwrap_or(1);
        for (adler, shard) in adlers.iter().zip(&inputs).skip(1) {
            checksum = adler32_combine(checksum, *adler, shard.len() as u64);
        }
        Compressor::Zlib { shards, checksum }
    }

    pub fn zstd(input: &[u8], level: i32) -> Compressor {
        // Compress each shard
        let shards = input
            .par_chunks(SHARD_SIZE)
            .map(|shard| zstd::bulk::compress(shard, level).expect("zstd compression failed"))
            .collect();
        Compressor::Zstd { shards }
    }

    pub fn compressed_size(&self) -> usize {
        // Comput the total size
        match self {
            // the header and the trailer
            Compressor::Zlib { shards, .. } => 8 + shards.iter().map(Vec::len).sum::<usize>(),
            Compressor::Zstd { shards } => shards.iter().map(Vec::len).sum(),
        }
    }

    pub fn write_to(&self, buf: &mut [u8]) {
        match self {
            Compressor::Zlib { shards, checksum } => {
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
            Compressor::Zstd { shards } => {
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

/// Decompresses a zstd stream into a buffer of known size.
pub fn zstd_decompress(input: &[u8], out: &mut [u8]) -> Result<(), String> {
    let n = zstd::bulk::decompress_to_buffer(input, out).map_err(|e| e.to_string())?;
    if n < out.len() {
        return Err("premature end of input".to_string());
    }
    Ok(())
}
