//! `.gnu.hash`, the GNU hash table for dynamic symbol lookup.

use crate::arch::Arch;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::symbol::SymbolId;
use crate::util::endian::Endian;

/// The hash function for `.gnu.hash`.
#[inline]
pub fn djb_hash(name: &[u8]) -> u32 {
    let mut h: u32 = 5381;
    let mut chunks = name.chunks_exact(4);
    for p in &mut chunks {
        let a = p[0] as u32 * 33 + p[1] as u32;
        let b = p[2] as u32 * 33 + p[3] as u32;
        h = h.wrapping_mul(1185921).wrapping_add(a * 1089 + b);
    }
    for &c in chunks.remainder() {
        h = h.wrapping_mul(33).wrapping_add(c as u32);
    }
    h
}

// .gnu.hash is an alternative format for .hash. It contains not only an
// on-disk hash table but also contains a bloom filter to quickly identify
// whether or not a given symbol name exists in .dynsym.
#[derive(Debug)]
pub struct GnuHashSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
    pub num_buckets: u32,
    pub num_bloom: u32,
    pub num_exported: u32,
}

impl<E: Arch> GnuHashSection<E> {
    pub const LOAD_FACTOR: u32 = 8;
    pub const HEADER_SIZE: u64 = 16;
    pub const BLOOM_SHIFT: u32 = 26;

    pub fn new() -> GnuHashSection<E> {
        let mut hdr = ChunkHeader::<E>::new(".gnu.hash", SHT_GNU_HASH, SHF_ALLOC as u64);
        hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
        GnuHashSection {
            hdr,
            num_buckets: 0,
            num_bloom: 1,
            num_exported: 0,
        }
    }
}

impl<E: Arch> Default for GnuHashSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
    if ctx.dynsym.symbols.is_empty() {
        return;
    }
    let word = E::WORD_SIZE as u64;
    let gh = ctx.gnu_hash.as_mut().unwrap();
    // We allocate 12 bits for each symbol in the bloom filter.
    gh.num_bloom = ((gh.num_exported as u64 * 12) / (word * 8))
        .max(1)
        .next_power_of_two() as u32;
    gh.hdr.shdr.sh_size.set(
        GnuHashSection::<E>::HEADER_SIZE
            + gh.num_bloom as u64 * word // Bloom filter
            + gh.num_buckets as u64 * 4 // Hash buckets
            + gh.num_exported as u64 * 4, // Hash values
    );
    gh.hdr.shdr.sh_link.set(ctx.dynsym.hdr.shndx);
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    buf.fill(0);
    let gh = ctx.gnu_hash.as_ref().unwrap();
    let word = E::WORD_SIZE;
    let first_exported = ctx.dynsym.symbols.len() - gh.num_exported as usize;

    E::Endian::write_u32(buf, gh.num_buckets);
    E::Endian::write_u32(&mut buf[4..], first_exported as u32);
    E::Endian::write_u32(&mut buf[8..], gh.num_bloom);
    E::Endian::write_u32(&mut buf[12..], GnuHashSection::<E>::BLOOM_SHIFT);

    let syms: Vec<SymbolId> = ctx.dynsym.symbols[first_exported..]
        .iter()
        .flatten()
        .copied()
        .collect();
    if syms.is_empty() {
        return;
    }

    // Write a bloom filter
    let bloom_off = GnuHashSection::<E>::HEADER_SIZE as usize;
    let word_bits = word * 8;
    let mut indices = Vec::with_capacity(syms.len());
    for &id in &syms {
        let h = ctx.symbols[id].aux(&ctx.symbols).unwrap().djb_hash;
        indices.push(h % gh.num_buckets);
        let idx = (h as usize / word_bits) % gh.num_bloom as usize;
        let bits = (1u64 << (h as usize % word_bits))
            | (1u64 << ((h >> GnuHashSection::<E>::BLOOM_SHIFT) as usize % word_bits));
        let slot = &mut buf[bloom_off + idx * word..];
        if E::IS_64 {
            E::Endian::write_u64(slot, E::Endian::read_u64(slot) | bits);
        } else {
            E::Endian::write_u32(slot, E::Endian::read_u32(slot) | bits as u32);
        }
    }

    // Write hash bucket indices
    let buckets_off = bloom_off + gh.num_bloom as usize * word;
    for (i, &bucket) in indices.iter().enumerate().rev() {
        E::Endian::write_u32(
            &mut buf[buckets_off + bucket as usize * 4..],
            (first_exported + i) as u32,
        );
    }

    // Write a hash table
    let table_off = buckets_off + gh.num_buckets as usize * 4;
    for (i, &id) in syms.iter().enumerate() {
        // The last entry in a chain must be terminated with an entry with
        // least-significant bit 1.
        let h = ctx.symbols[id].aux(&ctx.symbols).unwrap().djb_hash;
        let last = i + 1 == syms.len() || indices[i] != indices[i + 1];
        E::Endian::write_u32(
            &mut buf[table_off + i * 4..],
            if last { h | 1 } else { h & !1 },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_bytewise_hash_including_overflow_and_tails() {
        let bytes: Vec<u8> = (0..1024).map(|i| (i * 173) as u8).collect();
        for len in 0..=bytes.len() {
            let expected = bytes[..len]
                .iter()
                .fold(5381u32, |h, &c| h.wrapping_mul(33).wrapping_add(c as u32));
            assert_eq!(djb_hash(&bytes[..len]), expected);
        }
    }
}
