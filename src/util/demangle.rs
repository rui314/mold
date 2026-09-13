//! Symbol name demangling for diagnostics.

/// Demangles an Itanium C++ ABI symbol name, if `name` is one.
pub fn demangle_cpp(name: &[u8]) -> Option<String> {
    // TODO(cwasser): Actually demangle Symbols on Windows using e.g.
    // `UnDecorateSymbolName` from Dbghelp, maybe even Itanium symbols?
    if !name.starts_with(b"_Z") {
        return None;
    }
    let sym = cpp_demangle::Symbol::new(name).ok()?;
    let options = cpp_demangle::DemangleOptions::default();
    sym.demangle(&options).ok()
}

/// Demangles a Rust symbol name in either the legacy or the v0 scheme.
/// Crate disambiguator hashes are omitted, as rustc's own tools do.
pub fn demangle_rust(name: &[u8]) -> Option<String> {
    let name = std::str::from_utf8(name).ok()?;
    if let Ok(sym) = rustc_demangle::try_demangle(name) {
        return Some(format!("{sym:#}"));
    }

    // A legacy name may be followed by characters that aren't part of the
    // mangling, as in `_ZN2ns7versionEv`, which is `ns::version` and a
    // trailing `v`. Demangle the path and keep the rest as it is.
    let end = legacy_path_end(name)?;
    let sym = rustc_demangle::try_demangle(&name[..end]).ok()?;
    Some(format!("{sym:#}{}", &name[end..]))
}

/// The end of the legacy-mangled path `_ZN<len><ident>...E` at the start
/// of `name`, if there is one.
fn legacy_path_end(name: &str) -> Option<usize> {
    let mut rest = name.strip_prefix("_ZN")?;
    loop {
        if rest.starts_with('E') {
            return Some(name.len() - rest.len() + 1);
        }
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let len: usize = rest[..digits].parse().ok()?;
        if len == 0 {
            return None;
        }
        rest = rest.get(digits.checked_add(len)?..)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_legacy_with_suffix() {
        assert_eq!(
            demangle_rust(b"_ZN2ns7versionEv").as_deref(),
            Some("ns::versionv")
        );
        assert_eq!(demangle_rust(b"_ZN3foo3barE").as_deref(), Some("foo::bar"));
        assert_eq!(demangle_rust(b"_ZN3foo").as_deref(), None);
        assert_eq!(demangle_rust(b"main"), None);
    }

    #[test]
    fn rust_legacy_length_overflow() {
        // Adding the digit count to this length wraps to zero, so an
        // unchecked parser would keep visiting the same identifier.
        let len = usize::MAX - usize::MAX.ilog10() as usize;
        let name = format!("_ZN{len}E");
        assert_eq!(demangle_rust(name.as_bytes()), None);
    }
}
