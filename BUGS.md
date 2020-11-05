This is a note about interesting bugs that I met during the
development of the mold linker.

## GNU IFUNC

A statically-linked "hello world" program mysteriously crashed in
`__libc_start_main` function which is called just after `_start`.

I opened up gdb and found that the program reads a bogus value from
the TLS block. It looks like `memcpy` failed to copy proper data.
After some investigation, I noticed that `memcpy` doesn't copy data at
all but instead returns the address of `__memcpy_avx_unaligned`
function, which is a real `memcpy` function optimized for machines
with AVX registers.

It turned out the odd issue was caused by the GNU IFUNC mechanism.
That is, if a function symbol has the type `STT_GNU_IFUNC`, the
function does not do what its name suggests to do but instead returns
a pointer to a function that does the actual job. In this case,
`memcpy` is an IFUNC function, and it returns an address of
`__memcpy_avx_unaligned`.

IFUNC function addresses are stored to `.got` section. The dynamic
loader executes all IFUNC functions at startup and replace their GOT
entries with their return values. This mechanism allows programs to
choose the best implementation among variants of the same function at
runtime based on the machine info.

If a program is statically-linked, there's no dynamic loader that
rewrites its GOT entries. Therefore, if a program is
statically-linked, a libc's startup routine does that on behalf of the
dynamic loader. Concretely, the startup routine interprets all dynamic
relocations between `__rela_iplt_start` and `__rela_iplt_start` symbols.
It is linker's responsibility to mark the beginning and the ending of
a `.rela.dyn` section with the symbols, so that the startup routine
can find it.

The bug was my linker didn't define these symbols. Since these symbols
are weak, they are initialized to zero, and from the point of the
initializer function, there's no dynamic entries between
`__rela_iplt_start` and `__rela_iplt_start` symbols. That left GOT
entries for IFUNC symbols. If you call one of the functions, it don't
do what it should do but instead returns a pointer that its job.

The proper fix was to define the linker-synthesized symbols. I did
that, and the bug was fixed.
