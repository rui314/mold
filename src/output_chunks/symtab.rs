//! Symbol and string tables: `.symtab`, `.strtab`, `.shstrtab`, `.dynsym`,
//! `.dynstr`, `.hash` and `.gnu.hash`.

use std::collections::HashMap;

use bstr::BStr;
use rayon::prelude::*;

use crate::arch::{Arch, Family};
use crate::context::Context;
use crate::elf::*;
use crate::input_files::{FileId, SymtabBlock, SymtabEntries};
use crate::input_sections::r_delta;
use crate::output_chunks::{self, ChunkHeader, ChunkId};
use crate::symbol::{AddrFlags, Symbol, SymbolId};
use crate::util::write_cstr;
use crate::{error, fatal};

// .strtab is referenced by .strtab and contains symbol names. Note that
// .strtab is not needed at runtime; one can remove the section from an
// ELF file without breaking it. Strings that runtime accesses are stored
// in .dynstr.
#[derive(Debug)]
pub struct StrtabSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
}

impl<E: Layout> StrtabSection<E> {
    pub fn new() -> StrtabSection<E> {
        StrtabSection {
            hdr: ChunkHeader::<E>::new(".strtab", SHT_STRTAB, 0),
        }
    }
}

impl<E: Layout> Default for StrtabSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

pub mod strtab {
    use super::*;

    // Offsets in .strtab for ARM32 mapping symbols
    pub const ARM: u32 = 1;
    pub const THUMB: u32 = 4;
    pub const DATA: u32 = 7;

    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
        let mut offset = 1u64;

        // ARM32 uses $a, $t and $t mapping symbols to mark the beginning of
        // ARM, Thumb and data in text, respectively. These symbols don't
        // affect correctness of the program but helps disassembler to
        // disassemble machine code appropriately.
        if E::FAMILY == Family::Arm32 && !ctx.args.strip_all {
            offset += b"$a\0$t\0$d\0".len() as u64;
        }

        for id in ctx.chunks.clone() {
            let hdr = ctx.chunk_header_mut(id);
            hdr.strtab_offset = offset;
            offset += hdr.strtab_size;
        }
        for file in &mut ctx.objs {
            file.base.strtab_offset = offset;
            offset += file.base.strtab_size;
        }
        for file in &mut ctx.dsos {
            file.base.strtab_offset = offset;
            offset += file.base.strtab_size;
        }
        ctx.strtab
            .hdr
            .shdr
            .sh_size
            .set(if offset == 1 { 0 } else { offset });
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        buf[0] = 0;
        if E::FAMILY == Family::Arm32 && !ctx.args.strip_all {
            buf[1..10].copy_from_slice(b"$a\0$t\0$d\0");
        }
    }
}

// .shstrtab contains section names, such as ".text" or ".data". Just like
// .strtab, .shstrtab is not needed at runtime. One can remove .shstrtab
// and section table from an executable without breaking it.
#[derive(Debug)]
pub struct ShstrtabSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
}

impl<E: Layout> ShstrtabSection<E> {
    pub fn new() -> ShstrtabSection<E> {
        ShstrtabSection {
            hdr: ChunkHeader::<E>::new(".shstrtab", SHT_STRTAB, 0),
        }
    }
}

impl<E: Layout> Default for ShstrtabSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

pub mod shstrtab {
    use super::*;

    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
        let mut map: HashMap<&'static BStr, u64> = HashMap::new();
        let mut offset = 1u64;
        for id in ctx.chunks.clone() {
            if id.is_header() {
                continue;
            }
            let name = ctx.chunk_header(id).name;
            if name.is_empty() {
                continue;
            }
            let off = *map.entry(name).or_insert_with(|| {
                let off = offset;
                offset += name.len() as u64 + 1;
                off
            });
            ctx.chunk_header_mut(id).shdr.sh_name.set(off as u32);
        }
        ctx.shstrtab.as_mut().unwrap().hdr.shdr.sh_size.set(offset);
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        buf[0] = 0;
        for &id in &ctx.chunks {
            let hdr = ctx.chunk_header(id);
            if hdr.shdr.sh_name.get() != 0 {
                write_cstr(&mut buf[hdr.shdr.sh_name.get() as usize..], hdr.name);
            }
        }
    }
}

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
        self.hdr
            .shdr
            .sh_size
            .set(self.hdr.shdr.sh_size.get() + s.len() as u64 + 1);
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

pub mod dynstr {
    use super::*;

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        for (s, &off) in &ctx.dynstr.strings {
            write_cstr(&mut buf[off as usize..], s);
        }
        let mut off = ctx.dynsym.dynstr_offset as usize;
        for &id in ctx.dynsym.symbols.iter().flatten() {
            off += write_cstr(&mut buf[off..], ctx.symbols[id].name());
        }
    }
}

// .symtab contains non-dynamic symbols. The section is not needed at
// runtime and can be stripped from an ELF file without affecting the
// behavior of the program. Symbols in .symtab are mainly for debugging.
#[derive(Debug)]
pub struct SymtabSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
}

impl<E: Arch> SymtabSection<E> {
    pub fn new() -> SymtabSection<E> {
        let mut hdr = ChunkHeader::<E>::new(".symtab", SHT_SYMTAB, 0);
        hdr.shdr
            .sh_entsize
            .set(std::mem::size_of::<ElfSym<E>>() as u64);
        hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
        SymtabSection { hdr }
    }
}

impl<E: Arch> Default for SymtabSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

// .symtab_shndx is a parallel table for .symtab to contain section
// indices for symbols.
//
// Symbol table entry contains a field for section index, but that's only
// 16 bit in size, so it cannot refer to a section whose section index is
// greater than 65535. We use .symtab_shndx for ELF files containing a lot
// of sections.
//
// Use of this section is exceptional. Most ELF files don't contain one.
#[derive(Debug)]
pub struct SymtabShndxSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
}

impl<E: Layout> SymtabShndxSection<E> {
    pub fn new() -> SymtabShndxSection<E> {
        let mut hdr = ChunkHeader::<E>::new(".symtab_shndx", SHT_SYMTAB_SHNDX, 0);
        hdr.shdr.sh_entsize.set(4);
        hdr.shdr.sh_addralign.set(4);
        SymtabShndxSection { hdr }
    }
}

impl<E: Layout> Default for SymtabShndxSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

// Named after the chunk, like the other chunks' modules in this directory.
#[allow(clippy::module_inception)]
pub mod symtab {
    use super::*;

    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
        let mut nsyms = 1u32;

        // Section symbols
        nsyms += ctx
            .chunks
            .iter()
            .filter(|&&id| ctx.chunk_header(id).shndx != 0)
            .count() as u32;

        // Linker-synthesized symbols
        for id in ctx.chunks.clone() {
            let hdr = ctx.chunk_header_mut(id);
            hdr.local_symtab_idx = nsyms;
            nsyms += hdr.num_local_symtab;
        }

        // File local symbols
        for file in &mut ctx.objs {
            file.base.local_symtab_idx = nsyms;
            nsyms += file.base.num_local_symtab;
        }

        // File global symbols
        for file in &mut ctx.objs {
            file.base.global_symtab_idx = nsyms;
            nsyms += file.base.num_global_symtab;
        }
        for file in &mut ctx.dsos {
            file.base.global_symtab_idx = nsyms;
            nsyms += file.base.num_global_symtab;
        }

        ctx.symtab
            .hdr
            .shdr
            .sh_info
            .set(ctx.objs.first().map_or(nsyms, |f| f.base.global_symtab_idx));
        ctx.symtab.hdr.shdr.sh_link.set(ctx.strtab.hdr.shndx);
        ctx.symtab.hdr.shdr.sh_size.set(if nsyms == 1 {
            0
        } else {
            nsyms as u64 * std::mem::size_of::<ElfSym<E>>() as u64
        });
    }

    /// Writes `.symtab`, `.strtab` and `.symtab_shndx`.
    pub fn copy_buf<E: Arch>(
        ctx: &Context<E>,
        symtab: &mut [u8],
        strtab: &mut [u8],
        mut xindex: Option<&mut [u8]>,
    ) {
        let size = std::mem::size_of::<ElfSym<E>>();
        symtab[..size].fill(0);
        if let Some(xindex) = xindex.as_deref_mut() {
            xindex.fill(0);
        }

        // Create section symbols
        for &id in &ctx.chunks {
            let hdr = ctx.chunk_header(id);
            if hdr.shndx == 0 {
                continue;
            }
            let mut esym = ElfSym::<E>::default();
            esym.st_value_mut().set(hdr.shdr.sh_addr.get());
            esym.set_type(STT_SECTION);
            match xindex.as_deref_mut() {
                Some(xindex) => {
                    E::Endian::write_u32(&mut xindex[hdr.shndx as usize * 4..], hdr.shndx);
                    esym.st_shndx_mut().set(SHN_XINDEX as u16);
                }
                None => esym.st_shndx_mut().set(hdr.shndx as u16),
            }
            esym.write(&mut symtab[hdr.shndx as usize * size..]);
        }

        strtab::copy_buf(ctx, strtab);

        // Symbols synthesized by chunks, then symbols from input files.
        // Each writer fills its own part of the tables: the local symbols
        // come first, the chunks' then the files', and the global symbols
        // after them, with the names laid out in the same order.
        enum Writer {
            Chunk(ChunkId),
            Obj(crate::input_files::ObjId),
            Dso(crate::input_files::DsoId),
        }
        struct Part {
            writer: Writer,
            locals: (u32, u32),
            globals: (u32, u32),
            strtab: (u64, u64),
        }
        let mut parts: Vec<Part> = Vec::new();

        // Populate linker-synthesized symbols
        for &id in &ctx.chunks {
            let hdr = ctx.chunk_header(id);
            if hdr.num_local_symtab != 0 {
                let locals = (hdr.local_symtab_idx, hdr.num_local_symtab);
                parts.push(Part {
                    writer: Writer::Chunk(id),
                    locals,
                    globals: (0, 0),
                    strtab: (hdr.strtab_offset, hdr.strtab_size),
                });
            }
        }
        // Copy symbols from input files
        for file in &ctx.objs {
            let base = &file.base;
            let locals = (base.local_symtab_idx, base.num_local_symtab);
            let globals = (base.global_symtab_idx, base.num_global_symtab);
            parts.push(Part {
                writer: Writer::Obj(file.id()),
                locals,
                globals,
                strtab: (base.strtab_offset, base.strtab_size),
            });
        }
        for file in &ctx.dsos {
            let base = &file.base;
            let globals = (base.global_symtab_idx, base.num_global_symtab);
            parts.push(Part {
                writer: Writer::Dso(file.id()),
                locals: (0, 0),
                globals,
                strtab: (base.strtab_offset, base.strtab_size),
            });
        }

        // Carve each part's slices off the buffers, which the parts cover
        // in order.
        let (mut rest, mut pos) = (symtab, 0);
        let (mut xrest, mut xpos) = (xindex, 0);
        let mut entries = |(start, count): (u32, u32)| {
            let syms = carve(
                &mut rest,
                &mut pos,
                start as usize * size,
                count as usize * size,
            );
            let xindex = xrest
                .as_mut()
                .map(|x| carve(x, &mut xpos, start as usize * 4, count as usize * 4));
            (syms, xindex)
        };
        let locals: Vec<_> = parts.iter().map(|p| entries(p.locals)).collect();
        let globals: Vec<_> = parts.iter().map(|p| entries(p.globals)).collect();
        let (mut srest, mut spos) = (strtab, 0);
        let strtabs: Vec<_> = parts
            .iter()
            .map(|p| {
                carve(
                    &mut srest,
                    &mut spos,
                    p.strtab.0 as usize,
                    p.strtab.1 as usize,
                )
            })
            .collect();

        parts
            .par_iter()
            .zip(locals)
            .zip(globals)
            .zip(strtabs)
            .for_each(|(((part, (lsyms, lx)), (gsyms, gx)), strtab)| {
                let mut block = SymtabBlock::new(
                    SymtabEntries::new(lsyms, lx),
                    SymtabEntries::new(gsyms, gx),
                    strtab,
                    part.strtab.0,
                );
                match part.writer {
                    Writer::Chunk(id) => output_chunks::populate_symtab(ctx, id, &mut block),
                    Writer::Obj(id) => ctx.objs[id.index()].populate_symtab(ctx, id, &mut block),
                    Writer::Dso(id) => ctx.dsos[id.index()].populate_symtab(ctx, id, &mut block),
                }
            });
    }

    /// Splits `len` bytes starting at `start` off `rest`, which begins
    /// at `pos` in the buffer; the pieces are taken in order.
    fn carve<'a>(
        rest: &mut &'a mut [u8],
        pos: &mut usize,
        start: usize,
        len: usize,
    ) -> &'a mut [u8] {
        if len == 0 {
            return &mut [];
        }
        let buf = std::mem::take(rest);
        let (_, buf) = buf.split_at_mut(start - *pos);
        let (piece, tail) = buf.split_at_mut(len);
        *rest = tail;
        *pos = start + len;
        piece
    }
}

// RISC-V and LoongArch have code-shrinking linker relaxation. If we
// have removed instructions from a function, we need to update its
// size as well.
fn symbol_size<E: Arch>(ctx: &Context<E>, sym: &Symbol) -> u64 {
    let esym = &sym.esym(ctx);
    if (E::IS_RISCV || E::IS_LOONGARCH) && esym.st_size().get() != 0 {
        if let Some(isec) = sym.input_section_ref() {
            if isec.sh_flags & SHF_EXECINSTR as u64 != 0 {
                let end = esym.st_value().get() + esym.st_size().get();
                return (esym.st_size().get() as i64 + esym.st_value().get() as i64
                    - sym.value as i64
                    - r_delta(isec, end)) as u64;
            }
        }
    }
    esym.st_size().get()
}

/// Builds the output symbol table entry for a symbol. The returned index
/// is nonzero if the section index doesn't fit in `st_shndx` and must go
/// to `.symtab_shndx`.
pub fn to_output_esym<E: Arch>(ctx: &Context<E>, sym: &Symbol, st_name: u32) -> (ElfSym<E>, u32) {
    let mut esym = ElfSym::<E>::default();
    esym.st_name_mut().set(st_name);
    esym.st_size_mut().set(symbol_size(ctx, sym));
    esym.set_type(sym.ty());

    let file = sym.file().expect("symbol without a file in symbol table");
    esym.set_bind(if sym.is_local(ctx) {
        STB_LOCAL
    } else if sym.is_weak() {
        STB_WEAK
    } else if file.is_dso() {
        STB_GLOBAL
    } else {
        sym.st_bind()
    });

    match E::FAMILY {
        Family::Arm64 => esym.set_arm64_variant_pcs(sym.esym(ctx).arm64_variant_pcs()),
        Family::RiscV => esym.set_riscv_variant_cc(sym.esym(ctx).riscv_variant_cc()),
        Family::Ppc64V2 => esym.set_ppc64_local_entry(sym.esym(ctx).ppc64_local_entry()),
        _ => {}
    }

    let st_shndx_of = |sym: &Symbol| -> u32 {
        if let Some(frag) = sym.fragment() {
            if ctx.fragment(frag).is_alive() {
                return ctx.merged_sections[frag.section.index()].hdr.shndx;
            }
        }
        if E::FAMILY == Family::Ppc64V1 && sym.has_opd(&ctx.symbols) {
            return ctx.ppc64_opd.as_ref().unwrap().hdr.shndx;
        }
        if let Some(isec) = sym.input_section_ref() {
            if isec.is_alive() {
                return ctx.output_section(isec.output_section.unwrap()).hdr.shndx;
            }
            if isec.is_icf_removed() {
                let leader = ctx.section(isec.icf_leader().unwrap());
                return ctx.output_section(leader.output_section.unwrap()).hdr.shndx;
            }
        }
        SHN_UNDEF
    };

    let mut shndx: Option<u32> = None;
    let isec = sym.input_section_ref();

    if sym.has_copyrel() {
        // Symbol in .copyrel
        shndx = Some(if sym.is_copyrel_readonly() {
            ctx.copyrel_relro.hdr.shndx
        } else {
            ctx.copyrel.hdr.shndx
        });
        esym.st_value_mut().set(sym.addr(ctx));
    } else if file.is_dso() || sym.is_undef() {
        // Undefined symbol in a DSO
        esym.st_shndx_mut().set(SHN_UNDEF as u16);
        esym.st_size_mut().set(0);
        if sym.is_canonical() {
            esym.st_value_mut().set(sym.plt_addr(ctx));
        }
    } else if let Some(chunk) = sym.output_chunk() {
        // Linker-synthesized symbol
        shndx = Some(ctx.chunk_header(chunk).shndx);
        esym.st_value_mut().set(sym.addr(ctx));
    } else if let Some(frag) = sym.fragment() {
        shndx = Some(ctx.merged_sections[frag.section.index()].hdr.shndx);
        esym.st_value_mut().set(sym.addr(ctx));
    } else if isec.is_none() {
        if sym.is_common() {
            // Common symbol. Common symbols are converted to .bss unless we are
            // creating a relocatable output, in which case they are passed
            // through as they are. st_value of a common symbol is its alignment.
            debug_assert!(ctx.args.relocatable);
            esym.st_shndx_mut().set(SHN_COMMON as u16);
            esym.st_value_mut().set(sym.esym(ctx).st_value().get());
        } else {
            // Absolute symbol
            esym.st_shndx_mut().set(SHN_ABS as u16);
            esym.st_value_mut().set(sym.addr(ctx));
        }
    } else if sym.ty() == STT_TLS {
        // TLS symbol
        shndx = Some(st_shndx_of(sym));
        esym.st_value_mut().set(sym.addr(ctx) - ctx.tls_begin);
    } else if sym.is_pde_ifunc(ctx) && sym.has_plt(&ctx.symbols) {
        // IFUNC symbol in PDE that uses two GOT slots
        shndx = Some(st_shndx_of(sym));
        esym.set_type(STT_FUNC);
        esym.set_visibility(sym.visibility());
        esym.st_value_mut().set(sym.plt_addr(ctx));
    } else if let Some(isec) = isec.filter(|isec| {
        isec.sh_flags & SHF_MERGE as u64 != 0 && isec.sh_flags & SHF_ALLOC as u64 == 0
    }) {
        // Symbol in a mergeable non-SHF_ALLOC section, such as .debug_str
        let file = &ctx.objs[isec.file.index()];
        let m = file
            .mergeable_section(file.shndx_at_in(sym.sym_idx as usize))
            .expect("mergeable section");
        let (frag, addend) = m
            .fragment(sym.esym(ctx).st_value().get())
            .expect("fragment");
        let msec = &ctx.merged_sections[m.parent.index()];
        shndx = Some(msec.hdr.shndx);
        esym.set_visibility(sym.visibility());
        esym.st_value_mut().set(
            (msec.hdr.shdr.sh_addr.get() + msec.fragments.get(frag).offset())
                .wrapping_add(addend as u64),
        );
    } else {
        // Symbol in a regular section
        shndx = Some(st_shndx_of(sym));
        esym.set_visibility(sym.visibility());
        esym.st_value_mut()
            .set(sym.addr_with(ctx, AddrFlags::NO_PLT));
    }

    // Symbol's st_shndx is only 16 bits wide, so we can't store a large
    // section index there. If the total number of sections is equal to
    // or greater than SHN_LORESERVE (= 65280), the real index is stored
    // to a SHT_SYMTAB_SHNDX section which contains a parallel array of
    // the symbol table.
    let mut xindex = 0;
    if let Some(shndx) = shndx {
        if shndx < SHN_LORESERVE {
            esym.st_shndx_mut().set(shndx as u16);
        } else {
            esym.st_shndx_mut().set(SHN_XINDEX as u16);
            xindex = shndx;
        }
    }
    (esym, xindex)
}

// .dynsym contains symbols for dynamic linking. This is similar to
// .symtab, but .dynsym contains data that the runtime uses.
#[derive(Debug)]
pub struct DynsymSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
    /// Index 0 is the null symbol.
    pub symbols: Vec<Option<SymbolId>>,
    pub dynstr_offset: u64,
}

impl<E: Arch> DynsymSection<E> {
    pub fn new() -> DynsymSection<E> {
        let mut hdr = ChunkHeader::<E>::new(".dynsym", SHT_DYNSYM, SHF_ALLOC as u64);
        hdr.shdr
            .sh_entsize
            .set(std::mem::size_of::<ElfSym<E>>() as u64);
        hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
        DynsymSection {
            hdr,
            symbols: Vec::new(),
            dynstr_offset: 0,
        }
    }
}

impl<E: Arch> Default for DynsymSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

pub mod dynsym {
    use super::*;

    pub fn add_symbol<E: Arch>(ctx: &mut Context<E>, sym: SymbolId) {
        if ctx.dynsym.symbols.is_empty() {
            ctx.dynsym.symbols.push(None);
        }
        if ctx.symbols[sym].dynsym_idx(&ctx.symbols).is_none() {
            // A placeholder; the real index is assigned by sort_dynsyms.
            ctx.symbols.aux_mut(sym).dynsym_idx = Some(u32::MAX);
            ctx.dynsym.symbols.push(Some(sym));
        }
    }

    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
        ctx.dynsym.hdr.shdr.sh_link.set(ctx.dynstr.hdr.shndx);
        ctx.dynsym
            .hdr
            .shdr
            .sh_size
            .set(std::mem::size_of::<ElfSym<E>>() as u64 * ctx.dynsym.symbols.len() as u64);
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        let size = std::mem::size_of::<ElfSym<E>>();
        buf[..size].fill(0);
        let mut offset = ctx.dynsym.dynstr_offset as u32;
        for &id in ctx.dynsym.symbols.iter().skip(1).flatten() {
            let sym = &ctx.symbols[id];
            let (esym, xindex) = to_output_esym(ctx, sym, offset);
            if xindex != 0 {
                let nshdrs = ctx.shdr.as_ref().map_or(0, |s| {
                    s.hdr.shdr.sh_size.get() / ElfShdr::<E>::size() as u64
                });
                error!(
                    ctx,
                    "{}: .dynsym: too many output sections: {nshdrs} requested, but ELF allows at most 65279",
                    ctx.args.output
                );
                return;
            }
            esym.write(&mut buf[sym.dynsym_idx(&ctx.symbols).unwrap() as usize * size..]);
            offset += sym.name().len() as u32 + 1;
        }
    }
}

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

/// The hash function for `.gnu.hash`.
// The hash function for .gnu.hash.
pub fn djb_hash(name: &[u8]) -> u32 {
    let mut h: u32 = 5381;
    for &c in name {
        h = (h << 5).wrapping_add(h).wrapping_add(c as u32);
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
        let entry = hash::entry_size::<E>() as u64;
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

pub mod hash {
    use super::*;

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
}

// .gnu.hash is an alternative format for .hash. It contains not only an
// on-disk hash table but also contains a bloom filter to quickly identify
// whether or not a given symbol name exists in .dynsym.
#[derive(Debug)]
pub struct GnuHashSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
    pub num_buckets: u32,
    pub num_bloom: u32,
    pub num_exported: u32,
}

impl<E: Arch> GnuHashSection<E> {
    pub const LOAD_FACTOR: u32 = 8;
    pub const HEADER_SIZE: u64 = 16;
    pub const BLOOM_SHIFT: u32 = 26;

    pub fn new() -> GnuHashSection<E> {
        let mut hdr = ChunkHeader::<E>::new(".gnu.hash", SHT_GNU_HASH, SHF_ALLOC as u64);
        hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
        GnuHashSection {
            hdr,
            num_buckets: 0,
            num_bloom: 1,
            num_exported: 0,
        }
    }
}

impl<E: Arch> Default for GnuHashSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

pub mod gnu_hash {
    use super::*;

    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
        if ctx.dynsym.symbols.is_empty() {
            return;
        }
        let word = E::WORD_SIZE as u64;
        let gh = ctx.gnu_hash.as_mut().unwrap();
        // We allocate 12 bits for each symbol in the bloom filter.
        gh.num_bloom = ((gh.num_exported as u64 * 12) / (word * 8))
            .max(1)
            .next_power_of_two() as u32;
        gh.hdr.shdr.sh_size.set(
            GnuHashSection::<E>::HEADER_SIZE
                + gh.num_bloom as u64 * word // Bloom filter
                + gh.num_buckets as u64 * 4 // Hash buckets
                + gh.num_exported as u64 * 4, // Hash values
        );
        gh.hdr.shdr.sh_link.set(ctx.dynsym.hdr.shndx);
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        buf.fill(0);
        let gh = ctx.gnu_hash.as_ref().unwrap();
        let word = E::WORD_SIZE;
        let first_exported = ctx.dynsym.symbols.len() - gh.num_exported as usize;

        E::Endian::write_u32(buf, gh.num_buckets);
        E::Endian::write_u32(&mut buf[4..], first_exported as u32);
        E::Endian::write_u32(&mut buf[8..], gh.num_bloom);
        E::Endian::write_u32(&mut buf[12..], GnuHashSection::<E>::BLOOM_SHIFT);

        let syms: Vec<SymbolId> = ctx.dynsym.symbols[first_exported..]
            .iter()
            .flatten()
            .copied()
            .collect();
        if syms.is_empty() {
            return;
        }

        // Write a bloom filter
        let bloom_off = GnuHashSection::<E>::HEADER_SIZE as usize;
        let word_bits = word * 8;
        let mut indices = Vec::with_capacity(syms.len());
        for &id in &syms {
            let h = ctx.symbols[id].aux(&ctx.symbols).unwrap().djb_hash;
            indices.push(h % gh.num_buckets);
            let idx = (h as usize / word_bits) % gh.num_bloom as usize;
            let bits = (1u64 << (h as usize % word_bits))
                | (1u64 << ((h >> GnuHashSection::<E>::BLOOM_SHIFT) as usize % word_bits));
            let slot = &mut buf[bloom_off + idx * word..];
            if E::IS_64 {
                E::Endian::write_u64(slot, E::Endian::read_u64(slot) | bits);
            } else {
                E::Endian::write_u32(slot, E::Endian::read_u32(slot) | bits as u32);
            }
        }

        // Write hash bucket indices
        let buckets_off = bloom_off + gh.num_bloom as usize * word;
        for (i, &bucket) in indices.iter().enumerate().rev() {
            E::Endian::write_u32(
                &mut buf[buckets_off + bucket as usize * 4..],
                (first_exported + i) as u32,
            );
        }

        // Write a hash table
        let table_off = buckets_off + gh.num_buckets as usize * 4;
        for (i, &id) in syms.iter().enumerate() {
            // The last entry in a chain must be terminated with an entry with
            // least-significant bit 1.
            let h = ctx.symbols[id].aux(&ctx.symbols).unwrap().djb_hash;
            let last = i + 1 == syms.len() || indices[i] != indices[i + 1];
            E::Endian::write_u32(
                &mut buf[table_off + i * 4..],
                if last { h | 1 } else { h & !1 },
            );
        }
    }
}

/// Chunk ids for which dynamic symbol information must exist.
pub fn require_dynsym<E: Arch>(ctx: &Context<E>) {
    if ctx.dynsym.symbols.is_empty() {
        fatal!(ctx, "internal error: dynamic symbol table required");
    }
}

// True if the symbol's address is in the output file.
pub fn is_defined_in_output(sym: &Symbol) -> bool {
    matches!(sym.file(), Some(FileId::Obj(_)))
}

/// The chunk id of `.symtab_shndx` if it exists.
pub fn symtab_shndx_id<E: Arch>(ctx: &Context<E>) -> Option<ChunkId> {
    ctx.symtab_shndx.as_ref().map(|_| ChunkId::SymtabShndx)
}
