//! The symbol table in __LINKEDIT, and the writer that emits it together
//! with the string table.

use rayon::prelude::*;
use std::sync::atomic::{AtomicU32, Ordering};

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
    /// themselves are not materialized here: copy_symtab writes each
    /// entry's name at its n_strx straight into the output.
    pub strtab_size: usize,
    /// Each entry's name, empty for one that has no string of its own
    /// (it names nothing, or shares another entry's).
    pub names: Vec<&'static [u8]>,
    pub nlocal: u32,
    pub nextdef: u32,
    pub nundef: u32,
    /// Each symbol's index in the output symbol table - its local's,
    /// external's or import's entry, never a debug note's - or u32::MAX
    /// if it has none, for the indirect symbol table. mold keeps output
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
            names: Vec::new(),
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
    let symtab = &ctx.symtab;
    let symoff = symtab.hdr.fileoff as usize;
    let stroff = ctx.strtab.hdr.fileoff as usize;
    let symsize = symtab.entries.len() * size_of::<NList>();
    let (syms, strtab) = if symoff + symsize <= stroff {
        let (lo, hi) = buf.split_at_mut(stroff);
        (&mut lo[symoff..symoff + symsize], &mut hi[..symtab.strtab_size])
    } else {
        let (lo, hi) = buf.split_at_mut(symoff);
        (&mut hi[..symsize], &mut lo[stroff..stroff + symtab.strtab_size])
    };

    // The string table opens with " \0" (offset 1 is the empty string);
    // every other string is written with its entry, straight into the
    // output. Each owns a disjoint range, and its NUL is already zero.
    strtab[..2].copy_from_slice(b" \0");
    struct BufPtr(*mut u8);
    unsafe impl Sync for BufPtr {}
    let strtab = BufPtr(strtab.as_mut_ptr());
    let strtab = &strtab;

    // Millions of entries, each wanting a sym_addr lookup for its
    // n_value: emit them in parallel blocks.
    const BLOCK: usize = 4096;
    syms.par_chunks_mut(BLOCK * size_of::<NList>())
        .zip(symtab.entries.par_chunks(BLOCK))
        .zip(symtab.names.par_chunks(BLOCK))
        .for_each(|((out, ents), names)| {
            for (i, ((nlist, sym), name)) in ents.iter().zip(names).enumerate() {
                let mut nlist = *nlist;
                if let Some(id) = sym {
                    nlist.n_value = ctx.sym_addr(*id);
                }
                nlist.write_to(&mut out[i * size_of::<NList>()..]);
                // SAFETY: layout_strings gave each name a range of its
                // own within the string table.
                unsafe {
                    let dst = strtab.0.add(nlist.n_strx as usize);
                    std::ptr::copy_nonoverlapping(name.as_ptr(), dst, name.len());
                }
            }
        });
}

/// Each symbol's entry among a symbol table's plain locals, [0, nplain),
/// and externals, [nlocal, len) - never a debug note - or u32::MAX.
pub fn symbol_entries(
    entries: &[(NList, Option<SymbolId>)],
    nplain: usize,
    nlocal: usize,
    nsyms: usize,
) -> Vec<AtomicU32> {
    let entry_of: Vec<AtomicU32> =
        (0..nsyms).into_par_iter().map(|_| AtomicU32::new(u32::MAX)).collect();
    (0..nplain).into_par_iter().chain(nlocal..entries.len()).for_each(|i| {
        if let Some(id) = entries[i].1 {
            entry_of[id as usize].store(i as u32, Ordering::Relaxed);
        }
    });
    entry_of
}

/// Lays out a symbol table's strings as ld-prime does and sets every
/// entry's n_strx. The strings follow the defined and undefined
/// externals, then the local symbols, then the debug notes, each entry
/// with a copy of its own - two locals of one name get two - except
/// that a note naming a symbol shares that symbol's string (though not
/// the first local's, which ld-prime copies again) and an empty name is
/// offset 1, after the table's leading " ". `stabs` gives where the
/// notes start and the symbol each one names, `entry_of` each symbol's
/// entry (symbol_entries), and [nlocal, len) are the externals. A note
/// that shares a string gets an empty name, so that afterwards an
/// entry's name is exactly the string to write at its n_strx. Returns
/// the table's size, padded to 8.
pub fn layout_strings(
    entries: &mut [(NList, Option<SymbolId>)],
    names: &mut [&[u8]],
    nlocal: usize,
    stabs: (usize, &[Option<SymbolId>]),
    entry_of: &[AtomicU32],
) -> usize {
    // The notes that share a string, and the entries they share it
    // with: a plain local's or an external's, never another note's.
    let (stabs_start, names_of) = stabs;
    let shared: Vec<u32> = names[stabs_start..stabs_start + names_of.len()]
        .par_iter_mut()
        .zip(names_of)
        .map(|(name, id)| {
            let e = id.map_or(u32::MAX, |id| entry_of[id as usize].load(Ordering::Relaxed));
            if e == u32::MAX || e == 0 {
                return u32::MAX;
            }
            *name = b"";
            e
        })
        .collect();

    // The externals' strings come first, then the locals' and notes',
    // each block of entries at its prefix-summed offset.
    const CHUNK: usize = 1 << 16;
    let (locals, externs) = entries.split_at_mut(nlocal);
    let (local_names, extern_names) = names.split_at(nlocal);
    let chunks = || extern_names.par_chunks(CHUNK).chain(local_names.par_chunks(CHUNK));
    let size = |name: &&[u8]| if name.is_empty() { 0 } else { name.len() as u32 + 1 };
    let sums: Vec<u32> = chunks().map(|c| c.iter().map(size).sum()).collect();
    let mut bases = Vec::with_capacity(sums.len());
    let mut total = 2u32;
    for sum in sums {
        bases.push(total);
        total += sum;
    }
    externs
        .par_chunks_mut(CHUNK)
        .chain(locals.par_chunks_mut(CHUNK))
        .zip(chunks())
        .zip(bases)
        .for_each(|((ents, names), mut off)| {
            for ((ent, _), name) in ents.iter_mut().zip(names) {
                ent.n_strx = if name.is_empty() { 1 } else { off };
                off += size(name);
            }
        });

    // The notes that share take their entry's offset.
    let (head, rest) = entries.split_at_mut(stabs_start);
    let (notes, tail) = rest.split_at_mut(shared.len());
    let tail_start = stabs_start + shared.len();
    notes.par_iter_mut().zip(shared).for_each(|((ent, _), e)| {
        let e = e as usize;
        if e != u32::MAX as usize {
            let owner = if e < stabs_start { &head[e] } else { &tail[e - tail_start] };
            ent.n_strx = owner.0.n_strx;
        }
    });
    total.next_multiple_of(8) as usize
}
