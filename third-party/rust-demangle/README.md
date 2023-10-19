# `rust-demangle.c`

This is a single-file C99 port of the official Rust symbol demangler ([`rustc-demangle`](https://github.com/rust-lang/rustc-demangle), a Rust library).

## Usecases

This C port is intended for situations in which a Rust dependency is hard to
justify, or effectively impossible (e.g. platform toolchains, that would be used
*while building Rust*, not the other way around).

If a Rust dependency is acceptable, [using `rustc-demangle` from C](https://github.com/rust-lang/rustc-demangle#usage-from-non-rust-languages)
(or other languages via FFI) is possible, and may be preferred over this C port.

## Status

As this C port was originally (see the [History](#history) section) *only* for the
`rustc-demangle` code demangling the (new at the time) [Rust RFC2603 (aka "`v0`") mangling scheme](https://rust-lang.github.io/rfcs/2603-rust-symbol-name-mangling-v0.html),
it may lag behind `rustc-demangle` in functionality, for now.

The current port status by category is:
* **ported** `legacy` (pre-RFC2603 Rust symbols) demangling
* `v0` ([RFC2603](https://rust-lang.github.io/rfcs/2603-rust-symbol-name-mangling-v0.html) Rust symbols) demangling
  * **ported** PRs:
    * [[#23] Support demangling the new Rust mangling scheme (v0).](https://github.com/rust-lang/rustc-demangle/pull/23)
    * [[#26] v0: allow identifiers to start with a digit.](https://github.com/rust-lang/rustc-demangle/pull/26)
    * [[#53] v0: replace `skip_*` methods with `print_*` methods in a "skip printing" mode.](https://github.com/rust-lang/rustc-demangle/pull/53)
      * arguably backported to Rust, as the C port always took this approach
    * symbol prefix flexibility (`__R` and `R`, instead of `_R`)
    * [[#39] Add support for `min_const_generics` constants](https://github.com/rust-lang/rustc-demangle/pull/39)
      * [[#40] Elide the type when the const value is a placeholder `p`](https://github.com/rust-lang/rustc-demangle/pull/40)
    * [[#55] v0: demangle structural constants and &str.](https://github.com/rust-lang/rustc-demangle/pull/55)
      (only usable in `const` generics on unstable Rust)
  * **(UNPORTED)** recursion limits
* miscellaneous
  * **ported** PRs:
    * [[#30] v0: also support preserving extra suffixes found after mangled symbol.](https://github.com/rust-lang/rustc-demangle/pull/30)
  * **(UNPORTED)** output size limits

Notable differences (intentionally) introduced by porting:
* `rustc-demangle` can't use the heap (as it's `#![no_std]`), but the C port does
  * this is mainly dictated by the ergonomics of the `rust_demangle` API, which
    requires `malloc`/`realloc` to return a new C string allocation
  * if there is demand for it, `rust_demangle` support could be made optional,
    forcing heap-less users to always use `rust_demangle_with_callback` instead
  * a subtler consequence is that `rustc-demangle` uses a fixed-size buffer on
    the stack for punycode decoding, while the C port can allocate it on the heap
* Unicode support is always handrolled in the C port, and often simplified

## Usage

Get `rust-demangle.c` and `rust-demangle.h` (via `git submodule`, vendoring, etc.),
add them to your project's build system (as C source, and include path, respectively),
then you can call `rust_demangle` with a symbol and some flags, e.g.:
```c
#include <rust-demangle.h>
#include <stdio.h>

int main() {
    const char *sym = "_RNvNtCsbmNqQUJIY6D_4core3foo3bar";

    printf("demangle(%s) = %s\n", sym, rust_demangle(sym, 0));

    printf(
        "demangle(%s, VERBOSE) = %s\n", sym,
        rust_demangle(sym, RUST_DEMANGLE_FLAG_VERBOSE)
    );
}
```
which prints out, when ran:
```
demangle(_RNvNtCsbmNqQUJIY6D_4core3foo3bar) = core::foo::bar
demangle(_RNvNtCsbmNqQUJIY6D_4core3foo3bar, VERBOSE) = core[846817f741e54dfd]::foo::bar
```

Note that the example leaks the returned C strings, ideally you would `free` them.

### Advanced usage (callback-based API)

If you want to avoid the cost of allocating the output in memory, you can also
use `rust_demangle_with_callback`, which takes a "printing" callback instead, e.g.:
```c
#include <rust-demangle.h>
#include <stdio.h>

static void fwrite_callback(const char *data, size_t len, void *opaque) {
    fwrite(data, len, 1, (FILE *)opaque);
}

int main() {
    const char *sym = "_RNvNtCsbmNqQUJIY6D_4core3foo3bar";

    printf("demangle(%s) = ", sym);
    rust_demangle_with_callback(sym, 0, fwrite_callback, stdout);
    printf("\n");

    printf("demangle(%s, VERBOSE) = ", sym);
    rust_demangle_with_callback(
        sym, RUST_DEMANGLE_FLAG_VERBOSE, fwrite_callback, stdout
    );
    printf("\n");
}
```
(with identical output to the simpler example)

## Testing

`cargo test` will run built-in tests - it's implemented in Rust (in `test-harness`)
so that it can depend on `rustc-demangle` itself for comparisons.

Additionally, `cargo run -q --release --example check-csv-dataset path/to/syms/*.csv`
can be used to provide CSV files with additional mangled symbols test data, but such
datasets aren't trivial to obtain (existing ones required building `rust-lang/rust`
with a compiler patch that reacts to a custom environment variable).
They're also quite large (~1GiB uncompressed) so none have been published anywhere yet.

## History

This C port was started while the [Rust RFC2603 (aka "`v0`") mangling scheme](https://rust-lang.github.io/rfcs/2603-rust-symbol-name-mangling-v0.html)
was still being developed, with the intent of upstreaming it into `libiberty`
(which provides demangling for `binutils`, `gdb`, Linux `perf`, etc.) and other
projects (e.g. `valgrind`) - you can see some of that upstreaming history
[on the `v0` tracking issue](https://github.com/rust-lang/rust/issues/60705).

At the time, the expectation was that most projects could either depend on
`libiberty`, or vendor a copy of its code, so the C port focused on upstreaming
to it, rather than producing an independent reusable C codebase.

That meant that instead of a `git` repository, the [code revisions were only tracked by a gist](https://gist.github.com/eddyb/c41a69378750a433767cf53fe2316768/revisions),
and the GNU code style was followed (including C89 comments and variable declarations).

However, the LGPL license of `libiberty` turned out to be a problem for adoption,
compared to the typical MIT/Apache-2.0 dual licensing of Rust projects.

### The `rust-demangle.c` fork

This repository started out as a fork of [the original gist](https://gist.github.com/eddyb/c41a69378750a433767cf53fe2316768/revisions), at commit [`e2c30407516a87c0f8c3820cf152640bd08805dd`](https://github.com/LykenSol/rust-demangle.c/commit/e2c30407516a87c0f8c3820cf152640bd08805dd), *just before `libiberty`
integration* (which was in commit [`0e6f57b0e86ccec4395f8850f4885b1e391a9f4b`](https://gist.github.com/eddyb/c41a69378750a433767cf53fe2316768/0e6f57b0e86ccec4395f8850f4885b1e391a9f4b)).

Any changes since that gist are either fresh C ports of the Rust `rustc-demangle`
code, or completely new code, in order to maintain the [MIT/Apache-2.0 dual licensing](#license).

While this has the disadvantage of starting behind `libiberty` (which kept its
Rust `legacy` demangler, and also got a few more features during/since upstreaming),
the relationship may reverse eventually, where this port could get new features
that would then have to be upstreamed into `libiberty`.

## License

[Like `rustc-demangle`](https://github.com/rust-lang/rustc-demangle#license), this project is licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or
   http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or
   http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in `rust-demangle.c` you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
