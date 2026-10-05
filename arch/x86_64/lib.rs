pub fn link(cmdline: mold::driver::Cmdline) -> mold::driver::LinkResult {
    mold::driver::link::<mold::arch::X86_64>(cmdline)
}
