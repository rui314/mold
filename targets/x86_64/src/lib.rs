//! The linker instantiated for x86_64. Every target has a crate like this
//! one, so that the compiler can build the targets in parallel.

/// Links for this target, or reports the target the inputs are actually
/// for.
pub fn link(cmdline: &[std::ffi::OsString]) -> Result<i32, &'static str> {
    mold::driver::link::<mold::arch::X86_64>(cmdline)
}
