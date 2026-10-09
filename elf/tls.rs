//! This file contains helper functions for thread-local storage (TLS).
//! TLS is probably the most obscure feature the linker has to support,
//! so I'll explain it in detail in this comment.
//!
//! TLS is per-thread storage. Thread-local variables (TLVs) are in TLS so
//! that each thread has its own set of thread-local variables. Taking the
//! address of a TLV returns a unique value for each thread. For example,
//! `&foo` for the following code returns different pointer values for
//! different threads.
//!
//!   thread_local int foo;
//!
//! TLV is a relatively new feature. C, for example, didn't officially
//! support it until C11 added the keyword `_Thread_local`. TLVs need
//! coordination between the compiler, the linker and the runtime to work
//! correctly.
//!
//! An ELF executable or a shared library using TLVs contains a "TLS template
//! image" in the PT_TLS segment. For each newly created thread, including the
//! initial one, the runtime allocates contiguous memory for an executable and
//! the shared libraries it depends on, and copies the template images there.
//! That per-thread memory is called the "initial TLS block". After allocating
//! and initializing the initial TLS block, the runtime sets a register to
//! refer to the initial TLS block, so that the thread-local variables are
//! accessible relative to the register.
//!
//! The register referring to the per-thread storage is called the Thread
//! Pointer (TP). TP is part of the thread's context. When the kernel
//! scheduler switches threads, TP is saved and restored automatically just
//! like other registers are.
//!
//! The TLS template image is read-only. It contains TLVs' initial values
//! for new threads, and no one writes to it at runtime.
//!
//! Now, let's think about how to access a TLV. We need to know the TLV's
//! address to access it. There are several different ways to obtain the
//! address as follows:
//!
//!  1. If we are creating an executable, we know the exact size of the TLS
//!     template image we are creating, and we know where the TP will be set
//!     to after the template is copied to the initial TLS block. Therefore,
//!     the TP-relative address of a TLV in the main executable is known at
//!     link-time. That means computing a TLV's address can be as easy as
//!     `add %dst, %tp, <link-time constant>`.
//!
//!  2. If we are creating a shared library, we don't know exactly where
//!     its TLS template image will be copied to in terms of the
//!     TP-relative address, because we don't know how large the main
//!     executable's and other libraries' TLS template images are. Only the
//!     runtime knows the exact TP-relative address.
//!
//!     We can solve the problem with an indirection. Specifically, for
//!     each TLV whose TP-relative address is known only at process startup
//!     time, we create a GOT entry to store its TP-relative address. We
//!     then emit a dynamic relocation to let the runtime fill the GOT
//!     entry with a TP-relative address.
//!
//!     Computing a TLV address in this scheme needs at least two machine
//!     instructions in most ISAs; the first instruction loads a value from
//!     the GOT entry, and the second one adds the loaded value to TP.
//!
//!  3. Now, think about libraries that are dynamically loaded with dlopen.
//!     The TLS block for such a library may not be allocated next to the
//!     initial TLS block, so we can have two or more discontiguous TLS
//!     blocks. There's no easy formula to compute the address of a TLV in a
//!     separate TLS block.
//!
//!     The address of a TLV in a separate TLS block can be obtained by
//!     calling the libc-provided function, __tls_get_addr(). The function
//!     takes a pointer to two consecutive words, a module ID to identify
//!     the ELF file and the TLV's offset within the ELF file's TLS template
//!     image. Accessing a TLV is sometimes compiled to a function call! The
//!     two words are usually stored in the GOT.
//!
//! 1) is called the Local Exec access model. 2) is Initial Exec, and 3) is
//!    General Dynamic.
//!
//! There's another little trick that the compiler can use if it knows two
//! TLVs are in the same ELF file (usually in the same file as the code is).
//! In this case, we can call __tls_get_addr() only once with the module ID
//! and the offset 0 to obtain the base address of the ELF file's TLS block.
//! The base address obtained this way is sometimes called the Dynamic Thread
//! Pointer, or DTP. We can then compute TLVs' addresses by adding their
//! DTP-relative addresses to DTP. This access model is called Local Dynamic.
//!
//! The compiler tries to emit the most efficient code for a TLV access based
//! on compiler command line options and TLV properties as follows:
//!
//!  1. If -fno-PIC or -fPIE is given, and a TLV is defined as a file-local
//!     variable, the compiler knows that it is compiling code for an
//!     executable (i.e. not for a shared library) and the TLV is defined
//!     within the executable. In this case, Local Exec is used to access the
//!     variable.
//!
//!  2. If -fno-PIC or -fPIE is given (i.e. the compiler is compiling code for
//!     a main executable), but a TLV is defined in another translation unit,
//!     then the variable may be defined not in the main executable but in a
//!     shared library. In this case, Initial Exec is used to access the
//!     variable.
//!
//!  3. If -fPIC is given, it may be compiling code for a dlopen'able shared
//!     library. In this case, Local Dynamic or General Dynamic is used to
//!     access TLVs.
//!
//! You can also manually control how the compiler emits TLV access code
//! globally with `-ftls-model=<model-name>` or on a per-variable basis with
//! `__attribute__((tls_model(<model-name>)))`. For example, if you are
//! building a shared library that you have no plan to use with dlopen(), you
//! may want to compile the source files with `-ftls-model=initial-exec` to
//! avoid the cost associated with the General Dynamic access model.
//!
//! The linker may rewrite instructions with a code sequence for a cheaper
//! access model at link-time.
//!
//! === TLS Descriptor access model ===
//!
//! As described above, there are arguably too many different TLS access
//! models from the most generic one you can use in any ELF file to the most
//! efficient one you can use only when building a main executable. Compiling
//! source code with an appropriate TLS access model is bothersome. To solve
//! the problem, a new TLS access model was proposed. That is called the TLS
//! Descriptor (TLSDESC) model.
//!
//! For a TLV compiled with TLSDESC, we allocate two consecutive GOT slots
//! and create a TLSDESC dynamic relocation for them. The dynamic linker
//! stores a function pointer in the first GOT slot and its argument in the
//! second slot.
//!
//! To access the TLV, we call the function pointer with the argument we
//! read from the second GOT slot. The function returns the TLV's
//! TP-relative address.
//!
//! The runtime chooses the best access method depending on the situation
//! and stores a pointer to the most efficient code in the first GOT slot.
//! For example, if a TLV's TP-relative address is known at process startup
//! time, the runtime stores that address in the second GOT slot and a
//! pointer to a function that just returns its argument in the first GOT
//! slot.
//!
//! With TLSDESC, the compiler can always emit the same code for TLVs
//! without sacrificing runtime performance.
//!
//! TLSDESC is better than the traditional, non-TLSDESC TLS access models.
//! It's the default on ARM64, but on other targets, TLSDESC is
//! unfortunately either optional or not supported at all. So we still
//! need to support both the traditional TLS models and the TLSDESC model.
//!
//! Each thread has its own copy of the thread-local variables, initialized
//! from the PT_TLS segment's template image. A register, the thread
//! pointer (TP), refers to the thread's copy, and where exactly TP points
//! relative to the copy is decided by each psABI. The dynamic thread
//! pointer (DTP) is the base `__tls_get_addr` returns for offset 0.

use mold_common::util::{align_down, align_to};

use crate::arch::{Family, Target};
use crate::elf::{ElfPhdr, PT_TLS, PhdrRecord};

/// Returns the TP address which can be used for efficient TLV accesses in
/// the main executable. TP at runtime refers to a per-thread TLS block
/// whose address is not known at link-time. So this function returns the
/// address TP would refer to if the TLS template image were a TLS block.
pub fn tp_addr<E: Target>(phdr: &ElfPhdr<E>) -> u64 {
    debug_assert_eq!(phdr.p_type(), PT_TLS);
    match E::FAMILY {
        // On x86, SPARC and s390x, TP (%gs on i386, %fs on x86-64, %g7 on SPARC
        // and %a0/%a1 on s390x) points past the end of the TLS block for
        // historical reasons. TLVs are accessed with negative offsets from TP.
        Family::X86_64 | Family::I386 | Family::Sparc64 | Family::S390x => {
            align_to(phdr.p_vaddr() + phdr.p_memsz(), phdr.p_align())
        }
        // On ARM and SH4, the runtime inserts two words before the TLS
        // template image when copying TLVs to the TLS block, so we need to
        // offset it.
        Family::Arm64 | Family::Arm32 | Family::Sh4 => {
            align_down(phdr.p_vaddr().wrapping_sub(E::WORD_SIZE as u64 * 2), phdr.p_align())
        }
        // On PowerPC and m68k, TP is 0x7000 (28 KiB) past the beginning
        // of the TLS block to maximize the addressable range of load/store
        // instructions with 16-bit signed immediates. It's not exactly 0x8000
        // (32 KiB) off because there's a small implementation-defined piece of
        // data before the initial TLS block, and the runtime wants to access
        // it efficiently too.
        Family::Ppc32 | Family::Ppc64V1 | Family::Ppc64V2 | Family::M68k => phdr.p_vaddr() + 0x7000,
        // RISC-V and LoongArch just use the beginning of the main executable's
        // TLS block as TP. Their load/store instructions usually take 12-bit
        // signed immediates, so the beginning of the TLS block ± 2 KiB is
        // accessible with a single load/store instruction.
        Family::RiscV | Family::LoongArch => phdr.p_vaddr(),
    }
}

/// Returns the address __tls_get_addr() would return if it's called
/// with offset 0.
pub fn dtp_addr<E: Target>(phdr: &ElfPhdr<E>) -> u64 {
    debug_assert_eq!(phdr.p_type(), PT_TLS);
    match E::FAMILY {
        // On PowerPC and m68k, R_DTPOFF is resolved to the address 0x8000
        // (32 KiB) past the start of the TLS block. The bias maximizes the
        // accessible range of load/store instructions with 16-bit signed
        // immediates. That is, if the offset were right at the start of the
        // TLS block, half of the addressable space (negative immediates) would
        // be wasted.
        Family::Ppc32 | Family::Ppc64V1 | Family::Ppc64V2 | Family::M68k => phdr.p_vaddr() + 0x8000,
        // On RISC-V, the bias is 0x800 as the load/store instructions in the
        // ISA usually have a 12-bit immediate.
        Family::RiscV => phdr.p_vaddr() + 0x800,
        // On other targets, DTP simply refers to the beginning of the TLS block.
        _ => phdr.p_vaddr(),
    }
}
