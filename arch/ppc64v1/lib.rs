pub fn link(cmdline: libmold::driver::Cmdline) -> libmold::driver::LinkResult {
    libmold::driver::link::<libmold::arch::Ppc64V1>(cmdline)
}
