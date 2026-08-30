//! The linker instantiated for arm64. Every target has a crate like this
//! one, so that the compiler can build the targets in parallel.

use mold::diagnostics::Diagnostics;

/// Links for this target, or reports the target the inputs are actually
/// for.
pub fn link(cmdline: &[String], diag: &Diagnostics) -> Result<i32, String> {
    mold::driver::link::<mold::arch::Arm64>(cmdline, diag)
}
