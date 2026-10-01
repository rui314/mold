//! The sections -sectcreate makes from files and -add_empty_section
//! makes empty, and an empty one for a section only a boundary symbol
//! names.

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::target::Target;

/// A section the -sectcreate and -add_empty_section options make (see
/// SectCreateInput) of a name no input section has, or an empty one
/// for a section only a boundary symbol names.
#[derive(Debug)]
pub struct SectCreateSection {
    pub hdr: ChunkHeader,
    /// The options' contents, in file order.
    pub contents: &'static [u8],
    /// Made by -sectcreate or -add_empty_section rather than for a
    /// boundary symbol.
    pub from_option: bool,
}

impl SectCreateSection {
    pub fn new(
        segname: &'static [u8],
        sectname: &'static [u8],
        contents: &'static [u8],
        from_option: bool,
    ) -> Self {
        let mut hdr = ChunkHeader::new(segname, sectname);
        hdr.size = contents.len() as u64;
        Self { hdr, contents, from_option }
    }
}

/// The input section of a -sectcreate or -add_empty_section option.
/// ld-prime makes each option a file of one section, which takes the
/// option's place among the inputs - but the first option's section,
/// which it takes for its own (file 0) and so places after every
/// file's. It joins the output section of its name of the input
/// sections, among them in file order, or a section of the options'
/// own.
#[derive(Debug)]
pub struct SectCreateInput {
    pub size: u64,
    pub place: InputPlace,
}

/// Where a -sectcreate option's input section is laid out.
#[derive(Clone, Copy, Debug)]
pub enum InputPlace {
    /// In an input section's output section, as this subsection of the
    /// internal object.
    Isec(u32),
    /// In `Context::sectcreate_sections[section]`, at `offset`.
    Section { section: u32, offset: u64 },
}

/// The input-order priority of the file of option `i`: the linker's
/// own, after every file, for the first option.
pub fn file_priority<E: Target>(ctx: &Context<E>, i: usize) -> u32 {
    if i == 0 { u32::MAX } else { ctx.sectcreate_priority[i] }
}

impl SectCreateInput {
    /// The input section's address, and its output section's ordinal.
    pub fn place<E: Target>(&self, ctx: &Context<E>) -> (u64, u8) {
        match self.place {
            InputPlace::Isec(id) => {
                (ctx.isec_addr(id as usize), ctx.isec_n_sect(&ctx.isecs[id as usize]))
            }
            InputPlace::Section { section, offset } => {
                let hdr = &ctx.sectcreate_sections[section as usize].hdr;
                (hdr.addr + offset, hdr.n_sect)
            }
        }
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, idx: u32, buf: &mut [u8]) {
    let data = ctx.sectcreate_sections[idx as usize].contents;
    buf[..data.len()].copy_from_slice(data);
}
