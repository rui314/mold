//! This file contains helper functions for thread-local storage (TLS).
//! This comment explains how TLS works on Mach-O, assuming that you
//! already know ELF's TLS.
//!
//! On ELF, there are several TLS access models, and the linker may
//! rewrite instructions to use a cheaper one. On Mach-O, there's only
//! one way to access a thread-local variable (TLV). In ELF terms, it
//! uses TLSDESC for all TLVs, and the function it calls always does what
//! __tls_get_addr() does for General Dynamic.
//!
//! For each TLV, the compiler creates a three-word data structure called
//! a "TLV descriptor" in the __thread_vars section. For example, for the
//! following code
//!
//!   thread_local int foo = 3;
//!
//! the compiler emits something like this:
//!
//!   .section __DATA,__thread_data
//!   _foo$tlv$init:
//!     .long 3
//!
//!   .section __DATA,__thread_vars
//!   _foo:
//!     .quad __tlv_bootstrap   // function pointer
//!     .quad 0                 // key
//!     .quad _foo$tlv$init     // offset
//!
//! Note that the symbol `_foo` refers to the descriptor, not to the
//! variable itself. The variable's initial value is in __thread_data
//! under a different name, or in __thread_bss if it's zero. The
//! descriptor is in __thread_vars in either case.
//!
//! To access a TLV, the code obtains its descriptor's address and calls
//! the function pointer in the descriptor with the descriptor as an
//! argument. The function returns the TLV's address for the current
//! thread. On ARM64, the code looks like this:
//!
//!   adrp x0, _foo@TLVPPAGE
//!   ldr  x0, [x0, _foo@TLVPPAGEOFF]
//!   ldr  x8, [x0]
//!   blr  x8
//!
//! Mach-O doesn't have a PT_TLS-like segment. A Mach-O file's TLS
//! template image consists of a section of type S_THREAD_LOCAL_REGULAR
//! (__thread_data) and a section of type S_THREAD_LOCAL_ZEROFILL
//! (__thread_bss), which are next to each other in memory. dyld treats
//! the range from the beginning of the first section of these types to
//! the end of the last one as the template.
//!
//! At load time, dyld rewrites the contents of the TLV descriptors in
//! __thread_vars. The function pointer is set to dyld's function, and
//! the key is set to a number that dyld assigns to the Mach-O file,
//! which is like ELF's module ID. dyld does this only if the
//! MH_HAS_TLV_DESCRIPTORS flag is set in the Mach-O header.
//!
//! On ELF, the thread pointer (TP) points to the initial TLS block, so a
//! TLV in it can be accessed by adding a constant to TP. On macOS, TP
//! (TPIDRRO_EL0 on ARM64 and %gs on x86-64) doesn't point to TLVs. It
//! points to a per-thread array that has a pointer for each Mach-O file,
//! and the key is an index into the array. dyld's function works like
//! this:
//!
//!   void *get_addr(TLVDescriptor *desc) {
//!     char **array = thread_pointer;
//!     if (!array[desc->key]) {
//!       MachOFile *file = files[desc->key];
//!       array[desc->key] = malloc(file->template_size);
//!       memcpy(array[desc->key], file->template, file->template_size);
//!     }
//!     return array[desc->key] + desc->offset;
//!   }
//!
//! That means, unlike ELF, every TLS block is allocated when it is first
//! accessed, even the main executable's. TLVs are not at a fixed
//! distance from TP, so Mach-O has nothing like Initial Exec or Local
//! Exec.
//!
//! Here is what the linker has to do for TLVs:
//!
//!  - The function pointer in a descriptor refers to __tlv_bootstrap,
//!    which is defined in libSystem. We handle it as an ordinary pointer
//!    to an imported symbol.
//!
//!  - The offset in a descriptor refers to the variable's initial value
//!    with an ordinary 64-bit absolute relocation, but we write the
//!    offset from the beginning of the template instead of the address.
//!    Since it's not an address, it doesn't need a rebase. There's no
//!    relocation type for it like ELF's DTPOFF, so we recognize it by
//!    the type of the section it refers to.
//!
//!  - The adrp and ldr instructions in the above code load the
//!    descriptor's address from a GOT entry. If the descriptor's address
//!    is known at link-time, we relax the load to an address computation
//!    as we do for a GOT load. Otherwise, for example if the TLV is
//!    defined in another dylib, dyld sets the descriptor's address to
//!    the GOT entry.
//!
//!  - If the output has __thread_vars, we set MH_HAS_TLV_DESCRIPTORS in
//!    the Mach-O header.
//!
//! We don't rewrite any other instructions for TLVs.

use crate::arch::Target;
use crate::context::Context;

/// Returns the address of the TLS template image. A TLV descriptor's
/// offset is relative to this address.
pub fn tls_begin<E: Target>(ctx: &Context<E>) -> u64 {
    ctx.chunks
        .iter()
        .map(|&id| ctx.chunk_header(id))
        .filter(|hdr| hdr.is_thread_local())
        .map(|hdr| hdr.addr)
        .min()
        .unwrap_or(0)
}
