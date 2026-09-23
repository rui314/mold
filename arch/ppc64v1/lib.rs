//! The linker instantiated for ppc64v1. Every target has a crate like this
//! one, so that the compiler can build the targets in parallel.

/// Links for this target, or reports the target the inputs are actually
/// for.
pub fn link(cmdline: mold::driver::Cmdline) -> mold::driver::LinkResult {
    mold::driver::link::<mold::target::Ppc64V1>(cmdline)
}
