//! The linker instantiated for arm64. Every target has a crate like this
//! one, so that the compiler can build the targets in parallel.

/// Links for this target.
pub fn link(parsed: mold_macho::driver::ParsedArgs) {
    mold_macho::driver::link::<mold_macho::arch::Arm64>(parsed)
}

/// Links for arm64e, the arm64 variant with pointer authentication.
pub fn link_arm64e(parsed: mold_macho::driver::ParsedArgs) {
    mold_macho::driver::link::<mold_macho::arch::Arm64e>(parsed)
}
