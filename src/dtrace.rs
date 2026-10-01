//! DTrace USDT probes (statically defined tracing).
//!
//! A header `dtrace -h` makes from a D script has the code call an
//! undefined function for each probe site, `___dtrace_probe$<provider>$
//! <probe>$v1$<argument types>`, and for each is-enabled test,
//! `___dtrace_isenabled$<provider>$<probe>$v1`, and refer to two more
//! undefined symbols per provider that no relocation uses: its
//! stability attributes and its argument typedefs. None of these is
//! ever defined.

/// The prefix of the names of the symbols a `dtrace -h` header makes
/// code refer to. ld-prime takes every undefined symbol whose name
/// starts with it for one of them, whatever follows.
const PREFIX: &str = "___dtrace_";

/// Whether an undefined symbol of this name is a DTrace symbol.
pub fn is_dtrace_symbol(name: &str) -> bool {
    name.starts_with(PREFIX)
}
