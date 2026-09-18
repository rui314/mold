//! `.interp`, the dynamic linker's pathname.

use crate::arch::Arch;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::util::write_cstr;

// .interp contains the pathname of a dynamic linker. Dynamically-linked
// executables have the section. If exists, the kernel runs the program at
// the specified path with the executable pathname as an argument,
// allowing the dynamic linker to run the program.
pub fn new_header<E: Layout>() -> ChunkHeader<E> {
    ChunkHeader::<E>::new(".interp", SHT_PROGBITS, SHF_ALLOC as u64)
}

pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
    let size = ctx.args.dynamic_linker.as_os_str().len() as u64 + 1;
    ctx.interp.as_mut().unwrap().shdr.sh_size.set(size);
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    write_cstr(buf, ctx.args.dynamic_linker.as_os_str().as_encoded_bytes());
}
