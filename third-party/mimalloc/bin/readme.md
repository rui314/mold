# Windows Override

<span id="override_on_windows">Dynamically overriding on mimalloc on Windows</span> 
is robust and has the particular advantage to be able to redirect all malloc/free calls that go through
the (dynamic) C runtime allocator, including those from other DLL's or libraries.
As it intercepts all allocation calls on a low level, it can be used reliably 
on large programs that include other 3rd party components.
There are four requirements to make the overriding work robustly:

1. Use the C-runtime library as a DLL (using the `/MD` or `/MDd` switch).

2. Link your program explicitly with `mimalloc-override.dll` library.
   To ensure the `mimalloc-override.dll` is loaded at run-time it is easiest to insert some
    call to the mimalloc API in the `main` function, like `mi_version()`
    (or use the `/INCLUDE:mi_version` switch on the linker). See the `mimalloc-override-test` project
    for an example on how to use this. 

3. The `mimalloc-redirect.dll` (or `mimalloc-redirect32.dll`) must be put
   in the same folder as the main `mimalloc-override.dll` at runtime (as it is a dependency of that DLL).
   The redirection DLL ensures that all calls to the C runtime malloc API get redirected to
   mimalloc functions (which reside in `mimalloc-override.dll`).

4. Ensure the `mimalloc-override.dll` comes as early as possible in the import
   list of the final executable (so it can intercept all potential allocations).

For best performance on Windows with C++, it
is also recommended to also override the `new`/`delete` operations (by including
[`mimalloc-new-delete.h`](../include/mimalloc-new-delete.h) 
a single(!) source file in your project).

The environment variable `MIMALLOC_DISABLE_REDIRECT=1` can be used to disable dynamic
overriding at run-time. Use `MIMALLOC_VERBOSE=1` to check if mimalloc was successfully redirected.

## Minject

We cannot always re-link an executable with `mimalloc-override.dll`, and similarly, we cannot always
ensure the the DLL comes first in the import table of the final executable.
In many cases though we can patch existing executables without any recompilation
if they are linked with the dynamic C runtime (`ucrtbase.dll`) -- just put the `mimalloc-override.dll`
into the import table (and put `mimalloc-redirect.dll` in the same folder)
Such patching can be done for example with [CFF Explorer](https://ntcore.com/?page_id=388).

The `minject` program can also do this from the command line, use `minject --help` for options:

```
> minject --help

minject:
  Injects the mimalloc dll into the import table of a 64-bit executable,
  and/or ensures that it comes first in het import table.

usage:
  > minject [options] <exe>

options:
  -h   --help        show this help
  -v   --verbose     be verbose
  -l   --list        only list imported modules
  -i   --inplace     update the exe in-place (make sure there is a backup!)
  -f   --force       always overwrite without prompting
       --postfix=<p> use <p> as a postfix to the mimalloc dll (default is 'override')
                     e.g. use --postfix=override-debug to link with mimalloc-override-debug.dll

notes:
  Without '--inplace' an injected <exe> is generated with the same name ending in '-mi'.
  Ensure 'mimalloc-redirect.dll' is in the same folder as the mimalloc dll.

examples:
  > minject --list myprogram.exe
  > minject --force --inplace myprogram.exe
```  
