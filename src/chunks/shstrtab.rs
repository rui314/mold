//! `.shstrtab`, output section names.

use std::collections::HashMap;

use bstr::BStr;

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::target::Target;
use crate::util::write_cstr;

// .shstrtab contains section names, such as ".text" or ".data". Just like
// .strtab, .shstrtab is not needed at runtime. One can remove .shstrtab
// and section table from an executable without breaking it.
pub fn new_header<E: Layout>() -> ChunkHeader<E> {
    ChunkHeader::<E>::new(".shstrtab", SHT_STRTAB, 0)
}

pub fn update_shdr<E: Target>(ctx: &mut Context<E>) {
    let mut map: HashMap<&'static BStr, u64> = HashMap::new();
    let mut offset = 1u64;
    for i in 0..ctx.chunks.len() {
        let id = ctx.chunks[i];
        if id.is_header() {
            continue;
        }
        let name = ctx.chunk_header(id).name;
        if name.is_empty() {
            continue;
        }
        let off = *map.entry(name).or_insert_with(|| {
            let off = offset;
            offset += name.len() as u64 + 1;
            off
        });
        ctx.chunk_header_mut(id).shdr.sh_name.set(off as u32);
    }
    ctx.shstrtab.as_mut().unwrap().shdr.sh_size.set(offset);
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    buf[0] = 0;
    for &id in &ctx.chunks {
        let hdr = ctx.chunk_header(id);
        if hdr.shdr.sh_name.get() != 0 {
            write_cstr(&mut buf[hdr.shdr.sh_name.get() as usize..], hdr.name);
        }
    }
}
