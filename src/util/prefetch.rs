/// Prefetches a cache line for a future read, preferring the L1 cache.
/// Targets without a supported prefetch instruction ignore the hint.
#[inline]
pub fn prefetch(_ptr: *const u8) {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    // SAFETY: SSE is part of our x86 baseline. PREFETCHT0 is a non-faulting
    // hint and changes no registers or memory.
    unsafe {
        std::arch::asm!(
            "prefetcht0 [{ptr}]",
            ptr = in(reg) _ptr,
            options(readonly, nostack, preserves_flags),
        );
    }

    #[cfg(any(target_arch = "aarch64", target_arch = "arm64ec"))]
    // SAFETY: PRFM is a non-faulting hint and changes no registers or memory.
    unsafe {
        std::arch::asm!(
            "prfm pldl1keep, [{ptr}]",
            ptr = in(reg) _ptr,
            options(readonly, nostack, preserves_flags),
        );
    }

    #[cfg(target_arch = "arm")]
    // SAFETY: PLD is a non-faulting hint and changes no registers or memory.
    unsafe {
        std::arch::asm!(
            "pld [{ptr}]",
            ptr = in(reg) _ptr,
            options(readonly, nostack, preserves_flags),
        );
    }

    #[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
    // SAFETY: PREFETCH.R is an ORI hint, which is a no-op without Zicbop.
    // Spell it as ORI so the assembler does not require that extension.
    unsafe {
        std::arch::asm!(
            "ori zero, {ptr}, 1",
            ptr = in(reg) _ptr,
            options(readonly, nostack, preserves_flags),
        );
    }

    #[cfg(any(target_arch = "powerpc", target_arch = "powerpc64"))]
    // SAFETY: DCBT is a non-faulting hint and changes no registers or memory.
    unsafe {
        std::arch::asm!(
            "dcbt 0, {ptr}",
            ptr = in(reg) _ptr,
            options(readonly, nostack, preserves_flags),
        );
    }

    #[cfg(target_arch = "s390x")]
    // SAFETY: PFD is a non-faulting hint and changes no registers or memory.
    unsafe {
        std::arch::asm!(
            "pfd 1, 0({ptr})",
            ptr = in(reg_addr) _ptr,
            options(readonly, nostack, preserves_flags),
        );
    }

    #[cfg(target_arch = "loongarch64")]
    // SAFETY: PRELD is a non-faulting hint and changes no registers or memory.
    unsafe {
        std::arch::asm!(
            "preld 0, {ptr}, 0",
            ptr = in(reg) _ptr,
            options(readonly, nostack, preserves_flags),
        );
    }
}
