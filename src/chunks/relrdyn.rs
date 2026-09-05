//! `.relr.dyn`, packed base relocations.

use crate::arch::Arch;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;
use crate::util::endian::Endian;

// .relr.dyn is a relatively new section to contain base relocation
// information.
//
// A relocatable executable/DSO contains a lot of certain type of
// relocation entries, called "base relocations", to specify the locations
// of pointers in the FILE that need to be adjusted according to the
// desired load address and the actual load address. As an example,
// consider the following C code.
//
// extern int foo;
// int *bar = &foo;
//
// If an executable containing the above code is built as relocatable
// executable, meaning that the executable can be loaded not to a specific
// address in memory but anywhere in the virtual address space, then the
// pointer `bar`'s address is not known at link-time.
//
// The linker temporarily links the executable to a base address, record
// that information to the ELF header, and emits dynamic relocations to
// refer to the location of `bar`. At runtime, the loader adds the
// difference of the expected load address and the actual one to the
// pointer value to fix the pointer value.
//
// Relocatable executables/DSOs usually contain a fairly large number of
// base relocations. In particular, C++ virtual function table is an array
// of statically-initialized pointers which need base relocations.
//
// Notice that base relocations don't contain symbol information. They
// need only pointer locations in the ELF file that need fixing at
// load-time. Therefore, storing that information to the usual ELF
// relocation table is waste of space.
//
// .relr.dyn is designed to store base relocations in a space-efficient way.
#[derive(Debug)]
pub struct RelrDynSection<E: Layout> {
    pub hdr: ChunkHeader<E>,
}

impl<E: Arch> RelrDynSection<E> {
    pub fn new(args: &crate::cmdline::Args) -> RelrDynSection<E> {
        let ty = if args.use_android_relr_tags {
            SHT_ANDROID_RELR
        } else {
            SHT_RELR
        };
        let mut hdr = ChunkHeader::<E>::new(".relr.dyn", ty, SHF_ALLOC as u64);
        hdr.shdr.sh_entsize.set(E::WORD_SIZE as u64);
        hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
        RelrDynSection { hdr }
    }
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    let w = E::WORD_SIZE;
    let mut i = 0;
    for &id in &ctx.chunks {
        let hdr = ctx.chunk_header(id);
        for &val in &hdr.relr {
            let v = if val & 1 != 0 {
                val
            } else {
                hdr.shdr.sh_addr.get() + val
            };
            if E::IS_64 {
                E::Endian::write_u64(&mut buf[i * w..], v);
            } else {
                E::Endian::write_u32(&mut buf[i * w..], v as u32);
            }
            i += 1;
        }
    }
}

// .relr.dyn contains base relocations encoded in a space-efficient form.
// The contents of the section is essentially just a list of addresses
// that have to be fixed up at runtime.
//
// Here is the encoding scheme (we assume 64-bit ELF in this description
// for the sake of simplicity): .relr.dyn contains zero or more address
// groups. Each address group consists of a 64-bit start address followed
// by zero or more 63-bit bitmaps. Let A be the address of a start
// address. Then, the loader fixes address A. If Nth bit in the following
// bitmap is on, the loader also fixes address A + N * 8. In this scheme,
// one address and one bitmap can represent up to 64 base relocations in a
// 512 bytes range.
//
// A start address and a bitmap is distinguished by the lowest significant
// bit. An address must be even and thus its LSB is 0 (odd address is not
// representable in this encoding and such relocation must be stored to
// the .rel.dyn section). A bitmap has LSB 1.
pub fn encode_relr<E: Arch>(offsets: &[u64]) -> Vec<u64> {
    let word = E::WORD_SIZE as u64;
    let num_bits = if E::IS_64 { 63 } else { 31 };
    let max_delta = word * num_bits;
    let mut vec = Vec::new();
    let mut i = 0;

    while i < offsets.len() {
        let first = offsets[i];
        vec.push(first);
        let mut base = first + word;
        i += 1;
        loop {
            let mut bits = 0u64;
            while i < offsets.len() && offsets[i] - base < max_delta {
                bits |= 1 << ((offsets[i] - base) / word);
                i += 1;
            }
            if bits == 0 {
                break;
            }
            vec.push((bits << 1) | 1);
            base += max_delta;
        }
    }
    vec
}
