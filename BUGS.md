This is a note about interesting bugs that I met during the
development of the mold linker.

## GNU IFUNC

Problem: A statically-linked "hello world" program mysteriously
crashed in `__libc_start_main` function which is called just after
`_start`.

Investigation: I opened up gdb and found that the program reads a
bogus value from a TLS block. It looks like `memcpy` failed to copy
proper data there.  After some investigation, I noticed that `memcpy`
doesn't copy data at all but instead returns the address of
`__memcpy_avx_unaligned` function, which is a real `memcpy` function
optimized for machines with the AVX registers.

This odd issue was caused by the GNU IFUNC mechanism.  That is, if a
function symbol has type `STT_GNU_IFUNC`, the function does not do
what its name suggests to do but instead returns a pointer to a
function that does the actual job. In this case, `memcpy` is an IFUNC
function, and it returns an address of `__memcpy_avx_unaligned` which
is a real `memcpy` function.

IFUNC function addresses are stored to `.got` section in an ELF
executable.  The dynamic loader executes all IFUNC functions at
startup and replace their GOT entries with their return values. This
mechanism allows programs to choose the best implementation among
variants of the same function at runtime based on the machine info.

If a program is statically-linked, there's no dynamic loader that
rewrites the GOT entries. Therefore, if a program is
statically-linked, a libc's startup routine does that on behalf of the
dynamic loader. Concretely, a startup routine interprets all dynamic
relocations between `__rela_iplt_start` and `__rela_iplt_start`
symbols.  It is linker's responsibility to emit dynamic relocations
for IFUNC symbols even if it is linking a statically-linked program
and mark the beginning and the ending of a `.rela.dyn` section with
the symbols, so that the startup routine can find the relocations.

The bug was my linker didn't define `__rela_iplt_start` and
`__rela_iplt_stop` symbols. Since these symbols are weak, they are
initialized to zero. From the point of the initializer function,
there's no dynamic entries between `__rela_iplt_start` and
`__rela_iplt_start` symbols. That left GOT entries for IFUNC symbols
untouched.

The proper fix was to emit dynamic relocations for IFUNC symbols and
define the linker-synthesized symbols. I did that, and the bug was
fixed.
