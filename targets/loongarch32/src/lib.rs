//! The linker instantiated for loongarch32. Every target has a crate like this
//! one, so that the compiler can build the targets in parallel.

use mold::error::Diagnostics;

/// Links for this target, or reports the target the inputs are actually
/// for.
pub fn link(cmdline: &[String], diag: &Diagnostics) -> Result<i32, String> {
    mold::driver::link::<mold::arch::LoongArch32>(cmdline, diag)
}
