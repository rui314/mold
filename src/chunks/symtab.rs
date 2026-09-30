//! The symbol table in __LINKEDIT, and the writer that emits it together
//! with the string table.

use rayon::prelude::*;

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::macho::*;
use crate::symbol::SymbolId;
use crate::target::Target;

/// The symbol table, laid out before addresses are known. The symbol
/// slot of each entry supplies its final `n_value` when the table is
/// copied to the output.
#[derive(Debug)]
pub struct SymtabSection {
    pub hdr: ChunkHeader,
    pub entries: Vec<(NList, Option<SymbolId>)>,
    /// The string table's total size (bytes, padded to 8). The bytes
    /// themselves are not materialized here - `strtab_uniques` lists
    /// the deduplicated strings with their offsets, and copy_symtab
    /// writes them straight into the output, skipping a 150MB temp Vec
    /// and the copy that would follow it.
    pub strtab_size: usize,
    /// Each distinct string with its offset in the string table.
    pub strtab_uniques: Vec<(u32, &'static [u8])>,
    pub nlocal: u32,
    pub nextdef: u32,
    pub nundef: u32,
    /// Each symbol's index in the output symbol table (u32::MAX if
    /// absent), for the indirect symbol table. mold keeps output
    /// symtab indices as direct per-symbol data too, not in a map;
    /// one flat array serves here because Mach-O name-sorts its
    /// globals across all files, which rules out per-file bases.
    pub output_sym_indices: Vec<u32>,
}

impl SymtabSection {
    pub fn new() -> Self {
        Self {
            hdr: ChunkHeader::linkedit(),
            entries: Vec::new(),
            strtab_size: 0,
            strtab_uniques: Vec::new(),
            nlocal: 0,
            nextdef: 0,
            nundef: 0,
            output_sym_indices: Vec::new(),
        }
    }
}

impl Default for SymtabSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_symtab<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let off = ctx.symtab.hdr.fileoff as usize;
    let entries = &ctx.symtab.entries;
    // Millions of entries, each wanting a sym_addr lookup for its
    // n_value: emit them in parallel blocks.
    const BLOCK: usize = 4096;
    buf[off..off + entries.len() * size_of::<NList>()]
        .par_chunks_mut(BLOCK * size_of::<NList>())
        .zip(entries.par_chunks(BLOCK))
        .for_each(|(out, ents)| {
            for (i, (nlist, sym)) in ents.iter().enumerate() {
                let mut nlist = *nlist;
                if let Some(id) = sym {
                    nlist.n_value = ctx.sym_addr(*id);
                }
                nlist.write_to(&mut out[i * size_of::<NList>()..]);
            }
        });

    // The string table is written straight into the output here - no
    // intermediate 150MB buffer. The " \0" prefix (offset 1 is the empty
    // string), then every string at its offset, on all cores; each
    // string owns a disjoint range.
    let off = ctx.strtab.hdr.fileoff as usize;
    let strtab = &mut buf[off..off + ctx.symtab.strtab_size];
    strtab[..2].copy_from_slice(b" \0");
    struct BufPtr(*mut u8);
    unsafe impl Sync for BufPtr {}
    let base = BufPtr(strtab.as_mut_ptr());
    let base = &base;
    ctx.symtab.strtab_uniques.par_iter().for_each(|&(o, name)| {
        let o = o as usize;
        // SAFETY: strings occupy disjoint [o, o+len+1) ranges within
        // the string table; the trailing NUL is already zero in buf.
        unsafe {
            std::ptr::copy_nonoverlapping(name.as_ptr(), base.0.add(o), name.len());
        }
    });
}

/// Lays out a symbol table's strings as ld-prime does and sets every
/// entry's n_strx. The strings follow the defined and undefined
/// externals, then the local symbols, then the debug notes, each entry
/// with a copy of its own - two locals of one name get two - except
/// that a note naming a symbol shares that symbol's string (though not
/// the first local's, which ld-prime copies again) and an empty name is
/// offset 1, after the table's leading " ". Entries [0, nplain) are the
/// plain locals, `stabs` gives where the notes start and the symbol
/// each one names, and [nlocal, len) are the externals. Returns every
/// string written with its offset, and the table's size, padded to 8.
pub fn layout_strings(
    entries: &mut [(NList, Option<SymbolId>)],
    names: &[&'static [u8]],
    nplain: usize,
    stabs: (usize, &[Option<SymbolId>]),
    nlocal: usize,
    nsyms: usize,
) -> (Vec<(u32, &'static [u8])>, usize) {
    use std::sync::atomic::{AtomicU32, Ordering};
    let n = entries.len();

    // The entry each symbol has, for the notes naming it.
    let entry_of: Vec<AtomicU32> =
        (0..nsyms).into_par_iter().map(|_| AtomicU32::new(u32::MAX)).collect();
    (0..nplain).into_par_iter().chain(nlocal..n).for_each(|i| {
        if let Some(id) = entries[i].1 {
            entry_of[id as usize].store(i as u32, Ordering::Relaxed);
        }
    });
    let (stabs_start, names_of) = stabs;
    let shared = |i: usize| -> Option<usize> {
        let id = (*names_of.get(i.checked_sub(stabs_start)?)?)?;
        let e = entry_of[id as usize].load(Ordering::Relaxed);
        (e != u32::MAX && e != 0).then_some(e as usize)
    };

    // The externals' strings come first: position p holds entry
    // nlocal + p, then the locals and notes follow.
    let entry_at = |p: usize| if p < n - nlocal { nlocal + p } else { p - (n - nlocal) };
    let lens: Vec<u32> = (0..n)
        .into_par_iter()
        .map(|p| {
            let i = entry_at(p);
            if names[i].is_empty() || shared(i).is_some() { 0 } else { names[i].len() as u32 + 1 }
        })
        .collect();
    const CHUNK: usize = 1 << 16;
    let sums: Vec<u32> = lens.par_chunks(CHUNK).map(|c| c.iter().sum()).collect();
    let mut bases = Vec::with_capacity(sums.len());
    let mut total = 2u32;
    for sum in sums {
        bases.push(total);
        total += sum;
    }
    let mut offsets = vec![0u32; n];
    offsets.par_chunks_mut(CHUNK).zip(lens.par_chunks(CHUNK)).zip(&bases).for_each(
        |((out, lens), &base)| {
            let mut off = base;
            for (o, &len) in out.iter_mut().zip(lens) {
                *o = off;
                off += len;
            }
        },
    );

    // Each entry's own string first, then the notes that share one.
    let pos_of = |i: usize| if i >= nlocal { i - nlocal } else { n - nlocal + i };
    entries.par_iter_mut().enumerate().for_each(|(i, (ent, _))| {
        ent.n_strx = if names[i].is_empty() { 1 } else { offsets[pos_of(i)] };
    });
    let reused: Vec<(usize, u32)> = (stabs_start..nlocal)
        .into_par_iter()
        .filter_map(|i| shared(i).map(|e| (i, entries[e].0.n_strx)))
        .collect();
    for (i, strx) in reused {
        entries[i].0.n_strx = strx;
    }
    let uniques = (0..n)
        .into_par_iter()
        .filter(|&p| lens[p] != 0)
        .map(|p| (offsets[p], names[entry_at(p)]))
        .collect();
    (uniques, total.next_multiple_of(8) as usize)
}
