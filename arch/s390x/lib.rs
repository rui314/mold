//! The linker instantiated for s390x. Every target has a crate like this
//! one, so that the compiler can build the targets in parallel.

use std::borrow::Cow;
use std::ffi::OsStr;
use std::sync::Arc;

/// Links for this target, or reports the target the inputs are actually
/// for.
pub fn link(cmdline: Arc<[Cow<'static, OsStr>]>) -> Result<i32, &'static str> {
    mold::driver::link::<mold::arch::S390x>(cmdline)
}
