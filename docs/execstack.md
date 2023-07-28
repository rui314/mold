This page explains the following warning message and how to fix it.
mold emits this message when it sees an object file that may not be
compatible with mold.


```
mold: warning: foo.o: this file may cause a segmentation fault because it requires an executable stack. See https://github.com/rui314/mold/tree/main/docs/execstack.md for more info.
```

# Background

On modern computers, the stack area (to which local variables are
stored) cannot contain executable code. If the control reaches the
stack area, the CPU refuses to execute any code there and the program
is usually terminated due to segmentation fault.

This is a security measure. The stack area used to be executable (old
CPUs generally execute any code as long as it is in a readable memory
region), but that provided an easy attack vector to a malicious user.
They wrote executable code to the stack area using some buffer
overflow bug and jumped there to run arbitrary code in a remote server
process.

To prevent this type of attack, the stack area is no longer executable
since the early 2000s. On Linux, the stack's executable-ness is
controlled by a bit in an executable, and the loader respects that
bit. The bit is set by the linker.

GCC had (and still has) a feature that depends on the executable
stack, so they invented a way to tell the linker to mark the stack
executable. Specifically, if an object file contains a
`.note.GNU-stack` section with the `SHF_EXECSTACK` bit, GNU linker
silently makes the stack of an output file executable.

But the GNU linker's behavior is dangerous. If you accidentally link
an object file that has that marker section, the entire stack area
silently becomes executable, disabling the security mechanism.

Therefore, mold simply ignores that marker section. If you are using
mold, you need to explicitly pass `-z execstack` to the linker to make
the stack executable.

# What caused this issue?

You are likely to use GCC's [Nested
Functions](https://gcc.gnu.org/onlinedocs/gcc/Nested-Functions.html)
feature which still depends on the executable stack.

# How to fix it?

If you know what you are doing, pass `-z execstack` to mold. Beware
that this will significantly weaken your program's security.

If you don't want to pass `-z execstack`, rewrite your code so that
your code does not depend on the executable stack.
