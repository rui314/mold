//! The linker instantiated for x86_64. Every target has a crate like this
//! one, so that the compiler can build the targets in parallel.

/// Links for this target, or reports the target the inputs are actually
/// for.
pub fn link(cmdline: mold_macho::driver::Cmdline) -> mold_macho::driver::LinkResult {
    mold_macho::driver::link::<mold_macho::target::X86_64>(cmdline)
}
