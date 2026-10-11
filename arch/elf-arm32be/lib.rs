pub fn link(cmdline: mold_elf::driver::Cmdline) -> Option<&'static str> {
    mold_elf::driver::link::<mold_elf::arch::Arm32Be>(cmdline)
}
