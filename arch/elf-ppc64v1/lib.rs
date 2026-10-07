pub fn link(cmdline: mold_elf::driver::Cmdline) -> mold_elf::driver::LinkResult {
    mold_elf::driver::link::<mold_elf::arch::Ppc64V1>(cmdline)
}
