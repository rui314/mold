pub fn link(cmdline: mold::driver::Cmdline) -> mold::driver::LinkResult {
    mold::driver::link::<mold::arch::Ppc32>(cmdline)
}
