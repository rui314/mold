//! `.relr.dyn`, packed base relocations.

use crate::arch::Target;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::elf::*;

// .relr.dyn is a relatively new section for storing base relocation
// information.
//
// A relocatable executable/DSO contains a lot of relocation entries of a
// certain type, called "base relocations", to specify the locations
// of pointers in the file that need to be adjusted according to the
// desired load address and the actual load address. As an example,
// consider the following C code.
//
// extern int foo;
// int *bar = &foo;
//
// If an executable containing the above code is built as a relocatable
// executable, meaning that the executable can be loaded not at a specific
// address in memory but anywhere in the virtual address space, then the
// pointer `bar`'s address is not known at link-time.
//
// The linker temporarily links the executable to a base address, records
// that information in the program header, and emits dynamic relocations to
// refer to the location of `bar`. At runtime, the loader fixes the pointer
// by adding the difference between the expected load address and the
// actual one to its value.
//
// Relocatable executables/DSOs usually contain a fairly large number of
// base relocations. In particular, a C++ virtual function table is an array
// of statically-initialized pointers which need base relocations.
//
// Notice that base relocations don't contain symbol information. They
// need only pointer locations in the ELF file that need fixing at
// load-time. Therefore, storing that information in the usual ELF
// relocation table is a waste of space.
//
// .relr.dyn is designed to store base relocations in a space-efficient way.
pub fn new_header<E: Target>(args: &crate::cmdline::Args) -> ChunkHeader<E> {
    let ty = if args.use_android_relr_tags { SHT_ANDROID_RELR } else { SHT_RELR };
    let mut hdr = ChunkHeader::<E>::new(".relr.dyn", ty, SHF_ALLOC as u64);
    hdr.shdr.sh_entsize.set(E::WORD_SIZE as u64);
    hdr.shdr.sh_addralign.set(E::WORD_SIZE as u64);
    hdr
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let w = E::WORD_SIZE;
    let mut i = 0;
    for &id in &ctx.chunks {
        let hdr = ctx.chunk_header(id);
        for &val in &hdr.relr {
            let v = if val & 1 != 0 { val } else { hdr.shdr.sh_addr.get() + val };
            Word::<E>::new(v).write(&mut buf[i * w..]);
            i += 1;
        }
    }
}

// .relr.dyn contains base relocations encoded in a space-efficient form.
// The contents of the section are essentially just a list of addresses
// that have to be fixed up at runtime.
//
// Here is the encoding scheme (we assume 64-bit ELF in this description
// for the sake of simplicity): .relr.dyn contains zero or more address
// groups. Each address group consists of a 64-bit start address followed
// by zero or more 63-bit bitmaps. Let A be a start address. Then, the
// loader fixes address A. If the Nth bit in the following bitmap is on,
// the loader also fixes address A + N * 8. In this scheme, one address and
// one bitmap can represent up to 64 base relocations in a 512-byte range.
//
// A start address and a bitmap are distinguished by the least significant
// bit. An address must be even and thus its LSB is 0 (an odd address is not
// representable in this encoding and such a relocation must be stored in
// the .rel.dyn section). A bitmap has LSB 1.
pub fn encode_relr<E: Target>(offsets: &[u64]) -> Vec<u64> {
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
