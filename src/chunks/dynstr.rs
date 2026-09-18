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
    // Split at block boundaries so each worker owns its output range.
    let entries = &ctx.dynsym.dynstr_entries;
    let mut rest = buf;
    let mut pos = 0;
    let parts: Vec<_> = entries
        .get(1..)
        .unwrap_or_default()
        .chunks(1024)
        .map(|chunk| {
            let start = chunk[0].offset as usize;
            let last = chunk.last().unwrap();
            let end = last.offset as usize + last.name.len() + 1;
            rest.split_off_mut(..start - pos).unwrap();
            let out = rest.split_off_mut(..end - start).unwrap();

            pos = end;
            (chunk, out)
        })
        .collect();
    parts.into_par_iter().for_each(|(chunk, out)| {
        for entry in chunk {
            write_cstr(
                &mut out[(entry.offset - chunk[0].offset) as usize..],
                entry.name,
            );
        }
    });
}
