//! `.got`, `.got.plt`, `.plt`, `.plt.got` and `.rela.plt`.

use crate::arch::{Arch, Family};
use crate::chunks::{ChunkHeader, DynRelBuffer};
use crate::context::Context;
use crate::elf::*;
use crate::input_files::SymtabBlock;
use crate::symbol::{AddrFlags, SymbolId};

/// `.got` is a linker-synthesized table of pointer-sized entries holding
/// runtime addresses of global variables and TP-relative offsets of
/// thread-local variables.
#[derive(Debug)]
pub struct GotSection {
    pub hdr: ChunkHeader,
    pub got_syms: Vec<SymbolId>,
    pub tlsgd_syms: Vec<SymbolId>,
    pub tlsdesc_syms: Vec<SymbolId>,
    pub gottp_syms: Vec<SymbolId>,
    pub tlsld_idx: Option<u32>,
}

impl GotSection {
    pub fn new<E: Arch>() -> GotSection {
        let mut hdr = ChunkHeader::new(".got", SHT_PROGBITS, (SHF_ALLOC | SHF_WRITE) as u64);
        hdr.is_relro = true;
        hdr.shdr.sh_addralign = E::WORD_SIZE as u64;
        // .got always exists so that _GLOBAL_OFFSET_TABLE_ has something
        // to point to. s390x reserves GOT[1] and GOT[2] as well.
        let reserved = if E::FAMILY == Family::S390x { 3 } else { 1 };
        hdr.shdr.sh_size = reserved * E::WORD_SIZE as u64;
        GotSection {
            hdr,
            got_syms: Vec::new(),
            tlsgd_syms: Vec::new(),
            tlsdesc_syms: Vec::new(),
            gottp_syms: Vec::new(),
            tlsld_idx: None,
        }
    }

    pub fn has_tlsld(&self) -> bool {
        self.tlsld_idx.is_some()
    }

    pub fn tlsld_addr<E: Arch>(&self) -> u64 {
        self.hdr.shdr.sh_addr + self.tlsld_idx.unwrap() as u64 * E::WORD_SIZE as u64
    }
}

// Named after the chunk, like the other chunks' modules in this directory.
#[allow(clippy::module_inception)]
pub mod got {
    use super::*;

    fn word<E: Arch>() -> u64 {
        E::WORD_SIZE as u64
    }

    pub fn add_got_symbol<E: Arch>(ctx: &mut Context<E>, sym: SymbolId) {
        let idx = (ctx.got.hdr.shdr.sh_size / word::<E>()) as u32;
        let is_pde_ifunc = ctx.symbols[sym].is_pde_ifunc(ctx);
        ctx.symbols.aux_mut(sym).got_idx = Some(idx);
        // An IFUNC in a position-dependent executable uses two GOT slots.
        ctx.got.hdr.shdr.sh_size += if is_pde_ifunc {
            2 * word::<E>()
        } else {
            word::<E>()
        };
        ctx.got.got_syms.push(sym);
    }

    pub fn add_gottp_symbol<E: Arch>(ctx: &mut Context<E>, sym: SymbolId) {
        let idx = (ctx.got.hdr.shdr.sh_size / word::<E>()) as u32;
        ctx.symbols.aux_mut(sym).gottp_idx = Some(idx);
        ctx.got.hdr.shdr.sh_size += word::<E>();
        ctx.got.gottp_syms.push(sym);
    }

    pub fn add_tlsgd_symbol<E: Arch>(ctx: &mut Context<E>, sym: SymbolId) {
        let idx = (ctx.got.hdr.shdr.sh_size / word::<E>()) as u32;
        ctx.symbols.aux_mut(sym).tlsgd_idx = Some(idx);
        ctx.got.hdr.shdr.sh_size += 2 * word::<E>();
        ctx.got.tlsgd_syms.push(sym);
    }

    /// TLSDESC GOT slots always get a dynamic relocation, since their
    /// values depend on the libc. A static executable has no dynamic
    /// relocations, so TLSDESC is always relaxed away there.
    pub fn add_tlsdesc_symbol<E: Arch>(ctx: &mut Context<E>, sym: SymbolId) {
        debug_assert!(E::SUPPORTS_TLSDESC);
        debug_assert!(!ctx.args.is_static);
        let idx = (ctx.got.hdr.shdr.sh_size / word::<E>()) as u32;
        ctx.symbols.aux_mut(sym).tlsdesc_idx = Some(idx);
        ctx.got.hdr.shdr.sh_size += 2 * word::<E>();
        ctx.got.tlsdesc_syms.push(sym);
    }

    pub fn add_tlsld<E: Arch>(ctx: &mut Context<E>) {
        debug_assert!(ctx.got.tlsld_idx.is_none());
        ctx.got.tlsld_idx = Some((ctx.got.hdr.shdr.sh_size / word::<E>()) as u32);
        ctx.got.hdr.shdr.sh_size += 2 * word::<E>();
    }

    struct GotEntry {
        idx: u32,
        val: u64,
        r_type: u32,
        sym: Option<SymbolId>,
    }

    /// Computes the GOT contents. An entry whose value is known at link
    /// time is filled in directly; otherwise a dynamic relocation lets the
    /// loader fill it.
    fn got_entries<E: Arch>(ctx: &Context<E>) -> Vec<GotEntry> {
        let mut entries = Vec::new();
        let mut add = |idx: u32, val: u64, r_type: u32, sym: Option<SymbolId>| {
            entries.push(GotEntry {
                idx,
                val,
                r_type,
                sym,
            });
        };
        let got = &ctx.got;

        for &id in &got.got_syms {
            let sym = &ctx.symbols[id];
            let idx = sym.got_idx(&ctx.symbols).unwrap();

            // IFUNC always needs to be fixed up by the dynamic linker.
            if let Some(r_irelative) = E::R_IRELATIVE {
                if sym.is_ifunc() {
                    if sym.is_pde_ifunc(ctx) {
                        add(idx, sym.plt_addr(ctx), R_NONE, None);
                        add(
                            idx + 1,
                            sym.addr_with(ctx, AddrFlags::NO_PLT),
                            r_irelative,
                            None,
                        );
                    } else {
                        add(
                            idx,
                            sym.addr_with(ctx, AddrFlags::NO_PLT),
                            r_irelative,
                            None,
                        );
                    }
                    continue;
                }
            }

            if sym.is_imported() {
                add(idx, 0, E::R_GLOB_DAT, Some(id));
            } else if ctx.args.pic && sym.is_relative() {
                add(
                    idx,
                    sym.addr_with(ctx, AddrFlags::NO_PLT),
                    E::R_RELATIVE,
                    None,
                );
            } else {
                add(idx, sym.addr_with(ctx, AddrFlags::NO_PLT), R_NONE, None);
            }
        }

        for &id in &got.tlsgd_syms {
            let sym = &ctx.symbols[id];
            let idx = sym.tlsgd_idx(&ctx.symbols).unwrap();
            if sym.is_imported() {
                add(idx, 0, E::R_DTPMOD, Some(id));
                add(idx + 1, 0, E::R_DTPOFF, Some(id));
            } else if ctx.args.shared {
                // The offset within the TLS block is known but the module
                // ID isn't.
                add(idx, 0, E::R_DTPMOD, None);
                add(
                    idx + 1,
                    sym.addr(ctx).wrapping_sub(ctx.dtp_addr),
                    R_NONE,
                    None,
                );
            } else {
                // Module ID 1 is the main executable.
                add(idx, 1, R_NONE, None);
                add(
                    idx + 1,
                    sym.addr(ctx).wrapping_sub(ctx.dtp_addr),
                    R_NONE,
                    None,
                );
            }
        }

        if let Some(r_tlsdesc) = E::R_TLSDESC {
            for &id in &got.tlsdesc_syms {
                let sym = &ctx.symbols[id];
                let idx = sym.tlsdesc_idx(&ctx.symbols).unwrap();
                if sym.is_imported() {
                    add(idx, 0, r_tlsdesc, Some(id));
                } else {
                    add(
                        idx,
                        sym.addr(ctx).wrapping_sub(ctx.tls_begin),
                        r_tlsdesc,
                        None,
                    );
                }
            }
        }

        for &id in &got.gottp_syms {
            let sym = &ctx.symbols[id];
            let idx = sym.gottp_idx(&ctx.symbols).unwrap();
            if sym.is_imported() {
                add(idx, 0, E::R_TPOFF, Some(id));
            } else if ctx.args.shared {
                add(
                    idx,
                    sym.addr(ctx).wrapping_sub(ctx.tls_begin),
                    E::R_TPOFF,
                    None,
                );
            } else {
                add(idx, sym.addr(ctx).wrapping_sub(ctx.tp_addr), R_NONE, None);
            }
        }

        if let Some(idx) = got.tlsld_idx {
            if ctx.args.shared {
                add(idx, 0, E::R_DTPMOD, None);
            } else {
                add(idx, 1, R_NONE, None);
            }
        }
        entries
    }

    /// Counts the dynamic relocations the GOT emits without computing
    /// entry values, which is too expensive for every layout iteration.
    /// Must mirror `got_entries`.
    pub fn num_dynrels<E: Arch>(ctx: &Context<E>) -> u64 {
        let got = &ctx.got;
        let mut n = 0;
        for &id in &got.got_syms {
            let sym = &ctx.symbols[id];
            if (E::SUPPORTS_IFUNC && sym.is_ifunc())
                || sym.is_imported()
                || (ctx.args.pic && sym.is_relative())
            {
                n += 1;
            }
        }
        for &id in &got.tlsgd_syms {
            let sym = &ctx.symbols[id];
            if sym.is_imported() {
                n += 2;
            } else if ctx.args.shared {
                n += 1;
            }
        }
        if E::SUPPORTS_TLSDESC {
            n += got.tlsdesc_syms.len() as u64;
        }
        for &id in &got.gottp_syms {
            if ctx.symbols[id].is_imported() || ctx.args.shared {
                n += 1;
            }
        }
        if got.tlsld_idx.is_some() && ctx.args.shared {
            n += 1;
        }
        n
    }

    pub fn relr_offsets<E: Arch>(ctx: &Context<E>) -> Vec<u64> {
        if !ctx.args.pic {
            return Vec::new();
        }
        ctx.got
            .got_syms
            .iter()
            .map(|&id| &ctx.symbols[id])
            .filter(|sym| !(E::SUPPORTS_IFUNC && sym.is_ifunc()))
            .filter(|sym| !sym.is_imported() && sym.is_relative())
            .map(|sym| sym.got_idx(&ctx.symbols).unwrap() as u64 * word::<E>())
            .collect()
    }

    pub fn write_dynrels<E: Arch>(ctx: &Context<E>, mut out: DynRelBuffer<'_, E>) {
        let mut i = 0;
        for ent in got_entries(ctx) {
            if ent.r_type == R_NONE {
                continue;
            }
            let rel = ElfRel::new(
                ctx.got.hdr.shdr.sh_addr + ent.idx as u64 * word::<E>(),
                ent.r_type,
                ent.sym
                    .and_then(|s| ctx.symbols[s].dynsym_idx(&ctx.symbols))
                    .unwrap_or(0),
                ent.val as i64,
            );
            let is_relr = rel.r_type == E::R_RELATIVE && rel.r_offset.is_multiple_of(word::<E>());
            if !ctx.args.pack_dyn_relocs_relr || ctx.got.hdr.num_relrs == 0 || !is_relr {
                out.write(i, rel);
                i += 1;
            }
        }
        debug_assert_eq!(i, out.len());
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        buf.fill(0);
        let w = word::<E>() as usize;
        let write = |buf: &mut [u8], idx: usize, val: u64| {
            if E::IS_64 {
                E::Endian::write_u64(&mut buf[idx * w..], val);
            } else {
                E::Endian::write_u32(&mut buf[idx * w..], val as u32);
            }
        };

        // s390x's psABI requires GOT[0] to hold the address of _DYNAMIC.
        // glibc's arm64 -static-pie code path wrongly assumes the same.
        if let Some(dynamic) = &ctx.dynamic {
            if E::FAMILY == Family::S390x
                || (E::FAMILY == Family::Arm64 && ctx.args.is_static && ctx.args.pie)
            {
                write(buf, 0, dynamic.hdr.shdr.sh_addr);
            }
        }

        for ent in got_entries(ctx) {
            let is_relr = ent.r_type == E::R_RELATIVE && ctx.args.pack_dyn_relocs_relr;
            if is_relr || ent.r_type == R_NONE {
                write(buf, ent.idx as usize, ent.val);
                continue;
            }
            if ctx.args.apply_dynamic_relocs {
                // A TLSDESC relocation fixes two consecutive slots: a
                // function pointer and its argument. The addend applies to
                // the argument, which is in the second slot, except on ARM32
                // which uses the inverted layout.
                let mut i = ent.idx as usize;
                if E::SUPPORTS_TLSDESC
                    && E::FAMILY != Family::Arm32
                    && Some(ent.r_type) == E::R_TLSDESC
                {
                    i += 1;
                }
                write(buf, i, ent.val);
            }
        }
    }

    pub fn compute_symtab_size<E: Arch>(ctx: &mut Context<E>) {
        let symbols = &ctx.symbols;
        let got = &mut ctx.got;
        got.hdr.strtab_size = 0;
        got.hdr.num_local_symtab = 0;
        let groups: [(&[SymbolId], &str); 4] = [
            (&got.got_syms, "$got"),
            (&got.gottp_syms, "$gottp"),
            (&got.tlsgd_syms, "$tlsgd"),
            (&got.tlsdesc_syms, "$tlsdesc"),
        ];
        let mut strtab_size = 0;
        let mut count = 0;
        for (syms, suffix) in groups {
            for &id in syms {
                strtab_size += symbols[id].name().len() as u64 + suffix.len() as u64 + 1;
                count += 1;
            }
        }
        if got.tlsld_idx.is_some() {
            strtab_size += "$tlsld".len() as u64 + 1;
            count += 1;
        }
        got.hdr.strtab_size = strtab_size;
        got.hdr.num_local_symtab = count;
    }

    pub fn populate_symtab<E: Arch>(ctx: &Context<E>, block: &mut SymtabBlock<'_>) {
        let got = &ctx.got;
        if got.hdr.num_local_symtab == 0 {
            return;
        }
        let object = |value: u64| ElfSym {
            st_info: STT_OBJECT as u8,
            st_shndx: got.hdr.shndx as u16,
            st_value: value,
            ..ElfSym::default()
        };
        for &id in &got.got_syms {
            let sym = &ctx.symbols[id];
            block.push_synthetic::<E>(sym.name(), b"$got", object(sym.got_addr(ctx)));
        }
        for &id in &got.gottp_syms {
            let sym = &ctx.symbols[id];
            block.push_synthetic::<E>(sym.name(), b"$gottp", object(sym.gottp_addr(ctx)));
        }
        for &id in &got.tlsgd_syms {
            let sym = &ctx.symbols[id];
            block.push_synthetic::<E>(sym.name(), b"$tlsgd", object(sym.tlsgd_addr(ctx)));
        }
        for &id in &got.tlsdesc_syms {
            let sym = &ctx.symbols[id];
            block.push_synthetic::<E>(sym.name(), b"$tlsdesc", object(sym.tlsdesc_addr(ctx)));
        }
        if got.tlsld_idx.is_some() {
            block.push_synthetic::<E>(b"", b"$tlsld", object(got.tlsld_addr::<E>()));
        }
    }
}

/// `.got.plt` holds the function pointers used by `.plt`.
#[derive(Debug)]
pub struct GotPltSection {
    pub hdr: ChunkHeader,
}

impl GotPltSection {
    pub fn new<E: Arch>(args: &crate::args::Args) -> GotPltSection {
        let sh_type = if E::IS_PPC64 {
            SHT_NOBITS
        } else {
            SHT_PROGBITS
        };
        let mut hdr = ChunkHeader::new(".got.plt", sh_type, (SHF_ALLOC | SHF_WRITE) as u64);
        hdr.is_relro = args.z_now;
        hdr.shdr.sh_addralign = E::WORD_SIZE as u64;
        hdr.shdr.sh_size = gotplt::header_size::<E>();
        GotPltSection { hdr }
    }
}

pub mod gotplt {
    use super::*;

    pub fn header_size<E: Arch>() -> u64 {
        let words = if E::FAMILY == Family::Ppc64V2 { 2 } else { 3 };
        words * E::WORD_SIZE as u64
    }

    pub fn entry_size<E: Arch>() -> u64 {
        let words = if E::FAMILY == Family::Ppc64V1 { 3 } else { 1 };
        words * E::WORD_SIZE as u64
    }

    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
        ctx.gotplt.hdr.shdr.sh_size =
            header_size::<E>() + ctx.plt.symbols.len() as u64 * entry_size::<E>();
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        // On PPC64 the loader fills .got.plt itself, finding the PLT
        // through DT_PPC64_GLINK.
        if E::IS_PPC64 {
            return;
        }
        let w = E::WORD_SIZE;
        let write = |buf: &mut [u8], idx: usize, val: u64| {
            if E::IS_64 {
                E::Endian::write_u64(&mut buf[idx * w..], val);
            } else {
                E::Endian::write_u32(&mut buf[idx * w..], val as u32);
            }
        };
        // The first slot points to _DYNAMIC as the psABI requests; the
        // next two are reserved for the loader.
        write(
            buf,
            0,
            ctx.dynamic.as_ref().map_or(0, |d| d.hdr.shdr.sh_addr),
        );
        write(buf, 1, 0);
        write(buf, 2, 0);
        for i in 0..ctx.plt.symbols.len() {
            write(buf, i + 3, ctx.plt.hdr.shdr.sh_addr);
        }
    }
}

/// `.plt` contains stubs that branch to the real function entry points,
/// used for lazy symbol resolution.
#[derive(Debug)]
pub struct PltSection {
    pub hdr: ChunkHeader,
    pub symbols: Vec<SymbolId>,
}

impl PltSection {
    pub fn new<E: Arch>() -> PltSection {
        let mut hdr = ChunkHeader::new(".plt", SHT_PROGBITS, (SHF_ALLOC | SHF_EXECINSTR) as u64);
        if E::IS_SPARC {
            hdr.shdr.sh_flags |= SHF_WRITE as u64;
            hdr.shdr.sh_addralign = 256;
        } else {
            hdr.shdr.sh_addralign = 16;
        }
        PltSection {
            hdr,
            symbols: Vec::new(),
        }
    }
}

pub mod plt {
    use super::*;

    /// SPARC uses 32-byte "small" PLT entries until the table grows past
    /// 0x100000 bytes, then switches to a "large" format.
    pub const SPARC_NUM_SMALL_PLT: u64 = (0x100000 - 128) / 32;

    /// The offset of a PLT entry within `.plt`.
    pub fn entry_offset<E: Arch>(idx: u32) -> u64 {
        let idx = idx as u64;
        match E::FAMILY {
            // PPC64 ELFv1 entries are 8 bytes for indices below 32768 and
            // 12 bytes beyond.
            Family::Ppc64V1 => {
                if idx < 0x8000 {
                    E::PLT_HDR_SIZE + idx * 8
                } else {
                    E::PLT_HDR_SIZE + 0x8000 * 8 + (idx - 0x8000) * 12
                }
            }
            // SPARC large entries come in blocks of 160 24-byte stubs
            // followed by 160 8-byte pointers.
            Family::Sparc64 => {
                if idx < SPARC_NUM_SMALL_PLT {
                    E::PLT_HDR_SIZE + idx * E::PLT_SIZE
                } else {
                    let i = idx - SPARC_NUM_SMALL_PLT;
                    0x100000 + (i / 160) * 5120 + (i % 160) * 24
                }
            }
            _ => E::PLT_HDR_SIZE + idx * E::PLT_SIZE,
        }
    }

    pub fn add_symbol<E: Arch>(ctx: &mut Context<E>, sym: SymbolId) {
        debug_assert!(!ctx.symbols[sym].has_plt(&ctx.symbols));
        let idx = ctx.plt.symbols.len() as u32;
        ctx.symbols.aux_mut(sym).plt_idx = Some(idx);
        ctx.plt.symbols.push(sym);
        crate::chunks::symtab::dynsym::add_symbol(ctx, sym);
    }

    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
        let n = ctx.plt.symbols.len() as u64;
        ctx.plt.hdr.shdr.sh_size = if n == 0 {
            0
        } else if E::IS_SPARC {
            E::PLT_HDR_SIZE + n * E::PLT_SIZE
        } else {
            entry_offset::<E>(n as u32)
        };
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        E::write_plt_header(ctx, buf);
        for (i, &id) in ctx.plt.symbols.iter().enumerate() {
            let off = entry_offset::<E>(i as u32) as usize;
            E::write_plt_entry(ctx, &mut buf[off..], &ctx.symbols[id]);
        }
    }

    pub fn compute_symtab_size<E: Arch>(ctx: &mut Context<E>) {
        let n = ctx.plt.symbols.len() as u32;
        let strtab_size: u64 = ctx
            .plt
            .symbols
            .iter()
            .map(|&id| ctx.symbols[id].name().len() as u64 + "$plt".len() as u64 + 1)
            .sum();
        ctx.plt.hdr.num_local_symtab = if E::FAMILY == Family::Arm32 {
            n * 3 + 2
        } else {
            n
        };
        ctx.plt.hdr.strtab_size = strtab_size;
    }

    pub fn populate_symtab<E: Arch>(ctx: &Context<E>, block: &mut SymtabBlock<'_>) {
        let plt = &ctx.plt;
        if plt.hdr.num_local_symtab == 0 {
            return;
        }
        let func = |addr: u64| ElfSym {
            st_info: STT_FUNC as u8,
            st_shndx: plt.hdr.shndx as u16,
            st_value: addr,
            ..ElfSym::default()
        };
        use crate::chunks::symtab::strtab::{ARM, DATA};
        if E::FAMILY == Family::Arm32 {
            block.push_mapping_symbol::<E>(ARM, func(plt.hdr.shdr.sh_addr));
            block.push_mapping_symbol::<E>(DATA, func(plt.hdr.shdr.sh_addr + 16));
        }
        for &id in &plt.symbols {
            let sym = &ctx.symbols[id];
            let addr = sym.plt_addr(ctx);
            block.push_synthetic::<E>(sym.name(), b"$plt", func(addr));
            if E::FAMILY == Family::Arm32 {
                block.push_mapping_symbol::<E>(ARM, func(addr));
                block.push_mapping_symbol::<E>(DATA, func(addr + 12));
            }
        }
    }
}

/// `.plt.got` is like `.plt` but for symbols that already have a GOT
/// entry; resolving them lazily through `.plt` would be pointless.
#[derive(Debug)]
pub struct PltGotSection {
    pub hdr: ChunkHeader,
    pub symbols: Vec<SymbolId>,
}

impl PltGotSection {
    pub fn new() -> PltGotSection {
        let mut hdr =
            ChunkHeader::new(".plt.got", SHT_PROGBITS, (SHF_ALLOC | SHF_EXECINSTR) as u64);
        hdr.shdr.sh_addralign = 16;
        PltGotSection {
            hdr,
            symbols: Vec::new(),
        }
    }
}

impl Default for PltGotSection {
    fn default() -> Self {
        Self::new()
    }
}

pub mod pltgot {
    use super::*;

    pub fn add_symbol<E: Arch>(ctx: &mut Context<E>, sym: SymbolId) {
        debug_assert!(!ctx.symbols[sym].has_plt(&ctx.symbols));
        debug_assert!(ctx.symbols[sym].has_got(&ctx.symbols));
        let idx = ctx.pltgot.symbols.len() as u32;
        ctx.symbols.aux_mut(sym).pltgot_idx = Some(idx);
        ctx.pltgot.symbols.push(sym);
        ctx.pltgot.hdr.shdr.sh_size = ctx.pltgot.symbols.len() as u64 * E::PLTGOT_SIZE;
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        for (i, &id) in ctx.pltgot.symbols.iter().enumerate() {
            let off = i * E::PLTGOT_SIZE as usize;
            E::write_pltgot_entry(ctx, &mut buf[off..], &ctx.symbols[id]);
        }
    }

    pub fn compute_symtab_size<E: Arch>(ctx: &mut Context<E>) {
        let n = ctx.pltgot.symbols.len() as u32;
        let strtab_size: u64 = ctx
            .pltgot
            .symbols
            .iter()
            .map(|&id| ctx.symbols[id].name().len() as u64 + "$pltgot".len() as u64 + 1)
            .sum();
        ctx.pltgot.hdr.num_local_symtab = if E::FAMILY == Family::Arm32 { n * 3 } else { n };
        ctx.pltgot.hdr.strtab_size = strtab_size;
    }

    pub fn populate_symtab<E: Arch>(ctx: &Context<E>, block: &mut SymtabBlock<'_>) {
        let pltgot = &ctx.pltgot;
        if pltgot.hdr.num_local_symtab == 0 {
            return;
        }
        let func = |addr: u64| ElfSym {
            st_info: STT_FUNC as u8,
            st_shndx: pltgot.hdr.shndx as u16,
            st_value: addr,
            ..ElfSym::default()
        };
        use crate::chunks::symtab::strtab::{ARM, DATA};
        for &id in &pltgot.symbols {
            let sym = &ctx.symbols[id];
            let addr = sym.plt_addr(ctx);
            block.push_synthetic::<E>(sym.name(), b"$pltgot", func(addr));
            if E::FAMILY == Family::Arm32 {
                block.push_mapping_symbol::<E>(ARM, func(addr));
                block.push_mapping_symbol::<E>(DATA, func(addr + 12));
            }
        }
    }
}

/// `.rela.plt` holds the relocations for `.plt`.
#[derive(Debug)]
pub struct RelPltSection {
    pub hdr: ChunkHeader,
}

impl RelPltSection {
    pub fn new<E: Arch>() -> RelPltSection {
        let (name, ty) = if E::IS_RELA {
            (".rela.plt", SHT_RELA)
        } else {
            (".rel.plt", SHT_REL)
        };
        let mut hdr = ChunkHeader::new(name, ty, SHF_ALLOC as u64);
        hdr.shdr.sh_entsize = ElfRel::size::<E>() as u64;
        hdr.shdr.sh_addralign = E::WORD_SIZE as u64;
        RelPltSection { hdr }
    }
}

pub mod relplt {
    use super::*;

    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
        ctx.relplt.hdr.shdr.sh_size = ctx.plt.symbols.len() as u64 * ElfRel::size::<E>() as u64;
        ctx.relplt.hdr.shdr.sh_link = ctx.dynsym.hdr.shndx;
        if !E::IS_SPARC {
            ctx.relplt.hdr.shdr.sh_info = ctx.gotplt.hdr.shndx;
        }
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        let size = ElfRel::size::<E>();
        for (i, &id) in ctx.plt.symbols.iter().enumerate() {
            let sym = &ctx.symbols[id];
            let rel = if E::IS_SPARC {
                // SPARC has no .got.plt: its .plt is writable and the
                // loader patches the stubs in place.
                let idx = sym.plt_idx(&ctx.symbols).unwrap() as u64;
                if idx < plt::SPARC_NUM_SMALL_PLT {
                    ElfRel::new(
                        sym.plt_addr(ctx),
                        E::R_JUMP_SLOT,
                        sym.dynsym_idx(&ctx.symbols).unwrap_or(0),
                        0,
                    )
                } else {
                    let call = sym.plt_addr(ctx) + 4;
                    let ptr = ctx.plt.hdr.shdr.sh_addr
                        + crate::arch::sparc64::plt_ptr_offset(ctx.plt.symbols.len(), idx);
                    ElfRel::new(
                        ptr,
                        E::R_JUMP_SLOT,
                        sym.dynsym_idx(&ctx.symbols).unwrap_or(0),
                        -(call as i64),
                    )
                }
            } else {
                ElfRel::new(
                    sym.gotplt_addr(ctx),
                    E::R_JUMP_SLOT,
                    sym.dynsym_idx(&ctx.symbols).unwrap_or(0),
                    0,
                )
            };
            rel.write::<E>(&mut buf[i * size..]);
        }
    }
}
