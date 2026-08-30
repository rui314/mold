//! `.rela.dyn`, `.relr.dyn` and `.dynamic`.

use rayon::prelude::*;

use crate::arch::{Arch, Family};
use crate::chunks::{self, ChunkHeader, ChunkId};
use crate::context::Context;
use crate::elf::*;
use crate::input_files::FileId;
use crate::symbol::SymbolId;
use crate::util::encode_sleb;

// .rel.dyn contains relocation infromation for other sections.
#[derive(Debug)]
pub struct RelDynSection {
    pub hdr: ChunkHeader,
    pub android_encoded: Vec<u8>,
    pub keep_android_size: bool,
}

impl RelDynSection {
    pub fn new<E: Arch>(args: &crate::args::Args) -> RelDynSection {
        let name = if E::IS_RELA { ".rela.dyn" } else { ".rel.dyn" };
        let mut hdr = ChunkHeader::new(name, 0, SHF_ALLOC as u64);
        if args.pack_dyn_relocs_android {
            hdr.shdr.sh_type = if E::IS_RELA {
                SHT_ANDROID_RELA
            } else {
                SHT_ANDROID_REL
            };
            hdr.shdr.sh_entsize = 0;
            hdr.shdr.sh_addralign = 1;
        } else {
            hdr.shdr.sh_type = if E::IS_RELA { SHT_RELA } else { SHT_REL };
            hdr.shdr.sh_entsize = ElfRel::size::<E>() as u64;
            hdr.shdr.sh_addralign = E::WORD_SIZE as u64;
        }
        RelDynSection {
            hdr,
            android_encoded: Vec::new(),
            keep_android_size: false,
        }
    }
}

pub mod reldyn {
    use super::*;

    /// Gathers the dynamic relocations of all chunks.
    pub fn collect_relocs<E: Arch>(ctx: &Context<E>) -> Vec<ElfRel> {
        let count: usize = ctx
            .chunks
            .iter()
            .map(|&id| {
                let hdr = ctx.chunk_header(id);
                (hdr.num_dynrels - hdr.num_relrs) as usize
            })
            .sum();
        let mut out = vec![ElfRel::default(); count];
        let mut rest = out.as_mut_slice();
        for &id in &ctx.chunks {
            let hdr = ctx.chunk_header(id);
            let count = (hdr.num_dynrels - hdr.num_relrs) as usize;
            if count != 0 {
                let (slots, tail) = std::mem::take(&mut rest).split_at_mut(count);
                chunks::write_dynrels(ctx, id, chunks::DynRelBuffer::native(slots));
                rest = tail;
            }
        }
        debug_assert!(rest.is_empty());
        out
    }

    /// Encodes base relocations of each chunk in RELR form, using offsets
    /// relative to the chunk.
    pub fn construct_relr<E: Arch>(ctx: &mut Context<E>) {
        debug_assert!(ctx.args.pack_dyn_relocs_relr);
        let word = E::WORD_SIZE as u64;
        let ids = ctx.chunks.clone();

        for &id in &ids {
            let n = chunks::num_dynrels(ctx, id);
            let hdr = ctx.chunk_header_mut(id);
            hdr.num_dynrels = n;
            hdr.num_relrs = 0;
            hdr.relr.clear();

            // Do not use RELR for executable chunks, as they don't usually contain
            // base relocations.
            if hdr.shdr.sh_flags & SHF_EXECINSTR as u64 != 0 {
                continue;
            }
            // --section-start can override a chunk's alignment. Conservatively use
            // .rel[a].dyn if the explicitly assigned address is not word-aligned.
            let name = String::from_utf8_lossy(hdr.name).into_owned();
            if ctx
                .args
                .section_start
                .get(&name)
                .is_some_and(|&addr| addr % word != 0)
            {
                continue;
            }
            if n != 0 {
                let offsets = chunks::relr_offsets(ctx, id);
                let hdr = ctx.chunk_header_mut(id);
                hdr.num_relrs = offsets.len() as u64;
                debug_assert!(hdr.num_relrs <= hdr.num_dynrels);
                hdr.relr = encode_relr::<E>(&offsets);
            }
        }

        let size: u64 = ids
            .iter()
            .map(|&id| ctx.chunk_header(id).relr.len() as u64 * word)
            .sum();
        if let Some(relrdyn) = &mut ctx.relrdyn {
            relrdyn.hdr.shdr.sh_size = size;
        }
    }

    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
        let ids = ctx.chunks.clone();
        if !ctx.args.pack_dyn_relocs_relr {
            for &id in &ids {
                let n = chunks::num_dynrels(ctx, id);
                let hdr = ctx.chunk_header_mut(id);
                hdr.num_dynrels = n;
                hdr.num_relrs = 0;
            }
        }

        let mut num_relocs = 0;
        let mut num_relrs = 0;
        for &id in &ids {
            let hdr = ctx.chunk_header(id);
            num_relocs += hdr.num_dynrels;
            num_relrs += hdr.num_relrs;
        }

        if ctx.args.pack_dyn_relocs_android {
            let relocs = collect_relocs(ctx);
            // APS2 uses SLEB128-encoded deltas, so .rela.dyn size may oscillate
            // as addresses move. If a shrink is followed by a growth, stop
            // shrinking and pad the encoded stream to converge.
            let encoded = encode_android::<E>(relocs);
            let old_size = ctx.reldyn.hdr.shdr.sh_size as usize;
            let reldyn = &mut ctx.reldyn;
            if old_size != 0 && old_size < encoded.len() {
                reldyn.keep_android_size = true;
            }
            reldyn.android_encoded = encoded;
            if reldyn.keep_android_size && reldyn.android_encoded.len() < old_size {
                reldyn.android_encoded.resize(old_size, 0);
            }
            reldyn.hdr.shdr.sh_size = reldyn.android_encoded.len() as u64;
        } else {
            ctx.reldyn.hdr.shdr.sh_size = (num_relocs - num_relrs) * ElfRel::size::<E>() as u64;
        }
        ctx.reldyn.hdr.shdr.sh_link = ctx.dynsym.hdr.shndx;
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        if ctx.args.pack_dyn_relocs_android {
            buf[..ctx.reldyn.android_encoded.len()].copy_from_slice(&ctx.reldyn.android_encoded);
        } else {
            let size = ElfRel::size::<E>();
            let mut rest = buf;
            for &id in &ctx.chunks {
                let hdr = ctx.chunk_header(id);
                let count = (hdr.num_dynrels - hdr.num_relrs) as usize;
                if count != 0 {
                    let (slots, tail) = std::mem::take(&mut rest).split_at_mut(count * size);
                    chunks::write_dynrels(ctx, id, chunks::DynRelBuffer::output(slots));
                    rest = tail;
                }
            }
            debug_assert!(rest.is_empty());
        }
    }

    // Sort dynamic relocations. This is the reason why we do it.
    // Quote from https://www.airs.com/blog/archives/186
    //
    //   The dynamic linker in glibc uses a one element cache when processing
    //   relocs: if a relocation refers to the same symbol as the previous
    //   relocation, then the dynamic linker reuses the value rather than
    //   looking up the symbol again. Thus the dynamic linker gets the best
    //   results if the dynamic relocations are sorted so that all dynamic
    //   relocations for a given dynamic symbol are adjacent.
    //
    //   Other than that, the linker sorts together all relative relocations,
    //   which don't have symbols. Two relative relocations, or two relocations
    //   against the same symbol, are sorted by the address in the output
    //   file. This tends to optimize paging and caching when there are two
    //   references from the same page.
    pub fn sort<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        // .rela.dyn contains APS2-encoded bytes, not ElfRel entries.
        if ctx.args.pack_dyn_relocs_android {
            return;
        }

        // We group IFUNC relocations at the end of .rel.dyn because we want to
        // apply all the other relocations before running user-supplied IFUNC
        // resolvers.
        let rank = |r_type: u32| -> u32 {
            if r_type == E::R_RELATIVE {
                0
            } else if Some(r_type) == E::R_IRELATIVE {
                2
            } else {
                1
            }
        };
        match chunks::DynRelBuffer::<E>::output(buf) {
            chunks::DynRelBuffer::Native(relocs) => {
                relocs.par_sort_by_key(|r| (rank(r.r_type), r.r_sym, r.r_offset));
            }
            chunks::DynRelBuffer::Encoded(buf, _) => {
                let mut relocs = ElfRel::parse_all::<E>(buf);
                relocs.par_sort_by_key(|r| (rank(r.r_type), r.r_sym, r.r_offset));
                ElfRel::write_all::<E>(&relocs, buf);
            }
        }
    }
}

// .relr.dyn is a relatively new section to contain base relocation
// information.
//
// A relocatable executable/DSO contains a lot of certain type of
// relocation entries, called "base relocations", to specify the locations
// of pointers in the FILE that need to be adjusted according to the
// desired load address and the actual load address. As an example,
// consider the following C code.
//
// extern int foo;
// int *bar = &foo;
//
// If an executable containing the above code is built as relocatable
// executable, meaning that the executable can be loaded not to a specific
// address in memory but anywhere in the virtual address space, then the
// pointer `bar`'s address is not known at link-time.
//
// The linker temporarily links the executable to a base address, record
// that information to the ELF header, and emits dynamic relocations to
// refer to the location of `bar`. At runtime, the loader adds the
// difference of the expected load address and the actual one to the
// pointer value to fix the pointer value.
//
// Relocatable executables/DSOs usually contain a fairly large number of
// base relocations. In particular, C++ virtual function table is an array
// of statically-initialized pointers which need base relocations.
//
// Notice that base relocations don't contain symbol information. They
// need only pointer locations in the ELF file that need fixing at
// load-time. Therefore, storing that information to the usual ELF
// relocation table is waste of space.
//
// .relr.dyn is designed to store base relocations in a space-efficient way.
#[derive(Debug)]
pub struct RelrDynSection {
    pub hdr: ChunkHeader,
}

impl RelrDynSection {
    pub fn new<E: Arch>(args: &crate::args::Args) -> RelrDynSection {
        let ty = if args.use_android_relr_tags {
            SHT_ANDROID_RELR
        } else {
            SHT_RELR
        };
        let mut hdr = ChunkHeader::new(".relr.dyn", ty, SHF_ALLOC as u64);
        hdr.shdr.sh_entsize = E::WORD_SIZE as u64;
        hdr.shdr.sh_addralign = E::WORD_SIZE as u64;
        RelrDynSection { hdr }
    }
}

pub mod relrdyn {
    use super::*;

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        let w = E::WORD_SIZE;
        let mut i = 0;
        for &id in &ctx.chunks {
            let hdr = ctx.chunk_header(id);
            for &val in &hdr.relr {
                let v = if val & 1 != 0 {
                    val
                } else {
                    hdr.shdr.sh_addr + val
                };
                if E::IS_64 {
                    E::Endian::write_u64(&mut buf[i * w..], v);
                } else {
                    E::Endian::write_u32(&mut buf[i * w..], v as u32);
                }
                i += 1;
            }
        }
    }
}

// .relr.dyn contains base relocations encoded in a space-efficient form.
// The contents of the section is essentially just a list of addresses
// that have to be fixed up at runtime.
//
// Here is the encoding scheme (we assume 64-bit ELF in this description
// for the sake of simplicity): .relr.dyn contains zero or more address
// groups. Each address group consists of a 64-bit start address followed
// by zero or more 63-bit bitmaps. Let A be the address of a start
// address. Then, the loader fixes address A. If Nth bit in the following
// bitmap is on, the loader also fixes address A + N * 8. In this scheme,
// one address and one bitmap can represent up to 64 base relocations in a
// 512 bytes range.
//
// A start address and a bitmap is distinguished by the lowest significant
// bit. An address must be even and thus its LSB is 0 (odd address is not
// representable in this encoding and such relocation must be stored to
// the .rel.dyn section). A bitmap has LSB 1.
pub fn encode_relr<E: Arch>(offsets: &[u64]) -> Vec<u64> {
    let word = E::WORD_SIZE as u64;
    let num_bits = if E::IS_64 { 63 } else { 31 };
    let max_delta = word * num_bits;
    let mut vec = Vec::new();
    let mut i = 0;

    while i < offsets.len() {
        let first = offsets[i];
        vec.push(first);
        let mut base = first + word;
        i += 1;
        loop {
            let mut bits = 0u64;
            while i < offsets.len() && offsets[i] - base < max_delta {
                bits |= 1 << ((offsets[i] - base) / word);
                i += 1;
            }
            if bits == 0 {
                break;
            }
            vec.push((bits << 1) | 1);
            base += max_delta;
        }
    }
    vec
}

// Encode dynamic relocations using the Android Packed Relocation format
// (APS2). The encoded stream begins with the magic bytes "APS2" followed
// by SLEB128-encoded fields. Relocations are emitted in groups; within a
// group, common offset deltas, info values, and addend deltas are
// factored out so each per-relocation entry is just the differing fields.
//
// See bionic's linker/linker_relocs.cpp for the decoder.
pub fn encode_android<E: Arch>(mut rels: Vec<ElfRel>) -> Vec<u8> {
    const GROUPED_BY_INFO: i64 = 1;
    const GROUPED_BY_OFFSET_DELTA: i64 = 2;
    const GROUP_HAS_ADDEND: i64 = 8;

    let r_info = |r: &ElfRel| -> i64 {
        if E::IS_64 {
            (((r.r_sym as u64) << 32) | r.r_type as u64) as i64
        } else {
            (((r.r_sym as u64) << 8) | (r.r_type & 0xff) as u64) as i64
        }
    };

    let mut buf = b"APS2".to_vec();
    encode_sleb(&mut buf, rels.len() as i64);
    encode_sleb(&mut buf, 0); // initial offset state
    if rels.is_empty() {
        return buf;
    }

    // APS2 offset deltas are signed, so the format doesn't require
    // relocations to be in increasing-offset order. Sort by (r_type,
    // r_sym, r_offset) so all relocs of the dominant type (typically
    // R_RELATIVE, ~90% of dynrels in a real Android binary) land in one
    // contiguous block. That collapses dozens of type-broken groups into
    // a few large info-grouped runs.
    rels.sort_by_key(|r| (r.r_type, r.r_sym, r.r_offset));

    let mut prev_offset = 0i64;
    let mut prev_addend = 0i64;
    let mut i = 0;
    while i < rels.len() {
        let offset_delta = rels[i].r_offset as i64 - prev_offset;
        let cur_info = r_info(&rels[i]);

        // Greedily extend the group while consecutive relocations share the
        // same offset_delta and r_info.
        let mut j = i + 1;
        while j < rels.len()
            && r_info(&rels[j]) == cur_info
            && rels[j].r_offset as i64 - rels[j - 1].r_offset as i64 == offset_delta
        {
            j += 1;
        }

        let size = (j - i) as i64;
        let mut flags = 0;
        if size > 1 {
            flags = GROUPED_BY_INFO | GROUPED_BY_OFFSET_DELTA;
        }
        if E::IS_RELA {
            flags |= GROUP_HAS_ADDEND;
        }
        encode_sleb(&mut buf, size);
        encode_sleb(&mut buf, flags);
        encode_sleb(&mut buf, offset_delta);
        encode_sleb(&mut buf, cur_info);

        if E::IS_RELA {
            for r in &rels[i..j] {
                encode_sleb(&mut buf, r.r_addend - prev_addend);
                prev_addend = r.r_addend;
            }
        }
        prev_offset = rels[j - 1].r_offset as i64;
        i = j;
    }
    buf
}

// .dynamic contains various information for dynamically-linked ELF files.
// At runtime, the dynamic linker reads the information to work
// appropriately.
#[derive(Debug)]
pub struct DynamicSection {
    pub hdr: ChunkHeader,
}

impl DynamicSection {
    pub fn new<E: Arch>(args: &crate::args::Args) -> DynamicSection {
        let mut hdr = ChunkHeader::new(".dynamic", SHT_DYNAMIC, 0);
        hdr.shdr.sh_addralign = E::WORD_SIZE as u64;
        hdr.shdr.sh_entsize = ElfDyn::size::<E>() as u64;
        if args.z_rodynamic {
            hdr.shdr.sh_flags = SHF_ALLOC as u64;
            hdr.is_relro = false;
        } else {
            hdr.shdr.sh_flags = (SHF_ALLOC | SHF_WRITE) as u64;
            hdr.is_relro = true;
        }
        DynamicSection { hdr }
    }
}

// Named after the chunk, like the other chunks' modules in this directory.
#[allow(clippy::module_inception)]
pub mod dynamic {
    use super::*;

    // An ARM64 function with a non-standard calling convention is marked with
    // STO_AARCH64_VARIANT_PCS bit in the symbol table.
    //
    // A function with that bit is not safe to be called through a lazy PLT
    // stub because the PLT resolver may clobber registers that should be
    // preserved in a non-standard calling convention.
    //
    // To solve the problem, the dynamic linker scans the dynamic symbol table
    // at process startup time and resolve symbols with STO_AARCH64_VARIANT_PCS
    // bit eagerly, so that the PLT resolver won't be called for that symbol
    // lazily. As an optimization, it does so only when DT_AARCH64_VARIANT_PCS
    // is set in the dynamic section.
    //
    // This function returns true if DT_AARCH64_VARIANT_PCS needs to be set.
    fn contains_variant_pcs<E: Arch>(ctx: &Context<E>) -> bool {
        ctx.plt
            .symbols
            .iter()
            .any(|&id| ctx.symbols[id].esym(ctx).arm64_variant_pcs())
    }

    fn sym_addr_if_defined<E: Arch>(ctx: &Context<E>, id: SymbolId) -> Option<u64> {
        let sym = &ctx.symbols[id];
        match sym.file() {
            Some(FileId::Obj(_)) => Some(sym.addr(ctx)),
            _ => None,
        }
    }

    fn create_contents<E: Arch>(ctx: &Context<E>) -> Vec<(u64, u64)> {
        let mut vec: Vec<(u64, u64)> = Vec::new();
        let mut define = |tag: u32, val: u64| vec.push((tag as u64, val));
        let dynstr = &ctx.dynstr;

        for dso in &ctx.dsos {
            define(DT_NEEDED, dynstr.find_string(dso.soname.as_bytes()));
        }
        if !ctx.args.rpaths.is_empty() {
            let tag = if ctx.args.enable_new_dtags {
                DT_RUNPATH
            } else {
                DT_RPATH
            };
            define(tag, dynstr.find_string(ctx.args.rpaths.as_bytes()));
        }
        if !ctx.args.soname.is_empty() {
            define(DT_SONAME, dynstr.find_string(ctx.args.soname.as_bytes()));
        }
        for s in &ctx.args.auxiliary {
            define(DT_AUXILIARY, dynstr.find_string(s.as_bytes()));
        }
        if !ctx.args.audit.is_empty() {
            define(DT_AUDIT, dynstr.find_string(ctx.args.audit.as_bytes()));
        }
        if !ctx.args.depaudit.is_empty() {
            define(
                DT_DEPAUDIT,
                dynstr.find_string(ctx.args.depaudit.as_bytes()),
            );
        }
        for s in &ctx.args.filter {
            define(DT_FILTER, dynstr.find_string(s.as_bytes()));
        }

        if ctx.reldyn.hdr.shdr.sh_size != 0 {
            if ctx.args.pack_dyn_relocs_android {
                define(
                    if E::IS_RELA {
                        DT_ANDROID_RELA
                    } else {
                        DT_ANDROID_REL
                    },
                    ctx.reldyn.hdr.shdr.sh_addr,
                );
                define(
                    if E::IS_RELA {
                        DT_ANDROID_RELASZ
                    } else {
                        DT_ANDROID_RELSZ
                    },
                    ctx.reldyn.hdr.shdr.sh_size,
                );
            } else {
                define(
                    if E::IS_RELA { DT_RELA } else { DT_REL },
                    ctx.reldyn.hdr.shdr.sh_addr,
                );
                define(
                    if E::IS_RELA { DT_RELASZ } else { DT_RELSZ },
                    ctx.reldyn.hdr.shdr.sh_size,
                );
                define(
                    if E::IS_RELA { DT_RELAENT } else { DT_RELENT },
                    ElfRel::size::<E>() as u64,
                );
            }
        }

        if let Some(relrdyn) = &ctx.relrdyn {
            if ctx.args.use_android_relr_tags {
                define(DT_ANDROID_RELR, relrdyn.hdr.shdr.sh_addr);
                define(DT_ANDROID_RELRSZ, relrdyn.hdr.shdr.sh_size);
                define(DT_ANDROID_RELRENT, relrdyn.hdr.shdr.sh_entsize);
            } else {
                define(DT_RELR, relrdyn.hdr.shdr.sh_addr);
                define(DT_RELRSZ, relrdyn.hdr.shdr.sh_size);
                define(DT_RELRENT, relrdyn.hdr.shdr.sh_entsize);
            }
        }

        if ctx.relplt.hdr.shdr.sh_size != 0 {
            define(DT_JMPREL, ctx.relplt.hdr.shdr.sh_addr);
            define(DT_PLTRELSZ, ctx.relplt.hdr.shdr.sh_size);
            define(DT_PLTREL, if E::IS_RELA { DT_RELA } else { DT_REL } as u64);
        }

        if E::IS_SPARC {
            if ctx.plt.hdr.shdr.sh_size != 0 {
                define(DT_PLTGOT, ctx.plt.hdr.shdr.sh_addr);
            }
        } else if E::FAMILY == Family::Ppc32 {
            if ctx.gotplt.hdr.shdr.sh_size != 0 {
                define(
                    DT_PLTGOT,
                    ctx.gotplt.hdr.shdr.sh_addr + crate::chunks::got::gotplt::header_size::<E>(),
                );
            }
        } else if ctx.gotplt.hdr.shdr.sh_size != 0 {
            define(DT_PLTGOT, ctx.gotplt.hdr.shdr.sh_addr);
        }

        if ctx.dynsym.hdr.shdr.sh_size != 0 {
            define(DT_SYMTAB, ctx.dynsym.hdr.shdr.sh_addr);
            define(DT_SYMENT, ElfSym::size::<E>() as u64);
        }
        if ctx.dynstr.hdr.shdr.sh_size != 0 {
            define(DT_STRTAB, ctx.dynstr.hdr.shdr.sh_addr);
            define(DT_STRSZ, ctx.dynstr.hdr.shdr.sh_size);
        }

        let value = |id: Option<SymbolId>| id.map_or(0, |id| ctx.symbols[id].value);
        if ctx.find_chunk_by_type(SHT_INIT_ARRAY).is_some() {
            let start = value(ctx.syms.init_array_start);
            define(DT_INIT_ARRAY, start);
            define(DT_INIT_ARRAYSZ, value(ctx.syms.init_array_end) - start);
        }
        if ctx.find_chunk_by_type(SHT_PREINIT_ARRAY).is_some() {
            let start = value(ctx.syms.preinit_array_start);
            define(DT_PREINIT_ARRAY, start);
            define(
                DT_PREINIT_ARRAYSZ,
                value(ctx.syms.preinit_array_end) - start,
            );
        }
        if ctx.find_chunk_by_type(SHT_FINI_ARRAY).is_some() {
            let start = value(ctx.syms.fini_array_start);
            define(DT_FINI_ARRAY, start);
            define(DT_FINI_ARRAYSZ, value(ctx.syms.fini_array_end) - start);
        }

        if ctx.versym.hdr.shdr.sh_size != 0 {
            define(DT_VERSYM, ctx.versym.hdr.shdr.sh_addr);
        }
        if ctx.verneed.hdr.shdr.sh_size != 0 {
            define(DT_VERNEED, ctx.verneed.hdr.shdr.sh_addr);
            define(DT_VERNEEDNUM, ctx.verneed.hdr.shdr.sh_info as u64);
        }
        if let Some(verdef) = &ctx.verdef {
            define(DT_VERDEF, verdef.hdr.shdr.sh_addr);
            define(DT_VERDEFNUM, verdef.hdr.shdr.sh_info as u64);
        }

        if let Some(addr) = sym_addr_if_defined(ctx, ctx.syms.init) {
            define(DT_INIT, addr);
        }
        if let Some(addr) = sym_addr_if_defined(ctx, ctx.syms.fini) {
            define(DT_FINI, addr);
        }

        if let Some(hash) = &ctx.hash {
            define(DT_HASH, hash.hdr.shdr.sh_addr);
        }
        if let Some(gnu_hash) = &ctx.gnu_hash {
            define(DT_GNU_HASH, gnu_hash.hdr.shdr.sh_addr);
        }
        let has_textrel = ctx.has_textrel.load(std::sync::atomic::Ordering::Relaxed);
        if has_textrel {
            define(DT_TEXTREL, 0);
        }

        let mut flags = 0u64;
        let mut flags1 = 0u64;
        if ctx.args.pie {
            flags1 |= DF_1_PIE as u64;
        }
        if ctx.args.z_now {
            flags |= DF_BIND_NOW as u64;
            flags1 |= DF_1_NOW as u64;
        }
        if ctx.args.z_origin {
            flags |= DF_ORIGIN as u64;
            flags1 |= DF_1_ORIGIN as u64;
        }
        if !ctx.args.z_dlopen {
            flags1 |= DF_1_NOOPEN as u64;
        }
        if ctx.args.z_nodefaultlib {
            flags1 |= DF_1_NODEFLIB as u64;
        }
        if !ctx.args.z_delete {
            flags1 |= DF_1_NODELETE as u64;
        }
        if !ctx.args.z_dump {
            flags1 |= DF_1_NODUMP as u64;
        }
        if ctx.args.z_initfirst {
            flags1 |= DF_1_INITFIRST as u64;
        }
        if ctx.args.z_interpose {
            flags1 |= DF_1_INTERPOSE as u64;
        }
        if !ctx.got.gottp_syms.is_empty() {
            flags |= DF_STATIC_TLS as u64;
        }
        if has_textrel {
            flags |= DF_TEXTREL as u64;
        }
        if flags != 0 {
            define(DT_FLAGS, flags);
        }
        if flags1 != 0 {
            define(DT_FLAGS_1, flags1);
        }

        if E::FAMILY == Family::Arm64 && contains_variant_pcs(ctx) {
            define(DT_AARCH64_VARIANT_PCS, 0);
        }
        // RISC-V has the same feature but with a different name.
        if E::IS_RISCV
            && ctx
                .plt
                .symbols
                .iter()
                .any(|&id| ctx.symbols[id].esym(ctx).riscv_variant_cc())
        {
            define(DT_RISCV_VARIANT_CC, 0);
        }
        if E::FAMILY == Family::Ppc32 {
            define(DT_PPC_GOT, ctx.gotplt.hdr.shdr.sh_addr);
        }
        if E::IS_PPC64 {
            // PPC64_GLINK is defined by the psABI to refer to 32 bytes before
            // the first PLT entry. I don't know why it's 32 bytes off, but
            // it's what it is.
            define(
                DT_PPC64_GLINK,
                ctx.plt.hdr.shdr.sh_addr + crate::chunks::got::plt::entry_offset::<E>(0) - 32,
            );
        }

        // GDB needs a DT_DEBUG entry in an executable to store a word-size
        // data for its own purpose. Its content is not important.
        if !ctx.args.shared && !ctx.args.z_rodynamic {
            define(DT_DEBUG, 0);
        }

        define(DT_NULL, 0);
        for _ in 0..ctx.args.spare_dynamic_tags.max(0) {
            define(DT_NULL, 0);
        }
        vec
    }

    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
        if ctx.args.is_static && !ctx.args.pie {
            return;
        }
        let n = create_contents(ctx).len();
        let dynamic = ctx.dynamic.as_mut().unwrap();
        dynamic.hdr.shdr.sh_size = (n * ElfDyn::size::<E>()) as u64;
        dynamic.hdr.shdr.sh_link = ctx.dynstr.hdr.shndx;
    }

    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        let entries: Vec<ElfDyn> = create_contents(ctx)
            .into_iter()
            .map(|(d_tag, d_val)| ElfDyn { d_tag, d_val })
            .collect();
        debug_assert_eq!(
            ctx.dynamic.as_ref().unwrap().hdr.shdr.sh_size as usize,
            entries.len() * ElfDyn::size::<E>()
        );
        ElfDyn::write_all::<E>(&entries, buf);
    }

    /// A chunk id for the dynamic section, if it exists.
    pub fn id<E: Arch>(ctx: &Context<E>) -> Option<ChunkId> {
        ctx.dynamic.as_ref().map(|_| ChunkId::Dynamic)
    }
}
