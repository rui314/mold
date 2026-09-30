//! The symbol table in __LINKEDIT, and the writer that emits it together
//! with the string table.

use rayon::prelude::*;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::macho::*;
use crate::passes::StabPlan;
use crate::symbol::SymbolId;
use crate::target::Target;

/// The symbol table, laid out before addresses are known. The symbol
/// slot of each entry supplies its final `n_value` when the table is
/// copied to the output.
#[derive(Debug)]
pub struct SymtabSection {
    pub hdr: ChunkHeader,
    /// The entries but the debug notes: those before the notes - the
    /// plain locals, N_AST paths and the notes' opening N_SO - then the
    /// externals and imports, which follow the notes in the table.
    pub entries: Vec<(NList, Option<SymbolId>)>,
    /// The string table's total size (bytes, padded to 8). The bytes
    /// themselves are not materialized here: copy_symtab writes each
    /// entry's name at its n_strx straight into the output.
    pub strtab_size: usize,
    /// Each entry's name, empty for one that has no string of its own
    /// (it names nothing, or shares another entry's).
    pub names: Vec<&'static [u8]>,
    /// The debug notes, one plan per object, which copy_symtab writes
    /// straight into the output as mold-rust's populate_symtab writes a
    /// file's symbols: entries [stabs_start, stabs_start + nstabs) of the
    /// table, each object's run after the previous one's, and each run's
    /// strings from its offset in `stab_strx`, whose last element is
    /// where the notes' strings end.
    pub stabs: Vec<StabPlan>,
    pub stab_strx: Vec<u32>,
    pub stabs_start: usize,
    pub nstabs: usize,
    /// Each symbol's string offset, for the notes naming it, or u32::MAX
    /// if they need a copy of their own.
    pub strx_of: Vec<u32>,
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
            stabs: Vec::new(),
            stab_strx: Vec::new(),
            stabs_start: 0,
            nstabs: 0,
            strx_of: Vec::new(),
            nlocal: 0,
            nextdef: 0,
            nundef: 0,
            output_sym_indices: Vec::new(),
        }
    }

    /// The number of entries in the table, the debug notes included.
    pub fn len(&self) -> usize {
        self.entries.len() + self.nstabs
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
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
    let symsize = symtab.len() * size_of::<NList>();
    let (syms, strtab) = if symoff + symsize <= stroff {
        let (lo, hi) = buf.split_at_mut(stroff);
        (&mut lo[symoff..symoff + symsize], &mut hi[..symtab.strtab_size])
    } else {
        let (lo, hi) = buf.split_at_mut(symoff);
        (&mut hi[..symsize], &mut lo[stroff..stroff + symtab.strtab_size])
    };

    // The entries before the notes, the notes, and the externals; the
    // entries' strings, then the notes'.
    let start = symtab.stabs_start;
    let (local_syms, rest) = syms.split_at_mut(start * size_of::<NList>());
    let (mut stab_syms, extern_syms) = rest.split_at_mut(symtab.nstabs * size_of::<NList>());
    let stab_strx = symtab.stab_strx.first().map_or(strtab.len(), |&strx| strx as usize);
    let (strtab, mut stab_strtab) = strtab.split_at_mut(stab_strx);

    // Each object's notes are a block of the table of its own, with its
    // strings, carved off in order and written in parallel - mold-rust's
    // symtab copy_buf and populate_symtab.
    let mut blocks = Vec::with_capacity(symtab.stabs.len());
    for (plan, strx) in symtab.stabs.iter().zip(symtab.stab_strx.windows(2)) {
        let (syms, rest) = stab_syms.split_at_mut(plan.len() * size_of::<NList>());
        let (strs, strs_rest) = stab_strtab.split_at_mut((strx[1] - strx[0]) as usize);
        (stab_syms, stab_strtab) = (rest, strs_rest);
        blocks.push(SymtabBlock {
            syms,
            len: 0,
            strtab: strs,
            strtab_base: strx[0],
            strtab_len: 0,
        });
    }
    let stabs = || {
        symtab.stabs.par_iter().zip(blocks).for_each(|(plan, mut block)| {
            plan.populate_symtab(ctx, &symtab.strx_of, &mut block);
        });
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
    let (locals, externs) = symtab.entries.split_at(start);
    let (local_names, extern_names) = symtab.names.split_at(start);
    let write = |syms: &mut [u8], ents: &[(NList, Option<SymbolId>)], names: &[&[u8]]| {
        syms.par_chunks_mut(BLOCK * size_of::<NList>())
            .zip(ents.par_chunks(BLOCK))
            .zip(names.par_chunks(BLOCK))
            .for_each(|((out, ents), names)| {
                for (i, ((nlist, sym), name)) in ents.iter().zip(names).enumerate() {
                    let mut nlist = *nlist;
                    if let Some(id) = sym {
                        nlist.n_value = ctx.sym_addr(*id);
                    }
                    nlist.write_to(&mut out[i * size_of::<NList>()..]);
                    // SAFETY: layout_strings gave each name a range of
                    // its own within the string table.
                    unsafe {
                        let dst = strtab.0.add(nlist.n_strx as usize);
                        std::ptr::copy_nonoverlapping(name.as_ptr(), dst, name.len());
                    }
                }
            });
    };
    let entries = || {
        rayon::join(
            || write(local_syms, locals, local_names),
            || write(extern_syms, externs, extern_names),
        )
    };
    rayon::join(entries, stabs);
}

/// An object's block of the symbol table and of the string table, which
/// its debug notes are written into in place - mold-rust's SymtabBlock.
/// Blocks don't overlap, so they are written in parallel.
pub struct SymtabBlock<'a> {
    syms: &'a mut [u8],
    len: usize,
    strtab: &'a mut [u8],
    /// The offset of `strtab` within the string table.
    strtab_base: u32,
    strtab_len: usize,
}

impl SymtabBlock<'_> {
    pub fn push(&mut self, nlist: NList) {
        nlist.write_to(&mut self.syms[self.len * size_of::<NList>()..]);
        self.len += 1;
    }

    /// Adds a string, returning its offset in the string table.
    pub fn add_string(&mut self, name: &[u8]) -> u32 {
        let strx = self.strtab_base + self.strtab_len as u32;
        let strs = &mut self.strtab[self.strtab_len..];
        strs[..name.len()].copy_from_slice(name);
        strs[name.len()] = 0;
        self.strtab_len += name.len() + 1;
        strx
    }
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
/// where the strings end.
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
    total as usize
}
