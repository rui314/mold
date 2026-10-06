pub fn link(cmdline: libmold::driver::Cmdline) -> libmold::driver::LinkResult {
    libmold::driver::link::<libmold::arch::X86_64>(cmdline)
}
