//! `.eh_frame`, `.eh_frame_hdr` and `.rela.eh_frame`.
//!
//! `.eh_frame` is reconstructed rather than copied: FDEs of dead functions
//! are dropped, identical CIEs are merged, and a sorted lookup table is
//! emitted as `.eh_frame_hdr` so that the unwinder can find the FDE for a
//! PC by binary search.

use rayon::prelude::*;
use std::ptr::NonNull;

use crate::arch::Arch;
use crate::context::Context;
use crate::elf::*;
use crate::input_files::ObjectFile;
use crate::input_sections::CieRecord;
use crate::output_chunks::ChunkHeader;
use crate::output_file::split_at_offsets;
use crate::symbol::Symbol;
use crate::util::is_int;
use crate::{error, fatal};

/// A stable handle used while deduplicating CIEs. C++ mold keeps direct
/// `CieRecord *` leaders; the owner pointer is needed here because Rust keeps
/// the file-dependent relocation and symbol tables outside the record.
#[derive(Clone, Copy)]
pub(crate) struct CieHandle<E: Layout> {
    file: NonNull<ObjectFile<E>>,
    cie: NonNull<CieRecord>,
}

impl<E: Layout> CieHandle<E> {
    /// Creates a handle while object files and their CIE vectors are stable.
    ///
    /// # Safety
    ///
    /// Both pointers must remain valid until the handle is discarded, and the
    /// CIE vector must not be reallocated. Mutations through a handle must not
    /// overlap a call to [`Self::equals`] involving that handle.
    #[inline]
    pub(crate) unsafe fn new(file: *mut ObjectFile<E>, cie: *mut CieRecord) -> CieHandle<E> {
        CieHandle {
            file: unsafe { NonNull::new_unchecked(file) },
            cie: unsafe { NonNull::new_unchecked(cie) },
        }
    }

    #[inline]
    pub(crate) fn equals(self, other: CieHandle<E>) -> bool {
        // SAFETY: construction guarantees that both records and owners remain
        // live, and callers compare records only while they are not mutating.
        unsafe {
            cie_equals::<E>(
                self.file.as_ref(),
                self.cie.as_ref(),
                other.file.as_ref(),
                other.cie.as_ref(),
            )
        }
    }

    #[inline]
    pub(crate) fn size(self) -> usize {
        // SAFETY: the handle's owner and record remain live.
        unsafe { self.cie.as_ref().size::<E>() }
    }

    #[inline]
    pub(crate) fn icf_idx(self, value: u32) {
        // SAFETY: callers mutate only the current record, which is not yet in
        // the leader list and is not being compared concurrently.
        unsafe { (*self.cie.as_ptr()).icf_idx = value };
    }

    #[inline]
    fn output_offset(self) -> u32 {
        // SAFETY: the handle's record remains live.
        unsafe { self.cie.as_ref().output_offset }
    }

    #[inline]
    fn set_output_offset(self, value: u32) {
        // SAFETY: layout construction is serial and owns every CIE record.
        unsafe { (*self.cie.as_ptr()).output_offset = value };
    }

    #[inline]
    fn set_leader(self) {
        // SAFETY: layout construction is serial and owns every CIE record.
        unsafe { (*self.cie.as_ptr()).is_leader = true };
    }
}

// .eh_frame contains runtime information as to how to handle exceptions
// for each function. Each input object file contains one .eh_frame section.
// We parse input .eh_frame sections, merge their contents and emit the
// merged information to .eh_frame.
#[derive(Debug)]
pub struct EhFrameSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
}

impl<E: Arch> EhFrameSection<E> {
    pub fn new() -> EhFrameSection<E> {
        let mut hdr = ChunkHeader::<E>::new(".eh_frame", SHT_PROGBITS, SHF_ALLOC as u64);
        hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
        EhFrameSection { hdr }
    }
}

impl<E: Arch> Default for EhFrameSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether two CIEs are identical, including their relocations.
pub fn cie_equals<E: Layout>(
    a_file: &ObjectFile<E>,
    a: &CieRecord,
    b_file: &ObjectFile<E>,
    b: &CieRecord,
) -> bool {
    if a.contents::<E>() != b.contents::<E>() {
        return false;
    }
    let x = a.rels::<E>(a_file);
    let y = b.rels::<E>(b_file);
    x.len() == y.len()
        && x.iter().zip(y).all(|(rx, ry)| {
            rx.r_offset() - a.input_offset as u64 == ry.r_offset() - b.input_offset as u64
                && rx.r_type() == ry.r_type()
                && a_file.base.symbols[rx.r_sym() as usize]
                    == b_file.base.symbols[ry.r_sym() as usize]
                && rx.r_addend() == ry.r_addend()
        })
}

pub fn construct<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("eh_frame");

    // Remove dead FDEs and assign them offsets within their corresponding
    // CIE group.
    ctx.objs.par_iter_mut().for_each(|file| {
        file.fdes.retain(|fde| fde.is_alive());
        let mut offset = 0;
        let cies = &file.cies;
        for fde in &mut file.fdes {
            fde.output_offset = offset as u32;
            offset += fde.size_with::<E>(cies);
        }
        file.fde_size = offset as u64;
    });

    // Uniquify CIEs and assign offsets to them.
    let mut leaders: Vec<CieHandle<E>> = Vec::new();
    let mut offset = 0u64;
    for file in &mut ctx.objs {
        let file_ptr = file as *mut ObjectFile<E>;
        let cies = file.cies.as_mut_ptr();
        for ci in 0..file.cies.len() {
            // SAFETY: files are boxed and no CIE vector is resized during
            // layout construction, so both addresses remain stable.
            let cie = unsafe { CieHandle::new(file_ptr, cies.add(ci)) };
            let leader = leaders.iter().copied().find(|&leader| leader.equals(cie));
            match leader {
                Some(leader) => cie.set_output_offset(leader.output_offset()),
                None => {
                    cie.set_output_offset(offset as u32);
                    cie.set_leader();
                    offset += cie.size() as u64;
                    leaders.push(cie);
                }
            }
        }
    }

    // Assign FDE offsets to files.
    let mut idx = 0u64;
    for file in &mut ctx.objs {
        file.fde_idx = idx;
        idx += file.fdes.len() as u64;
        file.fde_offset = offset;
        offset += file.fde_size;
    }

    // .eh_frame must end with a null word.
    ctx.eh_frame.hdr.shdr.sh_size.set(offset + 4);
}

// Write to .eh_frame and .eh_frame_hdr.
pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8], hdr_buf: Option<&mut [u8]>) {
    let sh_addr = ctx.eh_frame.hdr.shdr.sh_addr.get();
    let sh_size = ctx.eh_frame.hdr.shdr.sh_size.get();

    // Each file owns the range of its FDEs; leader CIEs are written by
    // their files too. Since CIE ranges precede all FDE ranges, both are
    // covered by splitting at CIE and FDE boundaries. A file without FDEs
    // owns no range: its FDE offset coincides with the next file's.
    let mut bounds: Vec<u64> = Vec::new();
    for file in &ctx.objs {
        for cie in &file.cies {
            if cie.is_leader {
                bounds.push(cie.output_offset as u64);
            }
        }
    }
    for file in &ctx.objs {
        if !file.fdes.is_empty() {
            bounds.push(file.fde_offset);
        }
    }
    debug_assert!(bounds.windows(2).all(|pair| pair[0] < pair[1]));
    let mut slices = split_at_offsets(buf, &bounds).into_iter();

    let hdr_addr = ctx.eh_frame_hdr.as_ref().map(|h| h.hdr.shdr.sh_addr.get());

    let write_cie = |file: &ObjectFile<E>, cie: &CieRecord, dst: &mut [u8]| {
        let contents = cie.contents::<E>();
        dst[..contents.len()].copy_from_slice(contents);
        if ctx.args.relocatable {
            return;
        }
        for rel in cie.rels::<E>(file) {
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];
            let loc = (rel.r_offset() - cie.input_offset as u64) as usize;
            let val = sym
                .addr(ctx)
                .wrapping_add(file.section_at(cie.section).rel_addend::<E>(rel) as u64);
            let p = sh_addr + cie.output_offset as u64 + loc as u64;
            E::apply_eh_reloc(
                ctx,
                file.section_at(cie.section),
                rel,
                &mut dst[loc..],
                p,
                val,
            );
        }
    };

    // Gather per-file work items with their slices.
    struct Item<'a, E: Layout> {
        file: &'a ObjectFile<E>,
        cies: Vec<(usize, &'a mut [u8])>,
        fdes: Option<&'a mut [u8]>,
        hdr_entries: Option<&'a mut [u8]>,
    }
    let mut items: Vec<Item<E>> = ctx
        .objs
        .iter()
        .map(|file| Item {
            file,
            cies: Vec::new(),
            fdes: None,
            hdr_entries: None,
        })
        .collect();

    for (fi, file) in ctx.objs.iter().enumerate() {
        for (ci, cie) in file.cies.iter().enumerate() {
            if cie.is_leader {
                let slice = slices.next().unwrap();
                items[fi].cies.push((ci, slice));
            }
        }
    }
    for (fi, file) in ctx.objs.iter().enumerate() {
        if !file.fdes.is_empty() {
            items[fi].fdes = Some(slices.next().unwrap());
        }
    }
    debug_assert!(slices.next().is_none());

    let base = EhFrameHdrSection::<E>::HEADER_SIZE as usize;
    let mut hdr_table = hdr_buf.map(|buf| &mut buf[base..]);
    if let Some(table) = hdr_table.as_deref_mut() {
        let mut rest = table;
        for (item, file) in items.iter_mut().zip(&ctx.objs) {
            if !file.fdes.is_empty() {
                let len = file.fdes.len() * 8;
                let (own, tail) = std::mem::take(&mut rest).split_at_mut(len);
                item.hdr_entries = Some(own);
                rest = tail;
            }
        }
        debug_assert!(rest.is_empty());
    }

    items.into_par_iter().for_each(|mut item| {
        let file = item.file;

        // Copy CIEs.
        for (ci, dst) in item.cies {
            write_cie(file, &file.cies[ci], dst);
        }
        let Some(fde_buf) = item.fdes else {
            return;
        };

        // Copy FDEs.
        for (i, fde) in file.fdes.iter().enumerate() {
            let rels = fde.rels::<E>(file);
            let offset = file.fde_offset + fde.output_offset as u64;
            let dst = &mut fde_buf[fde.output_offset as usize..];
            let contents = fde.contents::<E>(file);
            dst[..contents.len()].copy_from_slice(contents);

            let cie = &file.cies[fde.cie_idx as usize];
            E::Endian::write_u32(
                &mut dst[4..],
                (offset + 4 - cie.output_offset as u64) as u32,
            );
            if ctx.args.relocatable {
                continue;
            }

            // Always set in the loop below because parse_ehframe() discards
            // FDEs that have no relocations.
            let mut func_addr = 0u64;
            for (j, rel) in rels.iter().enumerate() {
                let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];
                let loc = (rel.r_offset() - fde.input_offset as u64) as usize;
                let val = sym
                    .addr(ctx)
                    .wrapping_add(file.section_at(cie.section).rel_addend::<E>(rel) as u64);
                let p = sh_addr + offset + loc as u64;
                E::apply_eh_reloc(
                    ctx,
                    file.section_at(cie.section),
                    rel,
                    &mut dst[loc..],
                    p,
                    val,
                );
                if j == 0 {
                    func_addr = val;
                }
            }

            let Some(origin) = hdr_addr else {
                continue;
            };
            // Write to .eh_frame_hdr
            let entry = &mut item.hdr_entries.as_deref_mut().unwrap()[i * 8..][..8];

            // Compilers may emit a meaningless FDE covering a zero-length
            // address range. Such an FDE must not be written to
            // .eh_frame_hdr because another function may start at the same
            // address, and a binary search on .eh_frame_hdr could then find
            // the empty FDE instead of the real one. We write a tombstone
            // value instead.
            let ptr = &dst[8 + cie.fde_ptr_size as usize..];
            let range = if cie.fde_ptr_size == 4 {
                E::Endian::read_u32(ptr) as u64
            } else {
                E::Endian::read_u64(ptr)
            };
            if range == 0 {
                E::Endian::write_i32(entry, i32::MAX);
                E::Endian::write_i32(&mut entry[4..], 0);
            } else {
                // The table entries are 32-bit offsets from .eh_frame_hdr.
                if !is_int(func_addr.wrapping_sub(origin) as i64, 32) {
                    let sym = &ctx.symbols[file.base.symbols[rels[0].r_sym() as usize]];
                    error!("{file}: {sym}: address out of range of .eh_frame_hdr");
                }
                E::Endian::write_i32(entry, func_addr.wrapping_sub(origin) as i32);
                E::Endian::write_i32(
                    &mut entry[4..],
                    (sh_addr + offset).wrapping_sub(origin) as i32,
                );
            }
        }
    });

    // Write a terminator.
    E::Endian::write_u32(&mut buf[sh_size as usize - 4..], 0);

    // Sort .eh_frame_hdr contents.
    if let Some(table) = hdr_table {
        let (entries, remainder) = table.as_chunks_mut::<8>();
        debug_assert!(remainder.is_empty());
        entries.par_sort_unstable_by_key(|entry| E::Endian::read_i32(entry));
    }
}

/// Reports an `.eh_frame` relocation whose value doesn't fit.
pub fn check_range<E: Arch>(
    ctx: &Context<E>,
    isec: &crate::input_sections::InputSection,
    rel: &ElfRel<E>,
    val: i64,
    lo: i64,
    hi: i64,
) {
    if val < lo || hi <= val {
        let file = &ctx.objs[isec.file.index()];
        let sym: &Symbol = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];
        error!(
            "{}: relocation {} against {sym} out of range: {val} is not in [{lo}, {hi})",
            isec.display(file),
            rel.type_name::<E>()
        );
    }
}

// .eh_frame_hdr is a lookup table for .eh_frame. Entries in .eh_frame_hdr
// are sorted by their dcorresponding function addresses, so tha the
// runtime can quickly find an exception-handling record for the current
// function by binary search. Without .eh_frame_hdr, the runtime would
// have had to do linear search in .eh_frame.
#[derive(Debug)]
pub struct EhFrameHdrSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
    pub num_fdes: u64,
}

impl<E: Layout> EhFrameHdrSection<E> {
    pub const HEADER_SIZE: u64 = 12;

    pub fn new() -> EhFrameHdrSection<E> {
        let mut hdr = ChunkHeader::<E>::new(".eh_frame_hdr", SHT_PROGBITS, SHF_ALLOC as u64);
        hdr.shdr.sh_addralign.set(4);
        hdr.shdr.sh_size.set(Self::HEADER_SIZE);
        EhFrameHdrSection { hdr, num_fdes: 0 }
    }
}

impl<E: Layout> Default for EhFrameHdrSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

pub mod eh_frame_hdr {
    use super::*;

    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
        let num_fdes: u64 = ctx.objs.iter().map(|f| f.fdes.len() as u64).sum();
        let sec = ctx.eh_frame_hdr.as_mut().unwrap();
        sec.num_fdes = num_fdes;
        sec.hdr
            .shdr
            .sh_size
            .set(EhFrameHdrSection::<E>::HEADER_SIZE + num_fdes * 8);
    }

    pub fn write_header<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
        let sec = ctx.eh_frame_hdr.as_ref().unwrap();

        // Write a header. The actual table is written by EhFrameSection::copy_buf.
        buf[0] = 1;
        buf[1] = (DW_EH_PE_pcrel | DW_EH_PE_sdata4) as u8;
        buf[2] = DW_EH_PE_udata4 as u8;
        buf[3] = (DW_EH_PE_datarel | DW_EH_PE_sdata4) as u8;
        E::Endian::write_u32(
            &mut buf[4..],
            ctx.eh_frame
                .hdr
                .shdr
                .sh_addr
                .get()
                .wrapping_sub(sec.hdr.shdr.sh_addr.get())
                .wrapping_sub(4) as u32,
        );
        E::Endian::write_u32(&mut buf[8..], sec.num_fdes as u32);
    }
}

// EhFrameRelocSection contains relocation records for .eh_frame. It is used
// only for relocatable outputs (an .o file rather than an executable or .so).
#[derive(Debug)]
pub struct EhFrameRelocSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
}

impl<E: Arch> EhFrameRelocSection<E> {
    pub fn new() -> EhFrameRelocSection<E> {
        let (name, ty) = if E::IS_RELA {
            (".rela.eh_frame", SHT_RELA)
        } else {
            (".rel.eh_frame", SHT_REL)
        };
        let mut hdr = ChunkHeader::<E>::new(name, ty, SHF_INFO_LINK as u64);
        hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
        hdr.shdr
            .sh_entsize
            .set(std::mem::size_of::<ElfRel<E>>() as u64);
        EhFrameRelocSection { hdr }
    }
}

impl<E: Arch> Default for EhFrameRelocSection<E> {
    fn default() -> Self {
        Self::new()
    }
}

pub mod eh_frame_reloc {
    use super::*;

    pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
        let count: usize = ctx
            .objs
            .par_iter()
            .map(|file| {
                let cies: usize = file
                    .cies
                    .iter()
                    .filter(|c| c.is_leader)
                    .map(|c| c.rels::<E>(file).len())
                    .sum();
                let fdes: usize = file.fdes.iter().map(|f| f.rels::<E>(file).len()).sum();
                cies + fdes
            })
            .sum();
        let sec = ctx.eh_frame_reloc.as_mut().unwrap();
        sec.hdr
            .shdr
            .sh_size
            .set((count * std::mem::size_of::<ElfRel<E>>()) as u64);
        sec.hdr.shdr.sh_link.set(ctx.symtab.hdr.shndx);
        sec.hdr.shdr.sh_info.set(ctx.eh_frame.hdr.shndx);
    }

    /// Writes the relocations; with REL and `-r`, addends are written into
    /// `.eh_frame` itself, which is passed as `eh_frame_buf`.
    pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8], eh_frame_buf: Option<&mut [u8]>) {
        let out = rels_from_bytes_mut::<E>(buf);
        let mut eh_frame_buf = eh_frame_buf;
        let mut n = 0;

        let mut copy = |file: &ObjectFile<E>,
                        shndx: u32,
                        r: &ElfRel<E>,
                        offset: u64,
                        eh_frame_buf: &mut Option<&mut [u8]>| {
            let isec = file.section_at(shndx);
            let sym = &ctx.symbols[file.base.symbols[r.r_sym() as usize]];
            let mut rel = ElfRel::<E>::new(
                ctx.eh_frame.hdr.shdr.sh_addr.get() + offset,
                r.r_type(),
                0,
                0,
            );

            if sym.st_type() == STT_SECTION {
                // We discard section symbols in input files and re-create new
                // ones for each output section. So we need to adjust relocations'
                // addends if they refer a section symbol.
                let target = sym.input_section_ref(ctx).unwrap();
                rel.set_r_sym(ctx.output_section(target.output_section.unwrap()).hdr.shndx);
                let addend = isec.rel_addend::<E>(r) + target.offset() as i64;
                if E::IS_RELA {
                    rel.set_r_addend(addend);
                } else if ctx.args.relocatable {
                    if let Some(eh) = eh_frame_buf {
                        E::write_addend(&mut eh[offset as usize..], addend, r);
                    }
                }
            } else {
                rel.set_r_sym(sym.output_sym_idx(ctx));
                if E::IS_RELA {
                    rel.set_r_addend(isec.rel_addend::<E>(r));
                }
            }
            out[n] = rel;
            n += 1;
        };

        for file in &ctx.objs {
            for cie in &file.cies {
                if cie.is_leader {
                    for rel in cie.rels::<E>(file) {
                        let offset =
                            cie.output_offset as u64 + rel.r_offset() - cie.input_offset as u64;
                        copy(file, cie.section, rel, offset, &mut eh_frame_buf);
                    }
                }
            }
            for fde in &file.fdes {
                let cie = &file.cies[fde.cie_idx as usize];
                let base = file.fde_offset + fde.output_offset as u64;
                for rel in fde.rels::<E>(file) {
                    let offset = base + rel.r_offset() - fde.input_offset as u64;
                    copy(file, cie.section, rel, offset, &mut eh_frame_buf);
                }
            }
        }
    }
}

/// Fatal error for `.eh_frame` contents that can't be handled.
pub fn unsupported<E: Arch>(rel: &ElfRel<E>) -> ! {
    fatal!(
        "unsupported relocation in .eh_frame: {}",
        rel.type_name::<E>()
    )
}
