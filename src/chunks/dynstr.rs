//! `.dynstr`, strings used by the dynamic linker.

use std::collections::HashMap;

use rayon::prelude::*;

use crate::arch::Arch;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::util::write_cstr;

// .dynstr contains strings that the runtime uses.
#[derive(Debug)]
pub struct DynstrSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
    strings: HashMap<Vec<u8>, u64>,
}

impl<E: Layout> DynstrSection<E> {
    pub fn new() -> DynstrSection<E> {
        DynstrSection {
            hdr: ChunkHeader::<E>::new(".dynstr", SHT_STRTAB, SHF_ALLOC as u64),
            strings: HashMap::new(),
        }
    }

    pub fn add_string(&mut self, s: &[u8]) -> u64 {
        if self.hdr.shdr.sh_size.get() == 0 {
            self.strings.insert(Vec::new(), 0);
            self.hdr.shdr.sh_size.set(1);
        }
        if let Some(&off) = self.strings.get(s) {
            return off;
        }
        let off = self.hdr.shdr.sh_size.get();
        self.strings.insert(s.to_vec(), off);
        self.hdr.shdr.sh_size.set(off + s.len() as u64 + 1);
        off
    }

    pub fn find_string(&self, s: &[u8]) -> u64 {
        *self.strings.get(s).expect("string not in .dynstr")
    }
}

impl<E: Layout> Default for DynstrSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    for (s, &off) in &ctx.dynstr.strings {
        write_cstr(&mut buf[off as usize..], s);
    }
    let offsets = symbol_offsets(ctx);
    let mut rest = buf;
    let mut pos = 0;
    let parts: Vec<_> = ctx
        .dynsym
        .symbols
        .get(1..)
        .unwrap_or_default()
        .chunks(1024)
        .zip(offsets.get(1..).unwrap_or_default().chunks(1024))
        .map(|(chunk, offsets)| {
            let start = offsets[0] as usize;
            let end = *offsets.last().unwrap() as usize
                + ctx.symbols[chunk.last().unwrap().unwrap()].name().len()
                + 1;
            let (_, tail) = std::mem::take(&mut rest).split_at_mut(start - pos);
            let (out, tail) = tail.split_at_mut(end - start);
            rest = tail;
            pos = end;
            (chunk, offsets, out)
        })
        .collect();
    parts.into_par_iter().for_each(|(chunk, offsets, out)| {
        for (&id, &offset) in chunk.iter().zip(offsets) {
            write_cstr(
                &mut out[(offset - offsets[0]) as usize..],
                ctx.symbols[id.unwrap()].name(),
            );
        }
    });
}

/// Computes dynamic-name offsets for one output writer using block prefix sums.
pub(crate) fn symbol_offsets<E: Arch>(ctx: &Context<E>) -> Vec<u64> {
    let mut offsets: Vec<u64> = ctx
        .dynsym
        .symbols
        .par_iter()
        .map(|id| id.map_or(0, |id| ctx.symbols[id].name().len() as u64 + 1))
        .collect();
    let mut blocks: Vec<u64> = offsets
        .par_chunks(1024)
        .map(|chunk| chunk.iter().sum())
        .collect();
    let mut end = ctx.dynsym.dynstr_offset;
    for total in &mut blocks {
        let size = *total;
        *total = end;
        end += size;
    }
    offsets
        .par_chunks_mut(1024)
        .zip(blocks)
        .for_each(|(chunk, mut off)| {
            for value in chunk {
                let size = *value;
                *value = off;
                off += size;
            }
        });
    offsets
}
