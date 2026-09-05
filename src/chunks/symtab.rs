//! `.symtab`, the non-dynamic symbol table.

use rayon::prelude::*;

use crate::arch::{Arch, Family};
use crate::chunks::{self, strtab, ChunkHeader, ChunkId};
use crate::context::Context;
use crate::elf::*;
use crate::input_files::{SymtabBlock, SymtabEntries};
use crate::input_sections::{r_delta, InputSection};
use crate::symbol::{AddrFlags, OriginValue, Symbol};

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
                Writer::Chunk(id) => chunks::populate_symtab(ctx, id, &mut block),
                Writer::Obj(id) => ctx.objs[id.index()].populate_symtab(ctx, id, &mut block),
                Writer::Dso(id) => ctx.dsos[id.index()].populate_symtab(ctx, id, &mut block),
            }
        });
}

/// Splits `len` bytes starting at `start` off `rest`, which begins
/// at `pos` in the buffer; the pieces are taken in order.
fn carve<'a>(rest: &mut &'a mut [u8], pos: &mut usize, start: usize, len: usize) -> &'a mut [u8] {
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

// RISC-V and LoongArch have code-shrinking linker relaxation. If we
// have removed instructions from a function, we need to update its
// size as well.
fn symbol_size<E: Arch>(ctx: &Context<E>, sym: &Symbol) -> u64 {
    let esym = &sym.esym(ctx);
    if (E::IS_RISCV || E::IS_LOONGARCH) && esym.st_size().get() != 0 {
        if let Some(isec) = sym.input_section_ref(ctx) {
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

    let st_shndx_of = |sym: &Symbol, isec: &InputSection<E>| -> u32 {
        if E::FAMILY == Family::Ppc64V1 && sym.has_opd(&ctx.symbols) {
            return ctx.ppc64_opd.as_ref().unwrap().hdr.shndx;
        }
        if isec.is_alive() {
            return ctx.output_section(isec.output_section.unwrap()).hdr.shndx;
        }
        if isec.is_icf_removed() {
            let leader = ctx.section(isec.icf_leader().unwrap());
            return ctx.output_section(leader.output_section.unwrap()).hdr.shndx;
        }
        SHN_UNDEF
    };

    let mut shndx: Option<u32> = None;
    let origin = sym.origin::<E>();

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
    } else {
        match origin {
            OriginValue::OutputChunk(chunk) => {
                // Linker-synthesized symbol
                shndx = Some(chunk.shndx);
                esym.st_value_mut().set(sym.addr(ctx));
            }
            OriginValue::Fragment(frag) => {
                shndx = Some(ctx.merged_sections[frag.section.index()].hdr.shndx);
                esym.st_value_mut().set(sym.addr(ctx));
            }
            OriginValue::None | OriginValue::Symbol(_) => {
                if sym.is_common() {
                    // Common symbols are converted to .bss unless we are creating a
                    // relocatable output, in which case they are passed through as-is.
                    // Their st_value is their alignment.
                    debug_assert!(ctx.args.relocatable);
                    esym.st_shndx_mut().set(SHN_COMMON as u16);
                    esym.st_value_mut().set(sym.esym(ctx).st_value().get());
                } else {
                    // Absolute symbol
                    esym.st_shndx_mut().set(SHN_ABS as u16);
                    esym.st_value_mut().set(sym.addr(ctx));
                }
            }
            OriginValue::InputSection(section) => {
                let isec = ctx.input_section(section);
                if sym.ty() == STT_TLS {
                    // TLS symbol
                    shndx = Some(st_shndx_of(sym, isec));
                    esym.st_value_mut().set(sym.addr(ctx) - ctx.tls_begin);
                } else if sym.is_pde_ifunc(ctx) && sym.has_plt(&ctx.symbols) {
                    // IFUNC symbol in PDE that uses two GOT slots
                    shndx = Some(st_shndx_of(sym, isec));
                    esym.set_type(STT_FUNC);
                    esym.set_visibility(sym.visibility());
                    esym.st_value_mut().set(sym.plt_addr(ctx));
                } else if isec.sh_flags & SHF_MERGE as u64 != 0
                    && isec.sh_flags & SHF_ALLOC as u64 == 0
                {
                    // Symbol in a mergeable non-SHF_ALLOC section, such as .debug_str
                    let file = &ctx.objs[isec.file.index()];
                    let m = file
                        .merge_info(file.shndx_at_in(sym.sym_idx as usize))
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
                    shndx = Some(st_shndx_of(sym, isec));
                    esym.set_visibility(sym.visibility());
                    esym.st_value_mut()
                        .set(sym.addr_with(ctx, AddrFlags::NO_PLT));
                }
            }
        }
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
