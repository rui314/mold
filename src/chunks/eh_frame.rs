//! `.eh_frame`, reconstructed exception-handling records.
//!
//! `.eh_frame` is reconstructed rather than copied: FDEs of dead functions
//! are dropped, identical CIEs are merged, and a sorted lookup table is
//! emitted as `.eh_frame_hdr` so that the unwinder can find the FDE for a
//! PC by binary search.

use rayon::prelude::*;

use crate::arch::Arch;
use crate::chunks::ChunkHeader;
use crate::chunks::eh_frame_hdr::EhFrameHdrSection;
use crate::context::Context;
use crate::elf::*;
use crate::input_files::ObjectFile;
use crate::input_sections::CieRecord;
use crate::output_file::split_at_offsets;
use crate::symbol::Symbol;
use crate::util::endian::Endian;
use crate::util::is_int;
use crate::{error, fatal};

/// Visits CIEs in input order, reusing the value assigned to an equivalent
/// leader. The visitor only updates layout/ICF metadata, not CIE contents.
pub(crate) fn deduplicate_cies<E: Arch>(
    ctx: &mut Context<E>,
    mut assign: impl FnMut(&mut CieRecord, Option<u32>) -> u32,
) {
    let mut leaders: Vec<(crate::input_files::ObjId, usize, u32)> = Vec::new();
    for i in 0..ctx.objs.len() {
        let id = ctx.objs.live_file(i).id();
        for ci in 0..ctx.objs[id.index()].cies.len() {
            let file = &ctx.objs[id.index()];
            let leader = leaders
                .iter()
                .find(|&&(owner, index, _)| {
                    let other = &ctx.objs[owner.index()];
                    cie_equals::<E>(other, &other.cies[index], file, &file.cies[ci])
                })
                .map(|&(_, _, value)| value);
            let value = assign(&mut ctx.objs[id.index()].cies[ci], leader);
            if leader.is_none() {
                leaders.push((id, ci, value));
            }
        }
    }
}

// .eh_frame contains runtime information as to how to handle exceptions
// for each function. Each input object file contains one .eh_frame section.
// We parse input .eh_frame sections, merge their contents and emit the
// merged information to .eh_frame.
pub fn new_header<E: Arch>() -> ChunkHeader<E> {
    let mut hdr = ChunkHeader::<E>::new(".eh_frame", SHT_PROGBITS, SHF_ALLOC as u64);
    hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
    hdr
}

/// Whether two CIEs are identical, including their relocations.
pub fn cie_equals<E: Arch>(
    a_file: &ObjectFile<E>,
    a: &CieRecord,
    b_file: &ObjectFile<E>,
    b: &CieRecord,
) -> bool {
    if a.contents::<E>() != b.contents::<E>() {
        return false;
    }
    let x = a.rels(a_file);
    let y = b.rels(b_file);
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
    let mut offset = 0u64;
    deduplicate_cies(ctx, |cie, leader| {
        cie.output_offset = leader.unwrap_or_else(|| {
            let start = offset as u32;
            cie.is_leader = true;
            offset += cie.size::<E>() as u64;
            start
        });
        cie.output_offset
    });

    // Assign FDE offsets to files.
    for file in &mut ctx.objs {
        file.fde_offset = offset;
        offset += file.fde_size;
    }

    // .eh_frame must end with a null word.
    ctx.eh_frame.shdr.sh_size.set(offset + 4);
}

// Write to .eh_frame and .eh_frame_hdr.
pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8], hdr_buf: Option<&mut [u8]>) {
    let sh_addr = ctx.eh_frame.shdr.sh_addr.get();
    let sh_size = ctx.eh_frame.shdr.sh_size.get();

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
        for rel in cie.rels(file) {
            let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];
            let loc = (rel.r_offset() - cie.input_offset as u64) as usize;
            let val =
                sym.addr(ctx).wrapping_add(file.section_at(cie.section).rel_addend(rel) as u64);
            let p = sh_addr + cie.output_offset as u64 + loc as u64;
            E::apply_eh_reloc(ctx, file.section_at(cie.section), rel, &mut dst[loc..], p, val);
        }
    };

    // Gather per-file work items with their slices.
    struct Item<'a, E: Arch> {
        file: &'a ObjectFile<E>,
        cies: Vec<(usize, &'a mut [u8])>,
        fdes: Option<&'a mut [u8]>,
        hdr_entries: Option<&'a mut [u8]>,
    }
    let mut items: Vec<Item<E>> = ctx
        .objs
        .iter()
        .map(|file| Item { file, cies: Vec::new(), fdes: None, hdr_entries: None })
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
                let own = rest.split_off_mut(..len).unwrap();
                item.hdr_entries = Some(own);
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
            let rels = fde.rels(file);
            let offset = file.fde_offset + fde.output_offset as u64;
            let dst = &mut fde_buf[fde.output_offset as usize..];
            let contents = fde.contents::<E>(file);
            dst[..contents.len()].copy_from_slice(contents);

            let cie = &file.cies[fde.cie_idx as usize];
            E::Endian::write_u32(&mut dst[4..], (offset + 4 - cie.output_offset as u64) as u32);
            if ctx.args.relocatable {
                continue;
            }

            // Always set in the loop below because parse_ehframe() discards
            // FDEs that have no relocations.
            let mut func_addr = 0u64;
            for (j, rel) in rels.iter().enumerate() {
                let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];
                let loc = (rel.r_offset() - fde.input_offset as u64) as usize;
                let val =
                    sym.addr(ctx).wrapping_add(file.section_at(cie.section).rel_addend(rel) as u64);
                let p = sh_addr + offset + loc as u64;
                E::apply_eh_reloc(ctx, file.section_at(cie.section), rel, &mut dst[loc..], p, val);
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
    isec: &crate::input_sections::InputSection<E>,
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

/// Fatal error for `.eh_frame` contents that can't be handled.
pub fn unsupported<E: Arch>(rel: &ElfRel<E>) -> ! {
    fatal!("unsupported relocation in .eh_frame: {}", rel.type_name::<E>())
}
