//! `.hash`, the ELF hash table for dynamic symbol lookup.

use crate::arch::{Arch, Family};
use crate::context::Context;
use crate::elf::*;
use crate::output_chunks::ChunkHeader;

// The hash function for .hash.
pub fn elf_hash(name: &[u8]) -> u32 {
    let mut h: u32 = 0;
    for &c in name {
        h = (h << 4).wrapping_add(c as u32);
        let g = h & 0xf000_0000;
        if g != 0 {
            h ^= g >> 24;
        }
        h &= !g;
    }
    h
}

// .hash contains an on-disk hash table for .dynsym so that the runtime
// can look up a symbol name quickly without scannin all entries in
// .dynsym.
//
// Quickly identifying whether or not a .dynsym contains a given symbol is
// especially important for ELF because of the dynamic symbol lookup rule
// for ELF. In ELF, each dynamic symbol is not searched from a specific
// library but from all the ELF files loaded to memory. Therefore,
// minimizing the cost of each dynamic symbol lookup is important.
#[derive(Debug)]
pub struct HashSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
}

impl<E: Arch> HashSection<E> {
    pub fn new() -> HashSection<E> {
        let mut hdr = ChunkHeader::<E>::new(".hash", SHT_HASH, SHF_ALLOC as u64);
        // Even though u32 should suffice as an etnry size for all targets,
        // s390x uses u64. It looks like a spec bug, but we need to follow
        // suit for the sake of binary compatibility.
        let entry = entry_size::<E>() as u64;
        hdr.shdr.sh_entsize.set(entry);
        hdr.shdr.sh_addralign.set(entry);
        HashSection { hdr }
    }
}

impl<E: Arch> Default for HashSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn entry_size<E: Arch>() -> usize {
    if E::FAMILY == Family::S390x {
        8
    } else {
        4
    }
}

pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
    if ctx.dynsym.symbols.is_empty() {
        return;
    }
    let entry = entry_size::<E>() as u64;
    let num_slots = ctx.dynsym.symbols.len() as u64;
    let hash = ctx.hash.as_mut().unwrap();
    hash.hdr.shdr.sh_size.set(entry * 2 + num_slots * entry * 2);
    hash.hdr.shdr.sh_link.set(ctx.dynsym.hdr.shndx);
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    buf.fill(0);
    let entry = entry_size::<E>();
    let write = |buf: &mut [u8], i: usize, v: u32| {
        if entry == 8 {
            E::Endian::write_u64(&mut buf[i * 8..], v as u64);
        } else {
            E::Endian::write_u32(&mut buf[i * 4..], v);
        }
    };
    let read = |buf: &[u8], i: usize| -> u32 {
        if entry == 8 {
            E::Endian::read_u64(&buf[i * 8..]) as u32
        } else {
            E::Endian::read_u32(&buf[i * 4..])
        }
    };

    let n = ctx.dynsym.symbols.len();
    write(buf, 0, n as u32);
    write(buf, 1, n as u32);
    let buckets = 2;
    let chains = 2 + n;

    for &id in ctx.dynsym.symbols.iter().skip(1).flatten() {
        let sym = &ctx.symbols[id];
        let i = sym.dynsym_idx(&ctx.symbols).unwrap() as usize;
        let h = elf_hash(sym.name()) as usize % n;
        let head = read(buf, buckets + h);
        write(buf, chains + i, head);
        write(buf, buckets + h, i as u32);
    }
}
