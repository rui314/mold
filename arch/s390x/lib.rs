pub fn link(cmdline: libmold::driver::Cmdline) -> libmold::driver::LinkResult {
    libmold::driver::link::<libmold::arch::S390x>(cmdline)
}
