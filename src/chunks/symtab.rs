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
    // intermediate 150MB buffer. The " \0-\0" prefix (offsets 1 and 2
    // are the empty and "-" placeholders), then every distinct string
    // at its offset, on all cores; each string owns a disjoint range.
    let off = ctx.strtab.hdr.fileoff as usize;
    let strtab = &mut buf[off..off + ctx.symtab.strtab_size];
    strtab[..4].copy_from_slice(b" \0-\0");
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
