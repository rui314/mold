//! The linker instantiated for riscv64. Every target has a crate like this
//! one, so that the compiler can build the targets in parallel.

use std::ffi::OsString;
use std::sync::Arc;

/// Links for this target, or reports the target the inputs are actually
/// for.
pub fn link(cmdline: Arc<[OsString]>) -> Result<i32, &'static str> {
    mold::driver::link::<mold::arch::Riscv64>(cmdline)
}
