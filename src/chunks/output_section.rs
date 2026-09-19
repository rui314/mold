//! Output sections built from input sections, such as `.text` and `.data`.

use std::marker::PhantomData;
use std::sync::atomic::Ordering;

use bstr::BStr;
use rayon::prelude::*;

use crate::arch::{Arch, Family};
use crate::chunks::{ChunkHeader, OutputSectionId};
use crate::context::Context;
use crate::elf::*;
use crate::input_files::SymtabBlock;
use crate::input_sections::{InputSectionId, r_delta};
use crate::symbol::{AddrFlags, NEEDS_CANONICAL, SymbolId};
use crate::thunks::Thunk;
use crate::util::align_to;
use crate::util::endian::Endian;
use crate::{error, warn};

/// How a word-size absolute relocation is resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AbsRelKind {
    /// The value is known at link time.
    #[default]
    None,
    /// A base relocation, applied by the loader.
    BaseRel,
    /// A base relocation encoded in `.relr.dyn`.
    Relr,
    /// An IFUNC relocation.
    IFunc,
    /// A symbolic dynamic relocation.
    DynRel,
}

// Represents a word-size absolute relocation (e.g. R_X86_64_64)
#[derive(Clone, Debug)]
pub struct AbsRel {
    pub isec: InputSectionId,
    pub offset: u64,
    pub sym: SymbolId,
    pub addend: i64,
    pub kind: AbsRelKind,
}

// OutputSection represents the usual output section that contains input
// sections read from object files.
#[derive(Debug)]
pub struct OutputSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
    pub members: Vec<InputSectionId>,
    pub thunks: Vec<Thunk>,
    pub reloc_sec: Option<u32>,
    pub abs_rels: Vec<AbsRel>,

    /// The number of dynamic relocations before each shard of `abs_rels`.
    pub dynrel_offsets: Vec<u64>,
    pub relr_offsets: Vec<u64>,
}

/// A pointer to an output buffer whose disjoint ranges are written in parallel.
pub struct OutputBuffer<'a> {
    ptr: *mut u8,
    len: usize,
    marker: PhantomData<&'a mut [u8]>,
}

// SAFETY: Parallel loops use the pointer only for ranges that the ELF layout
// proves disjoint, just as their C++ counterparts do.
unsafe impl Sync for OutputBuffer<'_> {}

impl<'a> OutputBuffer<'a> {
    #[inline]
    pub(crate) fn new(buf: &'a mut [u8]) -> Self {
        OutputBuffer { ptr: buf.as_mut_ptr(), len: buf.len(), marker: PhantomData }
    }

    /// Runs `f` on one range owned by the current parallel-loop iteration.
    ///
    /// # Safety
    /// No other live access may overlap `range`.
    #[inline]
    pub(crate) unsafe fn with_slice<R>(
        &self,
        range: std::ops::Range<usize>,
        f: impl FnOnce(&mut [u8]) -> R,
    ) -> R {
        debug_assert!(range.start <= range.end && range.end <= self.len);
        // SAFETY: the caller guarantees that this in-bounds range is the
        // iteration's exclusive output range.
        let slice = unsafe {
            std::slice::from_raw_parts_mut(self.ptr.add(range.start), range.end - range.start)
        };
        f(slice)
    }

    /// Writes one relocated word.
    ///
    /// # Safety
    /// No other live access may overlap the word at `offset`.
    unsafe fn write_word<E: Arch>(&self, offset: u64, value: u64) {
        // SAFETY: The caller guarantees that the word is within the output
        // section and exclusively owned by this relocation.
        debug_assert!(offset as usize + E::WORD_SIZE <= self.len);
        let ptr = unsafe { self.ptr.add(offset as usize) };
        let slot = unsafe { std::slice::from_raw_parts_mut(ptr, E::WORD_SIZE) };
        if E::IS_64 {
            E::Endian::write_u64(slot, value);
        } else {
            E::Endian::write_u32(slot, value as u32);
        }
    }
}

/// Shard size for parallel processing of absolute relocations.
pub const DYNREL_SHARD_SIZE: usize = 65536;

impl<E: Layout> OutputSection<E> {
    pub fn new(name: &'static BStr, sh_type: u32) -> Self {
        Self {
            hdr: ChunkHeader::<E>::with_name(name, sh_type, 0),
            members: Vec::new(),
            thunks: Vec::new(),
            reloc_sec: None,
            abs_rels: Vec::new(),
            dynrel_offsets: Vec::new(),
            relr_offsets: Vec::new(),
        }
    }
}

// Assign offsets to OutputSection members
pub fn compute_section_size<E: Arch>(ctx: &mut Context<E>, id: OutputSectionId) {
    let size = layout(ctx, id);
    ctx.output_sections[id.index()].hdr.shdr.sh_size.set(size);
}

/// Assigns the members their offsets and returns the section size.
///
/// An output section such as `.text` has a million members, so the
/// members are split into groups: the sizes of the groups are computed
/// in parallel, then the offsets within each group.
pub fn layout<E: Arch>(ctx: &Context<E>, id: OutputSectionId) -> u64 {
    const GROUP_SIZE: usize = 10000;

    let osec = &ctx.output_sections[id.index()];

    // Text sections must to be handled by create_range_extension_thunks()
    // if they may need range extension thunks.
    debug_assert!(
        !E::NEEDS_THUNK
            || osec.hdr.shdr.sh_flags.get() & SHF_EXECINSTR as u64 == 0
            || ctx.args.relocatable
    );

    // Since one output section may contain millions of input sections,
    // we first split input sections into groups and assign offsets to
    // groups.
    struct Group {
        size: u64,
        offset: u64,
        align: u64,
    }

    let mut groups: Vec<Group> = osec
        .members
        .par_chunks(GROUP_SIZE)
        .map(|members| {
            let mut off = 0;
            let mut align = 1;
            for &m in members {
                let isec = ctx.input_section(m);
                off = align_to(off, 1 << isec.p2align()) + isec.sh_size;
                align = align.max(1 << isec.p2align());
            }
            Group { size: off, offset: 0, align }
        })
        .collect();

    let mut off = 0;
    for g in &mut groups {
        off = align_to(off, g.align);
        g.offset = off;
        off += g.size;
    }

    // Assign offsets to input sections.
    osec.members.par_chunks(GROUP_SIZE).zip(&groups).for_each(|(members, g)| {
        let mut off = g.offset;
        for &m in members {
            let isec = ctx.input_section(m);
            off = align_to(off, 1 << isec.p2align());
            isec.set_offset(off);
            off += isec.sh_size;
        }
    });
    off
}

/// Copies the members into `buf`, fills the padding between them, and
/// applies relocations.
pub fn write_to<E: Arch>(ctx: &Context<E>, id: OutputSectionId, buf: &mut [u8]) {
    let osec = &ctx.output_sections[id.index()];
    let members = &osec.members;

    let output = OutputBuffer::new(buf);

    // Copy section contents to an output file.
    members.par_iter().enumerate().for_each(|(i, &member)| {
        let isec = ctx.input_section(member);
        let start = isec.offset() as usize;
        let next_start = members.get(i + 1).map_or(osec.hdr.shdr.sh_size.get() as usize, |&next| {
            ctx.input_section(next).offset() as usize
        });
        // SAFETY: output-section member offsets are ordered and each
        // iteration owns the bytes up to the next member.
        unsafe {
            output.with_slice(start..next_start, |slice| {
                let (own, padding) = slice.split_at_mut(isec.sh_size as usize);
                isec.write_to(ctx, own);

                // Clear trailing padding. We write trap instructions for an
                // executable segment so that a disassembler wouldn't try to
                // disassemble garbage as instructions.
                if osec.hdr.shdr.sh_flags.get() & SHF_EXECINSTR as u64 != 0 {
                    // s390x's old CRT files use NOP slides in .init and .fini.
                    // https://sourceware.org/bugzilla/show_bug.cgi?id=31042
                    let filler: &[u8] = if E::FAMILY == Family::S390x
                        && (osec.hdr.name == b".init" || osec.hdr.name == b".fini")
                    {
                        &[0x07, 0x00] // nopr
                    } else {
                        E::TRAP
                    };
                    let mut pos = 0;
                    while pos + filler.len() <= padding.len() {
                        padding[pos..pos + filler.len()].copy_from_slice(filler);
                        pos += filler.len();
                    }
                } else {
                    padding.fill(0);
                }
            });
        }
    });

    // Emit range extension thunks.
    if E::NEEDS_THUNK {
        osec.thunks.par_iter().for_each(|thunk| {
            let start = thunk.offset as usize;
            let end = start + thunk.size() as usize;
            // SAFETY: thunks have distinct ranges and the member loop has
            // joined before this loop begins.
            unsafe {
                output.with_slice(start..end, |buf| {
                    crate::thunks::copy_buf(ctx, osec, thunk, buf);
                });
            }
        });
    }
}

/// Writes the section and applies word-size absolute relocations.
pub fn copy_buf<E: Arch>(ctx: &Context<E>, id: OutputSectionId, buf: &mut [u8]) {
    let osec = &ctx.output_sections[id.index()];
    if osec.hdr.shdr.sh_type.get() == SHT_NOBITS {
        return;
    }
    write_to(ctx, id, buf);

    // Apply absolute relocations. An output section can have a million
    // of them, so this loop is parallel.
    let word = E::WORD_SIZE;
    let base_addr = osec.hdr.shdr.sh_addr.get();
    let output = OutputBuffer::new(buf);

    osec.abs_rels.par_iter().for_each(|r| {
        let isec = ctx.input_section(r.isec);
        let sym = &ctx.symbols[r.sym];
        let mut loc = isec.offset() + r.offset;
        let mut p = base_addr + loc;
        if E::IS_RISCV || E::IS_LOONGARCH {
            let delta = r_delta(isec, r.offset) as u64;
            loc -= delta;
            p -= delta;
        }
        let s = sym.addr(ctx);
        let a = r.addend as u64;

        let can_relr = || {
            ctx.args.pack_dyn_relocs_relr
                && r.kind == AbsRelKind::BaseRel
                && p.is_multiple_of(word as u64)
        };

        let value = match r.kind {
            AbsRelKind::None | AbsRelKind::Relr => Some(s.wrapping_add(a)),
            AbsRelKind::BaseRel => {
                (can_relr() || ctx.args.apply_dynamic_relocs).then(|| s.wrapping_add(a))
            }
            AbsRelKind::IFunc => (E::SUPPORTS_IFUNC && ctx.args.apply_dynamic_relocs)
                .then(|| sym.addr_with(ctx, AddrFlags::NO_PLT).wrapping_add(a)),
            AbsRelKind::DynRel => ctx.args.apply_dynamic_relocs.then_some(a),
        };
        if let Some(value) = value {
            // SAFETY: Input files do not contain overlapping relocations;
            // each `AbsRel` therefore owns this output word.
            unsafe { output.write_word::<E>(loc, value) };
        }
    });
}

pub fn num_dynrels<E: Arch>(ctx: &Context<E>, id: OutputSectionId) -> u64 {
    ctx.output_sections[id.index()].dynrel_offsets.last().copied().unwrap_or(0)
}

/// Marks the base relocations that RELR can encode and returns their
/// offsets within the section.
pub fn relr_offsets<E: Arch>(ctx: &mut Context<E>, id: OutputSectionId) -> Vec<u64> {
    let word = E::WORD_SIZE as u64;
    let Context { objs, output_sections, .. } = ctx;
    let osec = &mut output_sections[id.index()];
    let nshards = osec.dynrel_offsets.len().saturating_sub(1);
    let mut relr_offsets = vec![0u64; nshards + 1];

    let shards: Vec<Vec<u64>> = osec
        .abs_rels
        .par_chunks_mut(DYNREL_SHARD_SIZE)
        .map(|shard| {
            let mut out = Vec::new();
            for r in shard {
                if r.kind != AbsRelKind::BaseRel {
                    continue;
                }
                let isec = objs[r.isec.file().index()].sections.input(r.isec.index());
                if (1u64 << isec.p2align()).is_multiple_of(word) && r.offset % word == 0 {
                    r.kind = AbsRelKind::Relr;
                    out.push(isec.offset() + r.offset);
                }
            }
            out
        })
        .collect();

    let mut offsets = Vec::new();
    for (i, shard) in shards.into_iter().enumerate() {
        relr_offsets[i + 1] = relr_offsets[i] + shard.len() as u64;
        offsets.extend(shard);
    }
    osec.relr_offsets = relr_offsets;
    offsets
}

pub fn write_dynrels<E: Arch>(ctx: &Context<E>, id: OutputSectionId, out: &mut [E::Rel]) {
    let osec = &ctx.output_sections[id.index()];
    // A single output section such as .data.rel.ro can account for
    // most of an output's dynamic relocations, so we process its
    // absolute relocations in parallel shards.
    let nshards = osec.dynrel_offsets.len().saturating_sub(1);
    let mut offsets = std::borrow::Cow::Borrowed(osec.dynrel_offsets.as_slice());

    if ctx.args.pack_dyn_relocs_relr && osec.hdr.num_relrs != 0 {
        debug_assert_eq!(osec.relr_offsets.len(), offsets.len());
        for (offset, &relr_offset) in offsets.to_mut().iter_mut().zip(&osec.relr_offsets) {
            *offset -= relr_offset;
        }
    }

    let count = offsets.last().copied().unwrap_or(0) as usize;
    debug_assert_eq!(count as u64, osec.hdr.num_dynrels - osec.hdr.num_relrs);
    debug_assert_eq!(out.len(), count);
    let slices = crate::output_file::split_at_offsets(out, &offsets[..nshards]);

    osec.abs_rels.par_chunks(DYNREL_SHARD_SIZE).zip(slices.into_par_iter()).for_each(
        |(rels, slots)| {
            let mut i = 0;
            for r in rels {
                let sym = &ctx.symbols[r.sym];
                let isec = ctx.input_section(r.isec);
                let s = sym.addr(ctx);
                let a = r.addend;
                let mut p = osec.hdr.shdr.sh_addr.get() + isec.offset() + r.offset;
                if E::IS_RISCV || E::IS_LOONGARCH {
                    p -= r_delta(isec, r.offset) as u64;
                }
                let rel = match r.kind {
                    AbsRelKind::None | AbsRelKind::Relr => None,
                    AbsRelKind::BaseRel => {
                        Some(ElfRel::<E>::new(p, E::R_RELATIVE, 0, s.wrapping_add(a as u64) as i64))
                    }
                    AbsRelKind::IFunc => E::R_IRELATIVE.map(|r_type| {
                        let val = sym.addr_with(ctx, AddrFlags::NO_PLT).wrapping_add(a as u64);
                        ElfRel::<E>::new(p, r_type, 0, val as i64)
                    }),
                    AbsRelKind::DynRel => Some(ElfRel::<E>::new(
                        p,
                        E::R_ABS,
                        sym.dynsym_idx(&ctx.symbols).unwrap_or(0),
                        a,
                    )),
                };
                if let Some(rel) = rel {
                    slots[i] = rel;
                    i += 1;
                }
            }
            debug_assert_eq!(i, slots.len());
        },
    );
}

fn abs_rel_kind<E: Arch>(ctx: &Context<E>, sym: &crate::symbol::Symbol) -> AbsRelKind {
    if sym.is_ifunc() {
        return if sym.is_pde_ifunc(ctx) { AbsRelKind::None } else { AbsRelKind::IFunc };
    }
    if sym.is_absolute() {
        return AbsRelKind::None;
    }
    // True if the symbol's address is in the output file.
    if !sym.is_imported() || sym.flags() & NEEDS_CANONICAL != 0 {
        return if ctx.args.pic { AbsRelKind::BaseRel } else { AbsRelKind::None };
    }
    AbsRelKind::DynRel
}

fn is_absrel<E: Arch>(r: &ElfRel<E>) -> bool {
    match E::FAMILY {
        // On ARM32, R_ARM_TARGET1 is typically used for entries in .init_array
        // and is interpreted as either ABS32 or REL32 depending on the target.
        // All targets we support handle it as if it were a ABS32.
        Family::Arm32 => r.r_type() == R_ARM_ABS32 || r.r_type() == R_ARM_TARGET1,
        // SPARC64 defines two separate relocations for aligned and unaligned words.
        Family::Sparc64 => r.r_type() == R_SPARC_64 || r.r_type() == R_SPARC_UA64,
        _ => r.r_type() == E::R_ABS,
    }
}

// Scan word-size absolute relocations (e.g. R_X86_64_64). This is
// separated from scan_relocations() because only such relocations can
// be promoted to dynamic relocations.
pub fn scan_abs_relocations<E: Arch>(
    ctx: &Context<E>,
    id: OutputSectionId,
) -> (Vec<AbsRel>, Vec<u64>) {
    let osec = &ctx.output_sections[id.index()];

    // Collect all word-size absolute relocations. Count them per member
    // first so that they can be written to their final positions in
    // parallel, without a per-member vector.
    let mut abs_rels: Vec<AbsRel> = osec
        .members
        .par_iter()
        .flat_map_iter(|&m| {
            let isec = ctx.input_section(m);
            let file = &ctx.objs[isec.file.index()];
            isec.rels(file).iter().filter(|r| is_absrel::<E>(r)).map(move |r| AbsRel {
                isec: m,
                offset: r.r_offset(),
                sym: file.base.symbols[r.r_sym() as usize],
                addend: isec.rel_addend(r),
                kind: AbsRelKind::None,
            })
        })
        .collect();

    // We can sometimes avoid creating dynamic relocations in read-only
    // sections by promoting symbols to canonical PLT or copy relocations.
    let promote = !ctx.args.pic && osec.hdr.shdr.sh_flags.get() & SHF_WRITE as u64 == 0;

    // Classify relocations and retain exact per-shard output counts. A
    // single output section such as .data.rel.ro can account for most of
    // an output's absolute relocations, so this runs in the same parallel
    // shards as write_dynrels().
    let counts: Vec<u64> = abs_rels
        .par_chunks_mut(DYNREL_SHARD_SIZE)
        .map(|shard| {
            let mut count = 0;
            for r in shard {
                let sym = &ctx.symbols[r.sym];
                if promote && sym.is_imported() && !sym.is_absolute() {
                    sym.add_flags(NEEDS_CANONICAL);
                }
                r.kind = abs_rel_kind(ctx, sym);

                let emit = matches!(r.kind, AbsRelKind::BaseRel | AbsRelKind::DynRel)
                    || (E::SUPPORTS_IFUNC && r.kind == AbsRelKind::IFunc);
                if emit {
                    count += 1;
                }

                // If we have a relocation against a read-only section, we need to
                // set the DT_TEXTREL flag for the loader.
                let isec = ctx.input_section(r.isec);
                if r.kind != AbsRelKind::None && isec.sh_flags & SHF_WRITE as u64 == 0 {
                    if ctx.args.z_text {
                        error!("{}: relocation at offset 0x{:x} against symbol `{}' can not be used; recompile with -fPIC",
                            ctx.input_section_display(r.isec),
                            r.offset,
                            sym
                        );
                    } else if ctx.args.warn_textrel {
                        warn!("{}: relocation against symbol `{}' in read-only section",
                            ctx.input_section_display(r.isec),
                            sym
                        );
                    }
                    ctx.has_textrel.store(true, Ordering::Relaxed);
                }
            }
            count
        })
        .collect();

    let mut dynrel_offsets = vec![0u64; counts.len() + 1];
    for (i, c) in counts.iter().enumerate() {
        dynrel_offsets[i + 1] = dynrel_offsets[i] + c;
    }
    (abs_rels, dynrel_offsets)
}

// Compute spaces needed for thunk symbols
pub fn compute_symtab_size<E: Arch>(ctx: &mut Context<E>, id: OutputSectionId) {
    if !E::NEEDS_THUNK {
        return;
    }
    let symbols = &ctx.symbols;
    let osec = &mut ctx.output_sections[id.index()];
    osec.hdr.strtab_size = 0;
    osec.hdr.num_local_symtab = 0;
    for thunk in &osec.thunks {
        // For ARM32, we emit additional symbol "$t", "$a" and "$d" for
        // each thunk to mark the beginning of Thumb code, ARM code and
        // data, respectively.
        let per_entry = if E::FAMILY == Family::Arm32 { 4 } else { 1 };
        osec.hdr.num_local_symtab += (thunk.symbols.len() * per_entry) as u32;
        for &sym in &thunk.symbols {
            osec.hdr.strtab_size += symbols[sym].name().len() as u64 + thunk.name.len() as u64 + 2;
        }
    }
}

// If we create range extension thunks, we also synthesize symbols to mark
// the locations of thunks. Creating such symbols is optional, but it helps
// disassembling and/or debugging our output.
pub fn populate_symtab<E: Arch>(
    ctx: &Context<E>,
    id: OutputSectionId,
    block: &mut SymtabBlock<'_>,
) {
    if !E::NEEDS_THUNK {
        return;
    }
    let osec = &ctx.output_sections[id.index()];
    let shndx = osec.hdr.shndx;
    let func = |addr: u64| {
        let mut sym = ElfSym::<E>::default();
        sym.set_st_shndx(shndx);
        sym.set_st_value(addr);
        sym.set_type(STT_FUNC);
        sym
    };

    for thunk in &osec.thunks {
        let suffix = format!("${}", thunk.name);
        for (i, &sym) in thunk.symbols.iter().enumerate() {
            let addr = osec.hdr.shdr.sh_addr.get() + thunk.offset + thunk.offsets[i];
            let name = ctx.symbols[sym].name();
            block.push_synthetic::<E>(name, suffix.as_bytes(), func(addr));
            if E::FAMILY == Family::Arm32 {
                // Emit "$t", "$a" and "$d" if ARM32.
                block.push_mapping_symbol::<E>(crate::chunks::strtab::THUMB, func(addr));
                block.push_mapping_symbol::<E>(crate::chunks::strtab::ARM, func(addr + 4));
                block.push_mapping_symbol::<E>(crate::chunks::strtab::DATA, func(addr + 12));
            }
        }
    }

    // Thunks can be removed after their symbol-table space is reserved.
    // Zero the unused entries and names, as C++ mold does.
    block.zero_unused::<E>();
}
