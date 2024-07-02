This is a note about interesting bugs that I met during the
development of the mold linker.

## GNU IFUNC

Problem: A statically-linked "hello world" program mysteriously
crashed in `__libc_start_main` function which is called just after
`_start`.

Investigation: I opened up gdb and found that the program reads a
bogus value from some array. It looks like `memcpy` failed to copy
proper data there.  After some investigation, I noticed that `memcpy`
did't copy data at all but instead returned the address of
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
startup to replace their GOT entries with their return values. This
mechanism allows programs to choose the best implementation among
variants of the same function at runtime based on the machine info.

If a program is statically-linked, there's no dynamic loader that
rewrites the GOT entries. Therefore, if a program is
statically-linked, a libc's startup routine does that on behalf of the
dynamic loader. Concretely, a startup routine interprets all dynamic
relocations between `__rela_iplt_start` and `__rela_iplt_stop`
symbols.  It is linker's responsibility to emit dynamic relocations
for IFUNC symbols even if it is linking a statically-linked program
and mark the beginning and the ending of a `.rela.dyn` section with
the symbols, so that the startup routine can find the relocations.

The bug was my linker didn't define `__rela_iplt_start` and
`__rela_iplt_stop` symbols. Since these symbols are weak, they are
initialized to zero. From the point of the initializer function,
there's no dynamic relocations between `__rela_iplt_start` and
`__rela_iplt_stop` symbols. That left GOT entries for IFUNC symbols
untouched.

The proper fix was to emit dynamic relocations for IFUNC symbols and
define the linker-synthesized symbols. I did that, and the bug was
fixed.

## stdio buffering

Problem: A statically-linked "Hello world" program prints out the
message if executed as `./hello`, but it doesn't output anything if
executed as `./hello | cat`.

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

So, libc doesn't directly call `__libc_atexit` but instead call all
function pointers between `__start___libc_atexit` and
`__stop___libc_atexit` symbols. libc puts `__libc_atexit` address in
`_libc_atexit` section, expecting that the linker automatically
creates the start and the end marker symbols for the section.

There's an obscure linker feature: if a section name is valid as a C
identifier (e.g. `foo` or `_foo_bar` but not `.foo`), the linker
automatically creates marker symbols by prepending `__start_` and
`__stop_` to the section name. My linker lacked the feature.

I implemented the feature, and the bug was fixed.

## TLS variable initialization

Problem: A statically-linked "hello world" program crashes after
reading a thread-local variable.

Investigation: Thread-local variables are very different from other
types of variables because there may be more than one instance of the
same variable in memory. Each thread has its copy of thread-local
variables. `%fs` segment register points the end of the variable area
for the current thread, and the variables are accessed as an offset
from `%fs`.

Thread-local variables may be initialized (e.g. `thread_local int x =
5;`). The linker gathers all thread-local variables and put them into
`PT_TLS` segment. At runtime, the contents of the segment is used as
an "initialization image" for new threads. When a new thread is
created, the image is memcpy'ed to the new thread's thread-local
variable area. The initialization image itself is read-only at
runtime.

It took more than a day to find out that memcpy copies the
initialization image to a different place than the thread-local
variables reside. That means, thread-local variables had garbage as
initial values, and the program crashed when using them.

The problem is that I set a very large value (4096) to the alignment
of `PT_TLS` segment. All `PT_LOAD` segments are naturally aligned to
the page boundary, so I use the same value for `PT_TLS`, but that was
a mistake. When a thread initialization routine sets a value to `%fs`,
it first aligns the end of the thread-local variable area address to
`PT_TLS` alignment value. So, if you set a large value to `PT_TLS`
alignment, `%fs` is set to a wrong place.

I fixed `PT_TLS` alignment, and the bug was gone.

## stdio buffering (another issue)

I noticed that a dynamically-linked "hello world" program didn't
flush its stdout buffer on exit. The cause of the problem was that the
executable had more than one DT_NEEDED entry for `libc.so`.

DT_NEEDED entries in `.dynamic` section specify a list of shared
object file names which need to be linked at runtime. I added one
DT_NEEDED entry for each library specified with the `-l` option.
The pitfall is, unlike object files, libraries are allowed to
appear more than once in a command line, and the linker has to
de-duplicate them before processing. Adding more than one DT_NEEDED
entry for the same shared object causes mysterious issues like this.

# Copy relocations and symbol aliases

environ, _environ and __environ point to the same location in libc.so,
so when we create a copy relocation for one of the symbols, we need to
do that for all of them. Otherwise, they'll end up pointing to different
places which causes a very mysterious issue.

# DT_DEBUG and gdb

If you forget to add an entry with type DT_DEBUG to .dynamic, gdb's
`info sharedlibrary` command doesn't print out a list of shared
libraries loaded to memory. The value of the entry doesn't matter, so
it can be just zero. The existence of it is important.

#

__EH_FRAME_BEGIN__ in libgcc/crtstuff.c
