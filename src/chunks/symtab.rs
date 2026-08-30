//! Symbol and string tables: `.symtab`, `.strtab`, `.shstrtab`, `.dynsym`,
//! `.dynstr`, `.hash` and `.gnu.hash`.

use std::collections::HashMap;

use bstr::BStr;
use rayon::prelude::*;

use crate::arch::{Arch, Family};
use crate::chunks::{self, ChunkHeader, ChunkId};
use crate::context::Context;
use crate::elf::*;
use crate::input_files::{FileId, SymtabBlock, SymtabEntries};
use crate::input_sections::r_delta;
use crate::symbol::{AddrFlags, Symbol, SymbolId};
use crate::util::write_cstr;
use crate::{error, fatal};

/// `.strtab` holds the names of `.symtab` symbols. It isn't needed at
/// runtime; names the loader needs are in `.dynstr`.
#[derive(Debug)]
pub struct StrtabSection {
    pub hdr: ChunkHeader,
}

impl StrtabSection {
    pub fn new() -> StrtabSection {
        StrtabSection {
            hdr: ChunkHeader::new(".strtab", SHT_STRTAB, 0),
        }
    }
}

impl Default for StrtabSection {
    fn default() -> Self {
        Self::new()
    }
}

pub mod strtab {
    use super::*;

    /// Offsets in `.strtab` of the ARM32 mapping symbol names.
    pub const ARM: u32 = 1;
    pub const THUMB: u32 = 4;
    pub const DATA: u32 = 7;

    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
        let mut offset = 1u64;

        // ARM32 uses $a, $t and $d mapping symbols to mark ARM code, Thumb
        // code and data; they only help disassemblers.
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
        ctx.strtab.hdr.shdr.sh_size = if offset == 1 { 0 } else { offset };
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        buf[0] = 0;
        if E::FAMILY == Family::Arm32 && !ctx.args.strip_all {
            buf[1..10].copy_from_slice(b"$a\0$t\0$d\0");
        }
    }
}

/// `.shstrtab` holds section names.
#[derive(Debug)]
pub struct ShstrtabSection {
    pub hdr: ChunkHeader,
}

impl ShstrtabSection {
    pub fn new() -> ShstrtabSection {
        ShstrtabSection {
            hdr: ChunkHeader::new(".shstrtab", SHT_STRTAB, 0),
        }
    }
}

impl Default for ShstrtabSection {
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
            ctx.chunk_header_mut(id).shdr.sh_name = off as u32;
        }
        ctx.shstrtab.as_mut().unwrap().hdr.shdr.sh_size = offset;
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        buf[0] = 0;
        for &id in &ctx.chunks {
            let hdr = ctx.chunk_header(id);
            if hdr.shdr.sh_name != 0 {
                write_cstr(&mut buf[hdr.shdr.sh_name as usize..], hdr.name);
            }
        }
    }
}

/// `.dynstr` holds the strings the dynamic linker uses.
#[derive(Debug)]
pub struct DynstrSection {
    pub hdr: ChunkHeader,
    strings: HashMap<Vec<u8>, u64>,
}

impl DynstrSection {
    pub fn new() -> DynstrSection {
        DynstrSection {
            hdr: ChunkHeader::new(".dynstr", SHT_STRTAB, SHF_ALLOC as u64),
            strings: HashMap::new(),
        }
    }

    pub fn add_string(&mut self, s: &[u8]) -> u64 {
        if self.hdr.shdr.sh_size == 0 {
            self.strings.insert(Vec::new(), 0);
            self.hdr.shdr.sh_size = 1;
        }
        if let Some(&off) = self.strings.get(s) {
            return off;
        }
        let off = self.hdr.shdr.sh_size;
        self.strings.insert(s.to_vec(), off);
        self.hdr.shdr.sh_size += s.len() as u64 + 1;
        off
    }

    pub fn find_string(&self, s: &[u8]) -> u64 {
        *self.strings.get(s).expect("string not in .dynstr")
    }
}

impl Default for DynstrSection {
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

/// `.symtab` holds non-dynamic symbols, mainly for debugging.
#[derive(Debug)]
pub struct SymtabSection {
    pub hdr: ChunkHeader,
}

impl SymtabSection {
    pub fn new<E: Arch>() -> SymtabSection {
        let mut hdr = ChunkHeader::new(".symtab", SHT_SYMTAB, 0);
        hdr.shdr.sh_entsize = ElfSym::size::<E>() as u64;
        hdr.shdr.sh_addralign = E::WORD_SIZE as u64;
        SymtabSection { hdr }
    }
}

/// `.symtab_shndx` holds section indices too large for the 16-bit
/// `st_shndx` field. Most files don't need it.
#[derive(Debug)]
pub struct SymtabShndxSection {
    pub hdr: ChunkHeader,
}

impl SymtabShndxSection {
    pub fn new() -> SymtabShndxSection {
        let mut hdr = ChunkHeader::new(".symtab_shndx", SHT_SYMTAB_SHNDX, 0);
        hdr.shdr.sh_entsize = 4;
        hdr.shdr.sh_addralign = 4;
        SymtabShndxSection { hdr }
    }
}

impl Default for SymtabShndxSection {
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

        // File local symbols, then global symbols
        for file in &mut ctx.objs {
            file.base.local_symtab_idx = nsyms;
            nsyms += file.base.num_local_symtab;
        }
        for file in &mut ctx.objs {
            file.base.global_symtab_idx = nsyms;
            nsyms += file.base.num_global_symtab;
        }
        for file in &mut ctx.dsos {
            file.base.global_symtab_idx = nsyms;
            nsyms += file.base.num_global_symtab;
        }

        ctx.symtab.hdr.shdr.sh_info = ctx.objs.first().map_or(nsyms, |f| f.base.global_symtab_idx);
        ctx.symtab.hdr.shdr.sh_link = ctx.strtab.hdr.shndx;
        ctx.symtab.hdr.shdr.sh_size = if nsyms == 1 {
            0
        } else {
            nsyms as u64 * ElfSym::size::<E>() as u64
        };
    }

    /// Writes `.symtab`, `.strtab` and `.symtab_shndx`.
    pub fn copy_buf<E: Arch>(
        ctx: &Context<E>,
        symtab: &mut [u8],
        strtab: &mut [u8],
        mut xindex: Option<&mut [u8]>,
    ) {
        let size = ElfSym::size::<E>();
        symtab[..size].fill(0);
        if let Some(xindex) = xindex.as_deref_mut() {
            xindex.fill(0);
        }

        // Section symbols
        for &id in &ctx.chunks {
            let hdr = ctx.chunk_header(id);
            if hdr.shndx == 0 {
                continue;
            }
            let mut esym = ElfSym {
                st_info: STT_SECTION as u8,
                st_value: hdr.shdr.sh_addr,
                ..ElfSym::default()
            };
            match xindex.as_deref_mut() {
                Some(xindex) => {
                    E::Endian::write_u32(&mut xindex[hdr.shndx as usize * 4..], hdr.shndx);
                    esym.st_shndx = SHN_XINDEX as u16;
                }
                None => esym.st_shndx = hdr.shndx as u16,
            }
            esym.write::<E>(&mut symtab[hdr.shndx as usize * size..]);
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
                    Writer::Chunk(id) => chunks::populate_symtab(ctx, id, &mut block),
                    Writer::Obj(id) => {
                        ctx.objs[id.index()].populate_symtab(ctx, id, &mut block)
                    }
                    Writer::Dso(id) => {
                        ctx.dsos[id.index()].populate_symtab(ctx, id, &mut block)
                    }
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

/// RISC-V and LoongArch relaxation may have removed instructions from a
/// function, so its size must be recomputed.
fn symbol_size<E: Arch>(ctx: &Context<E>, sym: &Symbol) -> u64 {
    let esym = &sym.esym(ctx);
    if (E::IS_RISCV || E::IS_LOONGARCH) && esym.st_size != 0 {
        if let Some(isec) = sym.input_section_ref() {
            if isec.sh_flags & SHF_EXECINSTR as u64 != 0 {
                let end = esym.st_value + esym.st_size;
                return (esym.st_size as i64 + esym.st_value as i64
                    - sym.value as i64
                    - r_delta(isec, end)) as u64;
            }
        }
    }
    esym.st_size
}

/// Builds the output symbol table entry for a symbol. The returned index
/// is nonzero if the section index doesn't fit in `st_shndx` and must go
/// to `.symtab_shndx`.
pub fn to_output_esym<E: Arch>(ctx: &Context<E>, sym: &Symbol, st_name: u32) -> (ElfSym, u32) {
    let mut esym = ElfSym {
        st_name,
        st_size: symbol_size(ctx, sym),
        ..ElfSym::default()
    };
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
        Family::Arm64 | Family::RiscV => esym.st_other |= sym.esym(ctx).st_other & 0x80,
        Family::Ppc64V2 => esym.st_other |= sym.esym(ctx).st_other & 0xe0,
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
        shndx = Some(if sym.is_copyrel_readonly() {
            ctx.copyrel_relro.hdr.shndx
        } else {
            ctx.copyrel.hdr.shndx
        });
        esym.st_value = sym.addr(ctx);
    } else if file.is_dso() || sym.is_undef() {
        esym.st_shndx = SHN_UNDEF as u16;
        esym.st_size = 0;
        if sym.is_canonical() {
            esym.st_value = sym.plt_addr(ctx);
        }
    } else if let Some(chunk) = sym.output_chunk() {
        // Linker-synthesized symbol
        shndx = Some(ctx.chunk_header(chunk).shndx);
        esym.st_value = sym.addr(ctx);
    } else if let Some(frag) = sym.fragment() {
        shndx = Some(ctx.merged_sections[frag.section.index()].hdr.shndx);
        esym.st_value = sym.addr(ctx);
    } else if isec.is_none() {
        if sym.is_common() {
            // Common symbols are converted to .bss unless we're creating
            // a relocatable output. st_value of a common symbol is its
            // alignment.
            debug_assert!(ctx.args.relocatable);
            esym.st_shndx = SHN_COMMON as u16;
            esym.st_value = sym.esym(ctx).st_value;
        } else {
            esym.st_shndx = SHN_ABS as u16;
            esym.st_value = sym.addr(ctx);
        }
    } else if sym.ty() == STT_TLS {
        shndx = Some(st_shndx_of(sym));
        esym.st_value = sym.addr(ctx) - ctx.tls_begin;
    } else if sym.is_pde_ifunc(ctx) && sym.has_plt(&ctx.symbols) {
        // An IFUNC in a PDE uses two GOT slots and its PLT address.
        shndx = Some(st_shndx_of(sym));
        esym.set_type(STT_FUNC);
        esym.set_visibility(sym.visibility());
        esym.st_value = sym.plt_addr(ctx);
    } else if let Some(isec) = isec.filter(|isec| {
        isec.sh_flags & SHF_MERGE as u64 != 0 && isec.sh_flags & SHF_ALLOC as u64 == 0
    }) {
        // A symbol in a mergeable non-alloc section, such as .debug_str
        let file = &ctx.objs[isec.file.index()];
        let m = file
            .mergeable_section(file.shndx_at_in::<E>(sym.sym_idx as usize))
            .expect("mergeable section");
        let (frag, addend) = m.fragment(sym.esym(ctx).st_value).expect("fragment");
        let msec = &ctx.merged_sections[m.parent.index()];
        shndx = Some(msec.hdr.shndx);
        esym.set_visibility(sym.visibility());
        esym.st_value =
            (msec.hdr.shdr.sh_addr + msec.fragments.get(frag).offset()).wrapping_add(addend as u64);
    } else {
        shndx = Some(st_shndx_of(sym));
        esym.set_visibility(sym.visibility());
        esym.st_value = sym.addr_with(ctx, AddrFlags::NO_PLT);
    }

    // st_shndx is 16 bits; a large index goes to .symtab_shndx.
    let mut xindex = 0;
    if let Some(shndx) = shndx {
        if shndx < SHN_LORESERVE {
            esym.st_shndx = shndx as u16;
        } else {
            esym.st_shndx = SHN_XINDEX as u16;
            xindex = shndx;
        }
    }
    (esym, xindex)
}

/// `.dynsym` holds the symbols used for dynamic linking.
#[derive(Debug)]
pub struct DynsymSection {
    pub hdr: ChunkHeader,
    /// Index 0 is the null symbol.
    pub symbols: Vec<Option<SymbolId>>,
    pub dynstr_offset: u64,
}

impl DynsymSection {
    pub fn new<E: Arch>() -> DynsymSection {
        let mut hdr = ChunkHeader::new(".dynsym", SHT_DYNSYM, SHF_ALLOC as u64);
        hdr.shdr.sh_entsize = ElfSym::size::<E>() as u64;
        hdr.shdr.sh_addralign = E::WORD_SIZE as u64;
        DynsymSection {
            hdr,
            symbols: Vec::new(),
            dynstr_offset: 0,
        }
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
        ctx.dynsym.hdr.shdr.sh_link = ctx.dynstr.hdr.shndx;
        ctx.dynsym.hdr.shdr.sh_size = ElfSym::size::<E>() as u64 * ctx.dynsym.symbols.len() as u64;
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        let size = ElfSym::size::<E>();
        buf[..size].fill(0);
        let mut offset = ctx.dynsym.dynstr_offset as u32;
        for &id in ctx.dynsym.symbols.iter().skip(1).flatten() {
            let sym = &ctx.symbols[id];
            let (esym, xindex) = to_output_esym(ctx, sym, offset);
            if xindex != 0 {
                let nshdrs = ctx
                    .shdr
                    .as_ref()
                    .map_or(0, |s| s.hdr.shdr.sh_size / ElfShdr::size::<E>() as u64);
                error!(
                    ctx,
                    "{}: .dynsym: too many output sections: {nshdrs} requested, but ELF allows at most 65279",
                    ctx.args.output
                );
                return;
            }
            esym.write::<E>(&mut buf[sym.dynsym_idx(&ctx.symbols).unwrap() as usize * size..]);
            offset += sym.name().len() as u32 + 1;
        }
    }
}

/// The hash function for `.hash`.
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
pub fn djb_hash(name: &[u8]) -> u32 {
    let mut h: u32 = 5381;
    for &c in name {
        h = (h << 5).wrapping_add(h).wrapping_add(c as u32);
    }
    h
}

/// `.hash` is the classic on-disk hash table for `.dynsym`.
#[derive(Debug)]
pub struct HashSection {
    pub hdr: ChunkHeader,
}

impl HashSection {
    pub fn new<E: Arch>() -> HashSection {
        let mut hdr = ChunkHeader::new(".hash", SHT_HASH, SHF_ALLOC as u64);
        // s390x uses 64-bit entries; it looks like a spec bug but we
        // follow suit for compatibility.
        let entry = hash::entry_size::<E>() as u64;
        hdr.shdr.sh_entsize = entry;
        hdr.shdr.sh_addralign = entry;
        HashSection { hdr }
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
        hash.hdr.shdr.sh_size = entry * 2 + num_slots * entry * 2;
        hash.hdr.shdr.sh_link = ctx.dynsym.hdr.shndx;
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

/// `.gnu.hash` adds a bloom filter to speed up negative lookups.
#[derive(Debug)]
pub struct GnuHashSection {
    pub hdr: ChunkHeader,
    pub num_buckets: u32,
    pub num_bloom: u32,
    pub num_exported: u32,
}

impl GnuHashSection {
    pub const LOAD_FACTOR: u32 = 8;
    pub const HEADER_SIZE: u64 = 16;
    pub const BLOOM_SHIFT: u32 = 26;

    pub fn new<E: Arch>() -> GnuHashSection {
        let mut hdr = ChunkHeader::new(".gnu.hash", SHT_GNU_HASH, SHF_ALLOC as u64);
        hdr.shdr.sh_addralign = E::WORD_SIZE as u64;
        GnuHashSection {
            hdr,
            num_buckets: 0,
            num_bloom: 1,
            num_exported: 0,
        }
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
        // 12 bits per symbol in the bloom filter.
        gh.num_bloom = ((gh.num_exported as u64 * 12) / (word * 8))
            .max(1)
            .next_power_of_two() as u32;
        gh.hdr.shdr.sh_size = GnuHashSection::HEADER_SIZE
            + gh.num_bloom as u64 * word
            + gh.num_buckets as u64 * 4
            + gh.num_exported as u64 * 4;
        gh.hdr.shdr.sh_link = ctx.dynsym.hdr.shndx;
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        buf.fill(0);
        let gh = ctx.gnu_hash.as_ref().unwrap();
        let word = E::WORD_SIZE;
        let first_exported = ctx.dynsym.symbols.len() - gh.num_exported as usize;

        E::Endian::write_u32(buf, gh.num_buckets);
        E::Endian::write_u32(&mut buf[4..], first_exported as u32);
        E::Endian::write_u32(&mut buf[8..], gh.num_bloom);
        E::Endian::write_u32(&mut buf[12..], GnuHashSection::BLOOM_SHIFT);

        let syms: Vec<SymbolId> = ctx.dynsym.symbols[first_exported..]
            .iter()
            .flatten()
            .copied()
            .collect();
        if syms.is_empty() {
            return;
        }

        // Bloom filter
        let bloom_off = GnuHashSection::HEADER_SIZE as usize;
        let word_bits = word * 8;
        let mut indices = Vec::with_capacity(syms.len());
        for &id in &syms {
            let h = ctx.symbols[id].aux(&ctx.symbols).unwrap().djb_hash;
            indices.push(h % gh.num_buckets);
            let idx = (h as usize / word_bits) % gh.num_bloom as usize;
            let bits = (1u64 << (h as usize % word_bits))
                | (1u64 << ((h >> GnuHashSection::BLOOM_SHIFT) as usize % word_bits));
            let slot = &mut buf[bloom_off + idx * word..];
            if E::IS_64 {
                E::Endian::write_u64(slot, E::Endian::read_u64(slot) | bits);
            } else {
                E::Endian::write_u32(slot, E::Endian::read_u32(slot) | bits as u32);
            }
        }

        // Hash buckets
        let buckets_off = bloom_off + gh.num_bloom as usize * word;
        for (i, &bucket) in indices.iter().enumerate().rev() {
            E::Endian::write_u32(
                &mut buf[buckets_off + bucket as usize * 4..],
                (first_exported + i) as u32,
            );
        }

        // Hash values; the last entry of a chain has its LSB set.
        let table_off = buckets_off + gh.num_buckets as usize * 4;
        for (i, &id) in syms.iter().enumerate() {
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

/// Whether the symbol belongs to the output rather than a DSO.
pub fn is_defined_in_output(sym: &Symbol) -> bool {
    matches!(sym.file(), Some(FileId::Obj(_)))
}

/// The chunk id of `.symtab_shndx` if it exists.
pub fn symtab_shndx_id<E: Arch>(ctx: &Context<E>) -> Option<ChunkId> {
    ctx.symtab_shndx.as_ref().map(|_| ChunkId::SymtabShndx)
}
