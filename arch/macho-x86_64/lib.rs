//! The linker instantiated for x86_64. Every target has a crate like this
//! one, so that the compiler can build the targets in parallel.

/// Links for this target.
pub fn link(parsed: mold_macho::driver::ParsedArgs) {
    mold_macho::driver::link::<mold_macho::arch::X86_64>(parsed)
}
