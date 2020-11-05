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

## stdio buffering

Problem: A statically-linked "Hello, world" prints out the message if
executed as `./hello`, but it doesn't output anything if executed as
`./hello | cat`.

Investigation: I knew that the default buffering mode for stdout is
line buffering (buffer is flushed on every '\n'), but if it is not
connected to the terminal (i.e. `isatty(2)` returns 0 on
`STDOUT_FILENO`), it automatically switches to full buffering (buffer
is flushed when it becomes full). So, it looks like libc failed to
flush the stdout on program exit for some reason.

I traced all function calls using gdb and noticed that `__libc_atexit`
was not called. That function seemed to be responsible for buffer
flushing. I don't know how exactly I found the root cause, but after
spending an hour or two, I found that `__start___libc_atexit` and
`__stop___libc_atexit` have value 0 in my linker's output while they
mark a section containing the address of `__libc_atexit` in GNU ld's
output.

So, libc doesn't directly call `__libc_atexit` but instead put its
address in `_libc_atexit` section, expecting that the linker
automatically creates the start and the end marker symbols for the
section.

There's an obscure linker feature: if a section name is valid as a C
identifier (e.g. `foo` or `_foo_bar` but not `.foo`), the linker
automatically create two symbols by prepending `__start_` and
`__stop_` to the section name. My linker didn't implement that.

I implemented the feature, and the bug was fixed.

## TLS variable initialization

Problem: A statically-linked "hello world" program crashes after
reading a thread-local variable.

Investigation: Thread-local variables are very different from other
types of varaibles because there may be more than one instance of the
same variable. Each thread has its copy of thread-local
varaibles. `%fs` segment register points the end of the variable area
for the current thread, and the variables are accessed as an offset
from `%fs`.

Thread-local variables may be initialized (e.g. `thread_local int x =
5;`). The linker gathers all thread-local variables and put them into
`PT_TLS` segment. At runtime, the contents of the segment is used as
an "initialization image". When a new thread is created, the image is
memcpy'ed to the new thread's thread-local variable area. The
initialization image itself is read-only at runtime.

It took more than a day to find out the location where the memcpy call
copies the initialization image to and the thread-local variables
reside are different. As a result, thread-local variables have garbage
as initial values, and the program crashes when using the varaibles.

The problem is that I set a very large value (4096) to the alignment
of `PT_TLS` segment. All `PT_LOAD` segments are naturally aligned to
the page boundary, so I use the same value for `PT_TLS`, but that was
a mistake. When a thread initialization routine sets a value to `%fs`,
it first aligns the end of the thread-local variable area address to
`PT_TLS` alignment. So, if you set a large value to `PT_TLS`
alignment, `%fs` is set to a wrong place.

I fixed `PT_TLS` alignment, and the bug was gone.
