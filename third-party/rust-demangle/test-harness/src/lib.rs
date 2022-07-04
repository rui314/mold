use std::fmt;

// HACK(eddyb) helper macros for tests.
#[macro_export]
macro_rules! assert_contains {
    ($s:expr, $needle:expr) => {{
        let (s, needle) = ($s, $needle);
        assert!(
            s.contains(needle),
            "{:?} should've contained {:?}",
            s,
            needle
        );
    }};
}
#[macro_export]
macro_rules! assert_ends_with {
    ($s:expr, $suffix:expr) => {{
        let (s, suffix) = ($s, $suffix);
        assert!(
            s.ends_with(suffix),
            "{:?} should've ended in {:?}",
            s,
            suffix
        );
    }};
}

/// `rustc_demangle::Demangle` wrapper that will also attempt demanging with
/// `rust-demangle.c`'s `rust_demangle` when formatted, and assert equality.
///
/// The reason this mimics `rustc_demangle`'s API is to allow its tests to be
/// reused without rewriting them (which could risk introducing bugs).
pub struct Demangle<'a> {
    // NOTE(eddyb) we don't trust `rustc_demangle` to keep the original string
    // unmodified, as if it e.g. strips suffixes early, it could hide the fact
    // that the C port doesn't have that sort of thing supported yet.
    original: &'a str,

    rustc_demangle: rustc_demangle::Demangle<'a>,
}

pub fn demangle(s: &str) -> Demangle {
    Demangle {
        original: s,
        rustc_demangle: rustc_demangle::demangle(s),
    }
}

pub fn try_demangle(s: &str) -> Result<Demangle<'_>, rustc_demangle::TryDemangleError> {
    match rustc_demangle::try_demangle(s) {
        Ok(d) => Ok(Demangle {
            original: s,
            rustc_demangle: d,
        }),
        Err(e) => {
            // HACK(eddyb) ensure the C port also errors.
            assert_eq!(demangle_via_c(s, false), "");

            Err(e)
        }
    }
}

impl fmt::Display for Demangle<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_maybe_verbose(!f.alternate()))
    }
}

impl Demangle<'_> {
    pub fn to_string_maybe_verbose(&self, verbose: bool) -> String {
        let rust = if verbose {
            format!("{}", self.rustc_demangle)
        } else {
            format!("{:#}", self.rustc_demangle)
        };
        let c = demangle_via_c(self.original, verbose);
        if rust != c {
            panic!(
                "Rust vs C demangling difference:\
            \n mangled: {mangled:?}\
            \n    rust: {rust:?}\
            \n       c: {c:?}\
            \n",
                mangled = self.original
            );
        }

        rust
    }
}

fn demangle_via_c(mangled: &str, verbose: bool) -> String {
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    extern "C" {
        fn rust_demangle(mangled: *const c_char, flags: i32) -> *mut c_char;
        fn free(ptr: *mut c_char);
    }

    let flags = if verbose { 1 } else { 0 };
    let mangled = CString::new(mangled).unwrap();
    let out = unsafe { rust_demangle(mangled.as_ptr(), flags) };
    if out.is_null() {
        String::new()
    } else {
        unsafe {
            let s = CStr::from_ptr(out).to_string_lossy().into_owned();
            free(out);
            s
        }
    }
}
