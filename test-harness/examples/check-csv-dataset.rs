use std::env;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::path::PathBuf;

use rust_demangle_c_test_harness::demangle;

// HACK(eddyb) this is only an `example` so that `cargo run` doesn't do anything.
// FIXME(eddyb) document this better and provide datasets for it.
fn main() {
    let header = "legacy+generics,legacy,mw,mw+compression,v0,v0+compression";

    for path in env::args_os().skip(1).map(PathBuf::from) {
        let mut lines = BufReader::new(File::open(path).unwrap())
            .lines()
            .map(|l| l.unwrap());

        assert_eq!(lines.next().unwrap(), header);

        for line in lines {
            for mangling in line.split(',').skip(4) {
                for verbose in [false, true] {
                    demangle(mangling).to_string_maybe_verbose(verbose);
                }
            }
        }
    }
}
