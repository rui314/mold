//! `.got.plt`, function pointers used by the PLT.

use crate::arch::{Arch, Family};
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::util::endian::Endian;

// .got.plt is similar to .got in the sense that it is a table containing
// pointers. The contents in .got.plt are function pointers used by .plt.
pub fn new_header<E: Arch>(args: &crate::cmdline::Args) -> ChunkHeader<E> {
    let sh_type = if E::IS_PPC64 {
        SHT_NOBITS
    } else {
        SHT_PROGBITS
    };
    let mut hdr = ChunkHeader::<E>::new(".got.plt", sh_type, (SHF_ALLOC | SHF_WRITE) as u64);
    hdr.is_relro = args.z_now;
    hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
    hdr.shdr.sh_size.set(header_size::<E>());
    hdr
}

pub fn header_size<E: Arch>() -> u64 {
    let words = if E::FAMILY == Family::Ppc64V2 { 2 } else { 3 };
    words * E::WORD_SIZE as u64
}

pub fn entry_size<E: Arch>() -> u64 {
    let words = if E::FAMILY == Family::Ppc64V1 { 3 } else { 1 };
    words * E::WORD_SIZE as u64
}

pub fn update_shdr<E: Arch>(ctx: &mut Context<E>) {
    let size = header_size::<E>() + ctx.plt.symbols.len() as u64 * entry_size::<E>();
    ctx.gotplt.shdr.sh_size.set(size);
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    // On PPC64, it's dynamic loader responsibility to fill the .got.plt
    // section. Dynamic loader finds the address of the first PLT entry by
    // DT_PPC64_GLINK and assumes that each PLT entry is 4 bytes long.
    if E::IS_PPC64 {
        return;
    }
    let w = E::WORD_SIZE;
    let write = |buf: &mut [u8], idx: usize, val: u64| {
        if E::IS_64 {
            E::Endian::write_u64(&mut buf[idx * w..], val);
        } else {
            E::Endian::write_u32(&mut buf[idx * w..], val as u32);
        }
    };
    // The first slot of .got.plt points to _DYNAMIC, as requested by
    // the psABI. The second and the third slots are reserved by the psABI.
    write(
        buf,
        0,
        ctx.dynamic.as_ref().map_or(0, |d| d.shdr.sh_addr.get()),
    );
    write(buf, 1, 0);
    write(buf, 2, 0);
    for i in 0..ctx.plt.symbols.len() {
        write(buf, i + 3, ctx.plt.hdr.shdr.sh_addr.get());
    }
}
