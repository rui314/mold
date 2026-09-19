//! The Mach-O linker instantiated for arm64. Every target has a crate like
//! this one, so that the compiler can build the targets in parallel.

/// Links for this target, or reports the target the inputs are actually
/// for.
pub fn link(cmdline: &[String]) -> Result<i32, String> {
    mold::macho::driver::link::<mold::macho::arch::Arm64>(cmdline)
}
