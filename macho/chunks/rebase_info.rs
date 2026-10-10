//! This file creates the rebase opcode stream of LC_DYLD_INFO, the older of
//! the two forms in which an image tells dyld how to fix up its pointers
//! (see chained_fixups.rs for the newer one).
//!
//! An image is usually loaded at an address other than the one it was
//! linked at, so every pointer in it that holds an address in the image
//! itself must be adjusted by the difference (the "slide"). Mach-O calls
//! that a rebase; it is what `R_*_RELATIVE` relocations do on ELF (see
//! mold's elf/chunks/reldyn.rs). Instead of a table of relocations, the
//! rebase info is a compact program for a small state machine in dyld:
//! opcodes set the segment and the offset, advance the address, and rebase
//! a run of consecutive pointers. A non-PIE executable, which is never
//! slid, has none.

use mold_common::leb128::encode_uleb;

use crate::arch::Target;
use crate::chunks::{ChunkHeader, got, lazy_ptrs, objc_stubs, output_section, segment_and_offset};
use crate::context::Context;
use crate::macho::*;

/// The rebase opcode stream: every pointer dyld slides.
#[derive(Debug)]
pub struct RebaseInfoSection {
    pub hdr: ChunkHeader,
    /// The stream, built during layout.
    pub contents: Vec<u8>,
}

impl RebaseInfoSection {
    pub fn new() -> Self {
        Self { hdr: ChunkHeader::linkedit(), contents: Vec::new() }
    }
}

impl Default for RebaseInfoSection {
    fn default() -> Self {
        Self::new()
    }
}

/// Every pointer a loader must slide when the image lands at another
/// address than its own: those each chunk writes - the output sections'
/// (for absolute relocations to local targets, and in the synthesized
/// records), the synthesized selector references, the lazy pointers
/// and the GOT slots. Unsorted. The rebase stream describes them to
/// dyld, a -static -pie image's local relocations to whatever loads it,
/// and legacy LINKEDIT's to dyld.
pub fn rebase_locations<E: Target>(ctx: &Context<E>) -> Vec<u64> {
    let mut locs: Vec<u64> = Vec::new();
    output_section::rebase_locations(ctx, &mut locs);
    objc_stubs::rebase_locations(ctx, &mut locs);
    lazy_ptrs::rebase_locations(ctx, &mut locs);
    got::rebase_locations(ctx, &mut locs);
    locs
}

/// Whether nothing ever slides the image: a non-PIE executable, which
/// the kernel maps where its segments say and dyld leaves there.
/// ld-prime records no rebases for one, in opcodes or in chains. (A
/// -static image keeps them for whatever loads it.)
pub fn is_never_slid<E: Target>(ctx: &Context<E>) -> bool {
    ctx.args.output_type == MH_EXECUTE && !ctx.args.pie && !ctx.args.static_link
}

/// Builds the rebase opcode stream: it tells dyld which pointers in the
/// image it must slide when the image is loaded at a non-default address.
/// Every absolute address the linker writes into a data section gets a
/// record. Runs during layout, once every segment before __LINKEDIT has
/// an address.
pub fn construct<E: Target>(ctx: &Context<E>) -> Vec<u8> {
    if is_never_slid(ctx) {
        return Vec::new();
    }
    let mut locs = rebase_locations(ctx);
    if locs.is_empty() {
        return Vec::new();
    }
    locs.sort_unstable();

    let mut buf = vec![REBASE_OPCODE_SET_TYPE_IMM | REBASE_TYPE_POINTER];
    for op in rebase_ops(ctx, &locs) {
        match op {
            Op::SegOffset(seg, off) => {
                buf.push(REBASE_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB | seg as u8);
                encode_uleb(&mut buf, off);
            }
            Op::AddAddr(delta) if delta <= 15 * 8 && delta % 8 == 0 => {
                buf.push(REBASE_OPCODE_ADD_ADDR_IMM_SCALED | (delta / 8) as u8)
            }
            Op::AddAddr(delta) => {
                buf.push(REBASE_OPCODE_ADD_ADDR_ULEB);
                encode_uleb(&mut buf, delta);
            }
            Op::Rebase(count) if count <= 15 => {
                buf.push(REBASE_OPCODE_DO_REBASE_IMM_TIMES | count as u8)
            }
            Op::Rebase(count) => {
                buf.push(REBASE_OPCODE_DO_REBASE_ULEB_TIMES);
                encode_uleb(&mut buf, count);
            }
        }
    }
    buf.push(REBASE_OPCODE_DONE);
    while !buf.len().is_multiple_of(8) {
        buf.push(REBASE_OPCODE_DONE);
    }
    buf
}

/// A rebase opcode before encoding.
#[derive(Clone, Copy)]
enum Op {
    SegOffset(usize, u64),
    AddAddr(u64),
    Rebase(u64),
}

/// The opcodes rebasing the sorted `locs`: the address moves by
/// ADD_ADDR_ULEB within a segment and by SET_SEGMENT_AND_OFFSET_ULEB
/// into another one, and a run of adjacent pointers is one
/// DO_REBASE_ULEB_TIMES, since each rebase advances the address past
/// its slot. A run ends at its segment's end, where dyld's bounds
/// check would reject a rebase past it.
fn rebase_ops<E: Target>(ctx: &Context<E>, locs: &[u64]) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut seg_start = 0;
    let mut seg_end = 0;
    let mut cur_addr = 0;
    let mut i = 0;
    while i < locs.len() {
        let addr = locs[i];
        if addr < seg_start || seg_end <= addr {
            let (seg, off) = segment_and_offset(ctx, addr);
            seg_start = addr - off;
            seg_end = seg_start + ctx.segments[seg].cmd.vmsize.get();
            ops.push(Op::SegOffset(seg, off));
        } else if addr != cur_addr {
            ops.push(Op::AddAddr(addr.wrapping_sub(cur_addr)));
        }
        let mut n = 1;
        while i + n < locs.len() && locs[i + n] == addr + n as u64 * 8 && locs[i + n] < seg_end {
            n += 1;
        }
        ops.push(Op::Rebase(n as u64));
        cur_addr = addr + n as u64 * 8;
        i += n;
    }
    ops
}
