//! LC_DATA_IN_CODE: ranges inside __text that hold data (jump tables, inline
//! constants), so disassemblers and the signature verifier can treat them
//! as bytes.

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::input_sections::InputSection;
use crate::target::Target;

/// LC_DATA_IN_CODE: ranges inside __text that hold data (jump tables,
/// inline constants), so disassemblers and the signature verifier can
/// treat them as bytes.
#[derive(Debug)]
pub struct DataInCodeSection {
    pub hdr: ChunkHeader,
    /// The entries (fileoff, length, kind), built once when layout
    /// reaches __LINKEDIT.
    pub entries: Vec<(u32, u16, u16)>,
}

impl DataInCodeSection {
    pub fn new() -> Self {
        Self { hdr: ChunkHeader::linkedit(), entries: Vec::new() }
    }
}

impl Default for DataInCodeSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    write_entries(&ctx.data_in_code.entries, buf);
}

/// Writes data-in-code entries (offset, length, kind) at the start of
/// `buf`, 8 bytes each.
pub fn write_entries(entries: &[(u32, u16, u16)], buf: &mut [u8]) {
    let mut p = 0;
    for &(off, len, kind) in entries {
        buf[p..p + 4].copy_from_slice(&off.to_le_bytes());
        buf[p + 4..p + 6].copy_from_slice(&len.to_le_bytes());
        buf[p + 6..p + 8].copy_from_slice(&kind.to_le_bytes());
        p += 8;
    }
}

/// Live data-in-code entries as (subsection, offset within it,
/// length, kind). In an object, an entry's offset is an address in
/// the object's own address space (sections there are laid out from
/// zero), which find_subsec maps to the owning subsection. Entries go
/// with their subsection: a dead-stripped one's vanish, and so do those
/// of a coalesced-away weak copy or a folded function, since the copy
/// that stays brings its own (ld64 writes each range once).
fn live_entries<E: Target>(
    ctx: &Context<E>,
) -> impl Iterator<Item = (&InputSection, u64, u16, u16)> {
    ctx.objs.iter().filter(|obj| obj.is_alive).flat_map(move |obj| {
        obj.dice.iter().filter_map(move |&(off, len, kind)| {
            let (isec, off_in) =
                crate::input_files::find_subsec(&ctx.isecs, &obj.subsecs, off as u64)?;
            let isec = &ctx.isecs[isec];
            isec.is_emitted().then_some((isec, off_in, len, kind))
        })
    })
}

/// Builds the LC_DATA_IN_CODE entries, sorted: each where `pos` puts
/// its output section - at its file offset in a final image, its
/// address in an object (a -r output) - plus the entry's offset there.
/// A final link runs it when layout reaches __LINKEDIT: the __text file
/// offsets the entries record are final by then, so the table is built
/// exactly once (sold builds its contents in compute_size the same way)
/// and copied out verbatim.
pub fn build<E: Target>(
    ctx: &Context<E>,
    pos: impl Fn(&ChunkHeader) -> u64,
) -> Vec<(u32, u16, u16)> {
    let mut out: Vec<(u32, u16, u16)> = live_entries(ctx)
        .map(|(isec, off_in, len, kind)| {
            let off =
                pos(ctx.chunk_header(isec.output_section().unwrap())) + isec.offset as u64 + off_in;
            (off as u32, len, kind)
        })
        .collect();
    out.sort_unstable();
    out
}
