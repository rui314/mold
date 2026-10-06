//! `.got`, addresses and thread-local offsets used at runtime.

use rayon::prelude::*;
use std::sync::atomic::Ordering;

use crate::arch::{Family, Target};
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::input_files::SymtabBlock;
use crate::symbol::{AddrFlags, NEEDS_GOT, NEEDS_GOTTP, NEEDS_TLSDESC, NEEDS_TLSGD, SymbolId};

// .got is a linker-synthesized constant pool whose entry size is the same
// as the pointer size. It is used to store runtime addresses of global
// variables and TP-relative offsets of thread-local variables.
#[derive(Debug)]
pub struct GotSection<E: Target> {
    pub hdr: ChunkHeader<E>,
    pub got_syms: Vec<SymbolId>,
    pub tlsgd_syms: Vec<SymbolId>,
    pub tlsdesc_syms: Vec<SymbolId>,
    pub gottp_syms: Vec<SymbolId>,
    pub tlsld_idx: Option<u32>,
}

impl<E: Target> GotSection<E> {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::<E>::new(".got", SHT_PROGBITS, (SHF_ALLOC | SHF_WRITE) as u64);
        hdr.is_relro = true;
        hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
        // We always create a .got so that _GLOBAL_OFFSET_TABLE_ has
        // something to point to. s390x psABI defines GOT[1] and GOT[2]
        // as reserved slots, so we allocate two more for them.
        let reserved = if E::FAMILY == Family::S390x { 3 } else { 1 };
        hdr.shdr.sh_size.set(reserved * E::WORD_SIZE as u64);
        Self {
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

    pub fn tlsld_addr(&self) -> u64 {
        self.hdr.shdr.sh_addr.get() + self.tlsld_idx.unwrap() as u64 * E::WORD_SIZE as u64
    }
}

impl<E: Target> Default for GotSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

fn word<E: Target>() -> u64 {
    E::WORD_SIZE as u64
}

/// Assigns GOT entries in file and symbol order, filling each file's range
/// in parallel. Returns requests that also need the ordered dynamic-table
/// pass (PLT entries, copy relocations, or dynamic symbols).
pub fn allocate_entries<E: Target>(
    ctx: &mut Context<E>,
    groups: Vec<Vec<SymbolId>>,
) -> Vec<(SymbolId, u8)> {
    const GOT_FLAGS: u8 = NEEDS_GOT | NEEDS_GOTTP | NEEDS_TLSGD | NEEDS_TLSDESC;
    struct Request {
        id: SymbolId,
        flags: u8,
        extra_slot: bool,
        ordered: bool,
    }
    let ctx_ref: &Context<E> = ctx;
    let groups: Vec<_> = groups
        .into_par_iter()
        .map(|ids| {
            let mut words = 0;
            let requests: Vec<_> = ids
                .into_iter()
                .map(|id| {
                    let sym = &ctx_ref.symbols[id];
                    // A symbol belongs to one file group. Repeated ids within
                    // that group consume the request only on their first use.
                    let flags = sym.take_flags();
                    let extra_slot = flags & NEEDS_GOT != 0 && sym.is_pde_ifunc(ctx_ref);
                    words += u64::from(flags & NEEDS_GOT != 0)
                        + u64::from(extra_slot)
                        + u64::from(flags & NEEDS_GOTTP != 0)
                        + 2 * u64::from(flags & NEEDS_TLSGD != 0)
                        + 2 * u64::from(flags & NEEDS_TLSDESC != 0);
                    let ordered = flags & !GOT_FLAGS != 0
                        || (ctx_ref.dynamic.is_some() && (sym.is_imported() || sym.is_exported()));
                    Request { id, flags, extra_slot, ordered }
                })
                .collect();
            (requests, words)
        })
        .collect();

    let mut next = ctx.got.hdr.shdr.sh_size.get() / word::<E>();
    let groups: Vec<_> = groups
        .into_iter()
        .map(|(requests, words)| {
            let first = next;
            next += words;
            (requests, first)
        })
        .collect();
    let tables: Vec<_> = groups
        .into_par_iter()
        .map(|(requests, mut next)| {
            let mut got = Vec::new();
            let mut gottp = Vec::new();
            let mut tlsgd = Vec::new();
            let mut tlsdesc = Vec::new();
            let mut ordered = Vec::new();
            for Request { id, flags, extra_slot, ordered: needs_order } in requests {
                let aux = ctx_ref.symbols[id].aux(&ctx_ref.symbols).unwrap();
                let mut assign =
                    |flag, index: &std::sync::atomic::AtomicU32, words, out: &mut Vec<_>| {
                        if flags & flag != 0 {
                            assert!(next < u32::MAX as u64);
                            index.store(next as u32, Ordering::Relaxed);
                            next += words;
                            out.push(id);
                        }
                    };
                assign(NEEDS_GOT, &aux.got_idx, 1 + u64::from(extra_slot), &mut got);
                assign(NEEDS_GOTTP, &aux.gottp_idx, 1, &mut gottp);
                assign(NEEDS_TLSGD, &aux.tlsgd_idx, 2, &mut tlsgd);
                assign(NEEDS_TLSDESC, &aux.tlsdesc_idx, 2, &mut tlsdesc);
                if flags & NEEDS_TLSDESC != 0 {
                    // Static links relax TLSDESC relocations because the
                    // runtime cannot initialize their GOT entries.
                    debug_assert!(E::SUPPORTS_TLSDESC);
                    debug_assert!(!ctx_ref.args.is_static);
                }
                if needs_order {
                    ordered.push((id, flags));
                }
            }
            (got, gottp, tlsgd, tlsdesc, ordered)
        })
        .collect();
    ctx.got.hdr.shdr.sh_size.set(next * word::<E>());
    let mut ordered = Vec::new();
    for (got, gottp, tlsgd, tlsdesc, requests) in tables {
        ctx.got.got_syms.extend(got);
        ctx.got.gottp_syms.extend(gottp);
        ctx.got.tlsgd_syms.extend(tlsgd);
        ctx.got.tlsdesc_syms.extend(tlsdesc);
        ordered.extend(requests);
    }
    ordered
}

pub fn add_got_symbol<E: Target>(ctx: &mut Context<E>, sym: SymbolId) {
    let size = ctx.got.hdr.shdr.sh_size.get();
    let idx = (size / word::<E>()) as u32;
    let is_pde_ifunc = ctx.symbols[sym].is_pde_ifunc(ctx);
    assert_ne!(idx, u32::MAX);
    *ctx.symbols.aux_mut(sym).got_idx.get_mut() = idx;
    // An IFUNC symbol uses two GOT slots in a position-dependent
    // executable.
    let increment = if is_pde_ifunc { 2 * word::<E>() } else { word::<E>() };
    ctx.got.hdr.shdr.sh_size.set(size.wrapping_add(increment));
    ctx.got.got_syms.push(sym);
}

pub fn add_gottp_symbol<E: Target>(ctx: &mut Context<E>, sym: SymbolId) {
    let size = ctx.got.hdr.shdr.sh_size.get();
    let idx = (size / word::<E>()) as u32;
    assert_ne!(idx, u32::MAX);
    *ctx.symbols.aux_mut(sym).gottp_idx.get_mut() = idx;
    ctx.got.hdr.shdr.sh_size.set(size + word::<E>());
    ctx.got.gottp_syms.push(sym);
}

pub fn add_tlsgd_symbol<E: Target>(ctx: &mut Context<E>, sym: SymbolId) {
    let size = ctx.got.hdr.shdr.sh_size.get();
    let idx = (size / word::<E>()) as u32;
    assert_ne!(idx, u32::MAX);
    *ctx.symbols.aux_mut(sym).tlsgd_idx.get_mut() = idx;
    ctx.got.hdr.shdr.sh_size.set(size + 2 * word::<E>());
    ctx.got.tlsgd_syms.push(sym);
}

pub fn add_tlsdesc_symbol<E: Target>(ctx: &mut Context<E>, sym: SymbolId) {
    // TLSDESC's GOT slot values may vary depending on libc, so we
    // always emit a dynamic relocation for each TLSDESC entry.
    //
    // If dynamic relocation is not available (i.e. if we are creating a
    // statically-linked executable), we always relax TLSDESC relocations
    // so that no TLSDESC relocation exists at runtime.
    debug_assert!(E::SUPPORTS_TLSDESC);
    debug_assert!(!ctx.args.is_static);
    let size = ctx.got.hdr.shdr.sh_size.get();
    let idx = (size / word::<E>()) as u32;
    assert_ne!(idx, u32::MAX);
    *ctx.symbols.aux_mut(sym).tlsdesc_idx.get_mut() = idx;
    ctx.got.hdr.shdr.sh_size.set(size + 2 * word::<E>());
    ctx.got.tlsdesc_syms.push(sym);
}

pub fn add_tlsld<E: Target>(ctx: &mut Context<E>) {
    debug_assert!(ctx.got.tlsld_idx.is_none());
    let size = ctx.got.hdr.shdr.sh_size.get();
    ctx.got.tlsld_idx = Some((size / word::<E>()) as u32);
    ctx.got.hdr.shdr.sh_size.set(size + 2 * word::<E>());
}

struct GotEntry {
    idx: u32,
    val: u64,
    r_type: u32,
    sym: Option<SymbolId>,
}

// Get .got and .rel.dyn contents.
//
// .got is a linker-synthesized constant pool whose entry is of pointer
// size. If we know a correct value for an entry, we'll just set that value
// to the entry. Otherwise, we'll create a dynamic relocation and let the
// dynamic linker fill the entry at load-time.
//
// Most GOT entries contain addresses of global variables. If a global
// variable is an imported symbol, we don't know its address until runtime.
// GOT contains the addresses of such variables at runtime so that we can
// access imported global variables via GOT.
//
// Thread-local variables (TLVs) also use GOT entries. We need them because
// TLVs are accessed in a different way than the ordinary global variables.
// Their addresses are not unique; each thread has its own copy of TLVs.
fn for_each_entry<E: Target>(ctx: &Context<E>, mut emit: impl FnMut(GotEntry)) {
    let mut add = |idx: u32, val: u64, r_type: u32, sym: Option<SymbolId>| {
        emit(GotEntry { idx, val, r_type, sym });
    };
    let got = &ctx.got;

    // Create GOT entries for ordinary symbols
    for &id in &got.got_syms {
        let sym = &ctx.symbols[id];
        let idx = sym.got_idx(&ctx.symbols).unwrap();

        // IFUNC always needs to be fixed up by the dynamic linker.
        if let Some(r_irelative) = E::R_IRELATIVE
            && sym.is_ifunc()
        {
            if sym.is_pde_ifunc(ctx) {
                add(idx, sym.plt_addr(ctx), R_NONE, None);
                add(idx + 1, sym.addr_with(ctx, AddrFlags::NO_PLT), r_irelative, None);
            } else {
                add(idx, sym.addr_with(ctx, AddrFlags::NO_PLT), r_irelative, None);
            }
            continue;
        }

        if sym.is_imported() {
            // If a symbol is imported, let the dynamic linker resolve it.
            add(idx, 0, E::R_GLOB_DAT, Some(id));
        } else if ctx.args.pic && sym.is_relative() {
            // We know the symbol's address, but it needs a base relocation.
            add(idx, sym.addr_with(ctx, AddrFlags::NO_PLT), E::R_RELATIVE, None);
        } else {
            // We know the symbol's exact run-time address at link-time.
            add(idx, sym.addr_with(ctx, AddrFlags::NO_PLT), R_NONE, None);
        }
    }

    // Create GOT entries for TLVs.
    for &id in &got.tlsgd_syms {
        let sym = &ctx.symbols[id];
        let idx = sym.tlsgd_idx(&ctx.symbols).unwrap();
        if sym.is_imported() {
            // If a symbol is imported, let the dynamic linker resolve it.
            add(idx, 0, E::R_DTPMOD, Some(id));
            add(idx + 1, 0, E::R_DTPOFF, Some(id));
        } else if ctx.args.shared {
            // If we are creating a shared library, we know the TLV's offset
            // within the current TLS block. We don't know the module ID though.
            add(idx, 0, E::R_DTPMOD, None);
            add(idx + 1, sym.addr(ctx).wrapping_sub(ctx.dtp_addr), R_NONE, None);
        } else {
            // If we are creating an executable, we know both the module ID and
            // the offset. Module ID 1 indicates the main executable.
            add(idx, 1, R_NONE, None);
            add(idx + 1, sym.addr(ctx).wrapping_sub(ctx.dtp_addr), R_NONE, None);
        }
    }

    if let Some(r_tlsdesc) = E::R_TLSDESC {
        for &id in &got.tlsdesc_syms {
            let sym = &ctx.symbols[id];
            let idx = sym.tlsdesc_idx(&ctx.symbols).unwrap();

            // TLSDESC uses two consecutive GOT slots, and a single TLSDESC
            // dynamic relocation fills both. The actual values of the slots
            // vary depending on libc, so we can't precompute their values.
            // We always emit a dynamic relocation for each incoming TLSDESC
            // reloc.
            if sym.is_imported() {
                add(idx, 0, r_tlsdesc, Some(id));
            } else {
                add(idx, sym.addr(ctx).wrapping_sub(ctx.tls_begin), r_tlsdesc, None);
            }
        }
    }

    for &id in &got.gottp_syms {
        let sym = &ctx.symbols[id];
        let idx = sym.gottp_idx(&ctx.symbols).unwrap();
        if sym.is_imported() {
            // If we know nothing about the symbol, let the dynamic linker
            // fill the GOT entry.
            add(idx, 0, E::R_TPOFF, Some(id));
        } else if ctx.args.shared {
            // If we know the offset within the current thread vector,
            // let the dynamic linker adjust it.
            add(idx, sym.addr(ctx).wrapping_sub(ctx.tls_begin), E::R_TPOFF, None);
        } else {
            // Otherwise, we know the offset from the thread pointer (TP) at
            // link-time, so we can fill the GOT entry directly.
            add(idx, sym.addr(ctx).wrapping_sub(ctx.tp_addr), R_NONE, None);
        }
    }

    if let Some(idx) = got.tlsld_idx {
        if ctx.args.shared {
            add(idx, 0, E::R_DTPMOD, None);
        } else {
            add(idx, 1, R_NONE, None); // 1 means the main executable
        }
    }
}

// Count the dynamic relocations that for_each_entry will emit, without
// materializing the entries; computing each entry's value involves a
// symbol address lookup, which is too expensive for a function that runs
// on every layout iteration. The cases below must mirror the r_type
// choices in for_each_entry.
pub fn num_dynrels<E: Target>(ctx: &Context<E>) -> u64 {
    let got = &ctx.got;
    let pic = ctx.args.pic;

    // Every lookup misses the cache in a large link, so count in parallel.
    let mut n = got
        .got_syms
        .par_iter()
        .filter(|&&id| {
            let sym = &ctx.symbols[id];
            // R_IRELATIVE, R_GLOB_DAT or R_RELATIVE
            (E::SUPPORTS_IFUNC && sym.is_ifunc()) || sym.is_imported() || (pic && sym.is_relative())
        })
        .count() as u64;
    for &id in &got.tlsgd_syms {
        let sym = &ctx.symbols[id];
        if sym.is_imported() {
            n += 2; // R_DTPMOD + R_DTPOFF
        } else if ctx.args.shared {
            n += 1; // R_DTPMOD
        }
    }
    if E::SUPPORTS_TLSDESC {
        n += got.tlsdesc_syms.len() as u64; // R_TLSDESC each
    }
    for &id in &got.gottp_syms {
        if ctx.symbols[id].is_imported() || ctx.args.shared {
            n += 1; // R_TPOFF
        }
    }
    if got.tlsld_idx.is_some() && ctx.args.shared {
        n += 1; // R_DTPMOD
    }
    n
}

pub fn relr_offsets<E: Target>(ctx: &Context<E>) -> Vec<u64> {
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

pub fn write_dynrels<E: Target>(ctx: &Context<E>, out: &mut [ElfRel<E>]) {
    let mut i = 0;
    for_each_entry(ctx, |ent| {
        if ent.r_type == R_NONE {
            return;
        }
        let rel = ElfRel::<E>::new(
            ctx.got.hdr.shdr.sh_addr.get() + ent.idx as u64 * word::<E>(),
            ent.r_type,
            ent.sym.and_then(|s| ctx.symbols[s].dynsym_idx(&ctx.symbols)).unwrap_or(0),
            ent.val as i64,
        );
        let is_relr = rel.r_type() == E::R_RELATIVE && rel.r_offset().is_multiple_of(word::<E>());
        if !ctx.args.pack_dyn_relocs_relr || ctx.got.hdr.num_relrs == 0 || !is_relr {
            out[i] = rel;
            i += 1;
        }
    });
    debug_assert_eq!(i, out.len());
}

// Fill .got.
pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    buf.fill(0);
    let w = word::<E>() as usize;
    let write =
        |buf: &mut [u8], idx: usize, val: u64| Word::<E>::new(val).write(&mut buf[idx * w..]);

    // s390x psABI requires GOT[0] to be set to the link-time value of _DYNAMIC.
    if let Some(dynamic) = &ctx.dynamic {
        if E::FAMILY == Family::S390x {
            write(buf, 0, dynamic.shdr.sh_addr.get());
        }

        // ARM64 psABI doesn't say anything about GOT[0], but glibc/arm64's code
        // path for -static-pie wrongly assumed that GOT[0] refers to _DYNAMIC.
        //
        // https://sourceware.org/git/?p=glibc.git;a=commitdiff;h=43d06ed218fc8be5
        if E::FAMILY == Family::Arm64 && ctx.args.is_static && ctx.args.pie {
            write(buf, 0, dynamic.shdr.sh_addr.get());
        }
    }

    for_each_entry(ctx, |ent| {
        let is_relr = ent.r_type == E::R_RELATIVE && ctx.args.pack_dyn_relocs_relr;
        if is_relr || ent.r_type == R_NONE {
            write(buf, ent.idx as usize, ent.val);
            return;
        }
        if ctx.args.apply_dynamic_relocs {
            // A single TLSDESC relocation fixes two consecutive GOT slots
            // where one slot holds a function pointer and the other an
            // argument to the function. An addend should be applied not to
            // the function pointer but to the function argument, which is
            // usually stored to the second slot.
            //
            // ARM32 employs the inverted layout for some reason, so an
            // addend is applied to the first slot.
            let mut i = ent.idx as usize;
            if E::SUPPORTS_TLSDESC && E::FAMILY != Family::Arm32 && Some(ent.r_type) == E::R_TLSDESC
            {
                i += 1;
            }
            write(buf, i, ent.val);
        }
    });
}

pub fn compute_symtab_size<E: Target>(ctx: &mut Context<E>) {
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

pub fn populate_symtab<E: Target>(ctx: &Context<E>, block: &mut SymtabBlock<'_>) {
    let got = &ctx.got;
    if got.hdr.num_local_symtab == 0 {
        return;
    }
    let object = |value: u64| {
        let mut sym = ElfSym::<E>::default();
        sym.set_st_shndx(got.hdr.shndx);
        sym.set_st_value(value);
        sym.set_type(STT_OBJECT);
        sym
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
        block.push_synthetic::<E>(b"", b"$tlsld", object(got.tlsld_addr()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arch::{I386, X86_64};
    use crate::cmdline::Args;
    use crate::symbol::NEEDS_PLT;

    fn check_layout<E: Target>(pic: bool, repeats: usize) {
        let args = Args { pic, ..Args::default() };
        let mut ctx = Context::<E>::new(args, Vec::new());
        ctx.dynamic = Some(crate::chunks::dynamic::new_header(&ctx.args));
        let a = ctx.symbols.intern(b"a");
        let b = ctx.symbols.intern(b"b");
        let c = ctx.symbols.intern(b"c");
        let d = ctx.symbols.intern(b"d");
        let e = ctx.symbols.intern(b"e");
        let f = ctx.symbols.intern(b"f");
        ctx.symbols[a].add_flags(NEEDS_GOT | NEEDS_PLT);
        ctx.symbols[a].set_write_to_symtab();
        ctx.symbols[b].add_flags(NEEDS_GOT | NEEDS_GOTTP | NEEDS_TLSGD | NEEDS_TLSDESC);
        ctx.symbols[c].add_flags(NEEDS_GOT | NEEDS_PLT);
        let mut esym = ElfSym::<E>::default();
        esym.set_type(STT_GNU_IFUNC);
        ctx.symbols[c].set_esym(&esym);
        ctx.symbols[d].add_flags(NEEDS_TLSDESC);
        ctx.symbols[e].set_exported(true);
        ctx.symbols[f].set_imported(true);
        ctx.symbols[f].add_flags(NEEDS_GOT);
        ctx.symbols.aux_mut(b).plt_idx = 17;

        // Mix single- and double-word entries, repeats, an empty file and
        // existing auxiliary data. TLSLD occupies the first two free slots.
        add_tlsld(&mut ctx);
        let mut first = vec![a, b];
        first.extend(std::iter::repeat_n(a, repeats));
        let groups = vec![first, vec![], vec![c, d, e, f]];
        ctx.symbols.allocate_aux(&groups);
        let ordered = allocate_entries(&mut ctx, groups);
        assert_eq!(
            ordered,
            [(a, NEEDS_GOT | NEEDS_PLT), (c, NEEDS_GOT | NEEDS_PLT), (e, 0), (f, NEEDS_GOT)]
        );
        assert_eq!(ctx.got.tlsld_idx, Some(1));
        assert_eq!(ctx.symbols[a].got_idx(&ctx.symbols), Some(3));
        assert_eq!(ctx.symbols[b].got_idx(&ctx.symbols), Some(4));
        assert_eq!(ctx.symbols[b].gottp_idx(&ctx.symbols), Some(5));
        assert_eq!(ctx.symbols[b].tlsgd_idx(&ctx.symbols), Some(6));
        assert_eq!(ctx.symbols[b].tlsdesc_idx(&ctx.symbols), Some(8));
        assert_eq!(ctx.symbols[b].plt_idx(&ctx.symbols), Some(17));
        assert_eq!(ctx.symbols[c].got_idx(&ctx.symbols), Some(10));
        let extra = u32::from(!pic);
        assert_eq!(ctx.symbols[d].tlsdesc_idx(&ctx.symbols), Some(11 + extra));
        assert_eq!(ctx.symbols[f].got_idx(&ctx.symbols), Some(13 + extra));
        assert_eq!(ctx.got.hdr.shdr.sh_size.get(), (14 + extra as u64) * E::WORD_SIZE as u64);
        assert_eq!(ctx.got.got_syms, [a, b, c, f]);
        assert_eq!(ctx.got.gottp_syms, [b]);
        assert_eq!(ctx.got.tlsgd_syms, [b]);
        assert_eq!(ctx.got.tlsdesc_syms, [b, d]);
        assert!(ctx.symbols[a].write_to_symtab());
        for id in [a, b, c, d, e, f] {
            assert_eq!(ctx.symbols[id].flags(), 0);
        }
        assert!(allocate_entries(&mut ctx, vec![]).is_empty());
        assert_eq!(ctx.got.got_syms, [a, b, c, f]);
    }

    #[test]
    fn got_ranges_preserve_order_and_ifunc_width() {
        for threads in [1, 4] {
            let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
            pool.install(|| {
                for pic in [false, true] {
                    for repeats in [1, 4096] {
                        check_layout::<X86_64>(pic, repeats);
                        check_layout::<I386>(pic, repeats);
                    }
                }
            });
        }
    }
}
