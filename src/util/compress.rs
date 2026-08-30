//! Multi-threaded zlib and zstd compression for `--compress-debug-sections`.
//!
//! zlib-compressed data can be merged by concatenation as long as each
//! piece ends with a sync flush, so the input is split into shards,
//! compressed in parallel, and concatenated under one header and trailer.
//! zstd frames concatenate the same way. Resetting the dictionary at shard
//! boundaries costs a little compression ratio, which is negligible with
//! large shards.

use flate2::{Compress, Compression, FlushCompress, FlushDecompress};
use rayon::prelude::*;

const SHARD_SIZE: usize = 1024 * 1024;

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
    let mut compress = Compress::new(Compression::new(level), false);
    let mut out = Vec::with_capacity(input.len() / 2 + 64);
    loop {
        let before = compress.total_in() as usize;
        let status = compress
            .compress_vec(&input[before..], &mut out, FlushCompress::Sync)
            .expect("deflate failed");
        let consumed = compress.total_in() as usize;
        if consumed == input.len() && status == flate2::Status::Ok && out.len() < out.capacity() {
            break;
        }
        if status == flate2::Status::StreamEnd {
            break;
        }
        out.reserve(out.capacity().max(1024));
    }
    out
}

impl Compressor {
    pub fn zlib(input: &[u8], level: u32) -> Compressor {
        let inputs: Vec<&[u8]> = input.chunks(SHARD_SIZE).collect();
        let (shards, adlers): (Vec<Vec<u8>>, Vec<u32>) = inputs
            .par_iter()
            .map(|shard| (zlib_compress(shard, level), adler32(shard)))
            .unzip();

        let mut checksum = adlers.first().copied().unwrap_or(1);
        for (adler, shard) in adlers.iter().zip(&inputs).skip(1) {
            checksum = adler32_combine(checksum, *adler, shard.len() as u64);
        }
        Compressor::Zlib { shards, checksum }
    }

    pub fn zstd(input: &[u8], level: i32) -> Compressor {
        let shards = input
            .par_chunks(SHARD_SIZE)
            .map(|shard| zstd::bulk::compress(shard, level).expect("zstd compression failed"))
            .collect();
        Compressor::Zstd { shards }
    }

    pub fn compressed_size(&self) -> usize {
        match self {
            // The header and the trailer add 8 bytes.
            Compressor::Zlib { shards, .. } => 8 + shards.iter().map(Vec::len).sum::<usize>(),
            Compressor::Zstd { shards } => shards.iter().map(Vec::len).sum(),
        }
    }

    pub fn write_to(&self, buf: &mut [u8]) {
        match self {
            Compressor::Zlib { shards, checksum } => {
                buf[0] = 0x78;
                buf[1] = 0x9c;
                let mut pos = 2;
                for shard in shards {
                    buf[pos..pos + shard.len()].copy_from_slice(shard);
                    pos += shard.len();
                }
                // An empty final block, then the Adler-32 checksum.
                buf[pos] = 3;
                buf[pos + 1] = 0;
                buf[pos + 2..pos + 6].copy_from_slice(&checksum.to_be_bytes());
            }
            Compressor::Zstd { shards } => {
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
