//! `.symtab_shndx`, extended section indices for the symbol table.

use crate::arch::Target;
use crate::chunks::ChunkHeader;
use crate::elf::*;

// .symtab_shndx is a table parallel to .symtab that contains section
// indices for symbols.
//
// A symbol table entry contains a field for the section index, but that's
// only 16 bits in size, and the values from SHN_LORESERVE (= 65280) up are
// reserved for special indices, so it cannot refer to a section whose
// section index is 65280 or greater. We use .symtab_shndx for ELF files
// containing a lot of sections.
//
// Use of this section is exceptional. Most ELF files don't contain one.
pub fn new_header<E: Target>() -> ChunkHeader<E> {
    let mut hdr = ChunkHeader::<E>::new(".symtab_shndx", SHT_SYMTAB_SHNDX, 0);
    hdr.shdr.sh_entsize.set(4);
    hdr.shdr.sh_addralign.set(4);
    hdr
}
