The very concept of linking is simple: a compiler compiles a piece of
source code into an object file (a file containing machine code), and
a linker combines object files into a single executable or a shared
library file. However, the actual implementation of the linker for
modern systems is much more complicated because hardware, operating
system, compiler and linker all have many more features.

In this file, I'll explain random topics in the glossary format that
you need to understand to read mold code.

## DSO

A .so file. Short for Dynamic Shared Object. Often called as a
shared library, a dynamic libray or a shared object as well.

A DSO contains common functions and data that are used by multiple
executables and/or other DSOs. At runtime, a DSO is loaded to a
contiguous region in the virtual address.

## Object file

A .o file. An object file contains machine code and data, but it
cannot be executed because it's not self-contained. For example,
if you compile a C source file containing a call of `printf`,
the actual function code of `printf` is not included in the resulting
object file. You include `stdio.h`, but that teaches the compiler
only about `printf`'s type, and the compiler still doesn't know what
`printf` actually does. Therefore, it cannot emit code for `printf`.

You need to link an object file with other object file or a shared
library to make it executable.

## Virtual address space

A pointer has a value like 0x803020 which is an address of the
pointee. But it doesn't mean that the pointee resides at the
physical memory address 0x803020 on the computer. Modern CPUs
contains so-called Memory Management Unit (MMU), and all access to
the memory are first translated by MMU to the physical address.
The address before translation is called the "virtual address".
Unless you are doing the kernel programming, all addresses you
handle are virtual addresses.

The OS kernel controls the MMU so that each process owns the entire
virtual address space. So, even if two processes use the same virtual
address, they don't conflict. They are mapped to different physical
addresses.

The existence of MMU has several implications to the linker. First,
we can link the main executable to a specific address. On process
startup, there's no code or data in the virtual address space, so
the mapping of the main executable always succeed. However, it's not
true to DSOs because they are loaded after the main executable and
possibly other DSOs. Therefore, shared libraries must be linked in a
way that they can be loaded to any address in the virtual address
space.

## Relocation

A piece of information for the linker as to how to link object files
or a dynamic objects.

Object files can refer functions or data in other object files. For
example, if you compile a function which calls a non-local function
`foo`, the resulting code contains something like this:

```
  26:   e8 00 00 00 00          callq  2b <bar+0xb>
                        27: R_X86_64_PLT32      foo-0x4
```

The above `callq` is the instruction to call a function at the
machine code level. Its opcode is `0xe8` in x86-64, so the
instruction begins with `0xe8`. The following four bytes are
displacement; that is, the address of the branch target relative to
the end of this `callq` instruction. Notice that the displacement is
0. The compiler couldn't fill the displacement because it has no
idea as to where `foo` will be at runtime. So, the compiler writes 0
as a placeholder and instead writes a relocation `R_X86_64_PLT32`
with `foo` as its associated symbol. The linker reads this
relocation, computes the offsets between this call instruction and
function `foo` and overwrites the placeholder value 0 with an actual
displacement.

There are many different types of relocations. For example, if you
want to fix up not with a displacement but with an absolute address
of a symbol, you need to use `R_X86_64_ABS64` instead.

## Static library

A .a file. Often called as an archive file or just archive as well.

A static library is a container just like tar or zip. Actually,
there's no technical reason to not use tar or (uncompressed) zip,
but traditionally the .a file format is used by the linker.

A static library contains object files and can be passed to the
linker along with other object files and/or archives.

A linker pulls out object files from an archive only if it is needed
to resolve undefined symbols. In other words, object files in an
archive are not linked by default and used as a complement to supply
missing definitions. This is ideal for a library because you don't
want to link library functions unless you are actually using them.

Contrary to archive files, object files directly given to a linker
are always linked to the output.

To maximize the benefit of archive files, a library often used as a
static library is broken down to small files to separate each
function individually (for example, look at
https://git.musl-libc.org/cgit/musl/tree/src/stdio). By doing this,
you import only used functions.

A static file is created by `ar`, whose command line arguments are
similar to `tar`. A static library contains the symbol table which
offers a quick way to look up an object file for a defined symbol,
but mold does not use the static library's symbol table. mold
doesdn't need a symbol table to exist in an archive, and if exists,
mold just ignores it.

See also: DSO (dynamic library)

## Symbol

A symbol is a label assigned to a specific location in an input file
or an output file. For example, if you define function `foo` and
compile it, the resulting object file contains a symbol `foo`
pointing to the beginning of the machine code for `foo`.

Usually, a symbol name is a function or a variable name. If an
object is anonymous (such the one for a string literal), a compiler
generated a unique symbol, which often starts with `.` to avoid
conflict with user-defined symbols.

For C++, symbol name is a complex "mangled" name. We need to mangle
identifiers because a simple name such as `foo` cannot be uniquely
identify a function or a data in C++, because for example `foo` may
be in a namespace or defined as a static member in some class. If
`foo` is an overloaded function, we need to distinguish different
`foo`s by its type. Therefore, C++ compiler mangles an identifier by
appending namespace names, type information and such so that
different things get different names.

For example, a function `int foo(int)` in a namespace `bar` is
mangled as `_ZN3bar3fooEi`.

A symbol can be either defined or undefined. A defined symbol points
to some location in a file which may contain the function's machine
code or the variable's initial value. An undefined symbol does not
point to anywhere. It needs to be merged with a defined symbol with
the same name at link-time. This merging process is called "name
resolution".

For example, if your program is using `printf`, it usually contains
`printf` as an undefined symbol. You need to link it with `libc.a`
or `libc.so`, which contain a defined symbol of `printf`, to make a
complete program.
