//! The symbol table, string table and indirect symbol table in
//! __LINKEDIT.

use crate::macho::arch::Arch;
use crate::macho::context::Context;
use crate::macho::format::*;
use crate::macho::output_chunks::ChunkHeader;
use crate::macho::symbol::SymbolId;

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
    pub strtab_uniques: Vec<(u32, &'static str)>,
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

impl Default for SymtabSection {
    fn default() -> Self {
        Self::new()
    }
}

impl SymtabSection {
    pub fn new() -> SymtabSection {
        SymtabSection {
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

/// The string table.
#[derive(Debug)]
pub struct StrtabSection {
    pub hdr: ChunkHeader,
}

impl Default for StrtabSection {
    fn default() -> Self {
        Self::new()
    }
}

impl StrtabSection {
    pub fn new() -> StrtabSection {
        StrtabSection { hdr: ChunkHeader::linkedit() }
    }
}

/// The indirect symbol table: for each __stubs, __got and
/// __la_symbol_ptr slot, the output symbol it holds.
#[derive(Debug)]
pub struct IndirectSymtabSection {
    pub hdr: ChunkHeader,
}

impl Default for IndirectSymtabSection {
    fn default() -> Self {
        Self::new()
    }
}

impl IndirectSymtabSection {
    pub fn new() -> IndirectSymtabSection {
        IndirectSymtabSection { hdr: ChunkHeader::linkedit() }
    }
}

pub mod indirect_symtab {
    use super::*;

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        let mut off = 0;
        let lazy: &[SymbolId] = if ctx.lazy_binding() { &ctx.stubs.symbols } else { &[] };
        // A GOT slot holding a definition of this image that dyld never
        // rebinds is INDIRECT_SYMBOL_LOCAL, as ld64 writes it, whatever
        // the symbol's scope; a stub's or an imported (or
        // weak-coalesced) symbol's slot names the symbol.
        let entries = ctx
            .stubs
            .symbols
            .iter()
            .map(|&id| (id, false))
            .chain(ctx.got.got_syms.iter().map(|&id| (id, !ctx.binds_at_runtime(id))))
            .chain(lazy.iter().map(|&id| (id, false)));
        for (id, local) in entries {
            let val = match ctx.symtab.output_sym_indices[id as usize] {
                _ if local => INDIRECT_SYMBOL_LOCAL,
                u32::MAX => INDIRECT_SYMBOL_LOCAL,
                idx => idx,
            };
            buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
            off += 4;
        }
    }
}

pub fn copy_symtab<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    use rayon::prelude::*;
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
