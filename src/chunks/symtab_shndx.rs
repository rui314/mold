//! `.symtab_shndx`, extended section indices for the symbol table.

use crate::chunks::ChunkHeader;
use crate::elf::*;

// .symtab_shndx is a parallel table for .symtab to contain section
// indices for symbols.
//
// Symbol table entry contains a field for section index, but that's only
// 16 bit in size, so it cannot refer to a section whose section index is
// greater than 65535. We use .symtab_shndx for ELF files containing a lot
// of sections.
//
// Use of this section is exceptional. Most ELF files don't contain one.
pub fn new_header<E: Layout>() -> ChunkHeader<E> {
    let mut hdr = ChunkHeader::<E>::new(".symtab_shndx", SHT_SYMTAB_SHNDX, 0);
    hdr.shdr.sh_entsize.set(4);
    hdr.shdr.sh_addralign.set(4);
    hdr
}
