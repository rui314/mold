//! Symbol name demangling for diagnostics.

/// Demangles an Itanium C++ ABI symbol name, if `name` is one the
/// demangler reads: its identifiers are ASCII, so a name with other
/// bytes stays as it is.
pub fn demangle_cpp(name: &[u8]) -> Option<String> {
    if !name.starts_with(b"_Z") {
        return None;
    }
    cpp_demangle::Symbol::new(name).ok()?.demangle().ok()
}

/// A Mach-O symbol name as diagnostics spell it (see display_name).
pub struct DisplayName<'a>(&'a [u8]);

/// A Mach-O symbol name as diagnostics spell it: demangled when
/// -demangle is in effect. Mach-O prefixes every C-level name with an
/// underscore, so an Itanium name reads `__Z...` here; symbol lookup and
/// output always use the original spelling. A name that isn't demangled
/// prints as its bytes are (see error::raw), UTF-8 or not.
pub fn display_name(name: &[u8]) -> DisplayName<'_> {
    DisplayName(name)
}

impl std::fmt::Display for DisplayName<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if crate::error::demangle_enabled()
            && let Some(demangled) = self.0.strip_prefix(b"_").and_then(demangle_cpp)
        {
            return f.write_str(&demangled);
        }
        crate::error::raw(self.0).fmt(f)
    }
}
