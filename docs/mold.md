mold(1) -- a modern linker
==========================

## SYNOPSIS

`mold` [_option_...] _file_...

## DESCRIPTION
`mold` is a faster drop-in replacement for the default GNU ld(1).

### How to use
See <https://github.com/rui314/mold#how-to-use>.

### Compatibility

`mold` is designed to accept the same command-line options as the GNU linkers
for linking user-land programs. If your program cannot be built because `mold`
lacks an option, please file a bug at <https://github.com/rui314/mold/issues>.

`mold` supports a very limited set of linker script features, which is just
sufficient to read `/usr/lib/x86_64-linux-gnu/libc.so` on Linux systems
(despite its name, that file is not a shared library but a text linker script
that refers to the real shared library).

### Archive symbol resolution

Traditionally, Unix linkers are sensitive to the order in which input files
appear on the command line. They process input files from the first (leftmost)
file to the last (rightmost) file one-by-one. While reading input files, they
maintain sets of defined and undefined symbols. When visiting an archive file
(`.a` files), they pull out object files to resolve as many undefined symbols
as possible and move on to the next input file. Object files that weren't
pulled out will never get a second look.

Due to this behavior, you usually have to add archive files at the end of a
command line, so that when a linker reaches archive files, it knows what
symbols remain undefined.

If you put archive files at the beginning of a command line, a linker doesn't
have any undefined symbols, and thus no object files will be pulled out from
archives. You can change the processing order by using the `--start-group` and
`--end-group` options, though they make a linker slower.

`mold` and the LLVM lld(1) linker take a different approach. They
remember which symbols can be resolved from archive files instead of
forgetting them after processing each archive. Therefore, `mold` and lld(1)
can "go back" in a command line to pull out object files from archives if they
are needed to resolve remaining undefined symbols. They are not sensitive to
the input file order.

`--start-group` and `--end-group` are still accepted by `mold` and lld(1)
for compatibility with traditional linkers, but they are silently ignored.

### Dynamic symbol resolution

Some Unix linker features are difficult to understand without comprehending
the semantics of dynamic symbol resolution. Therefore, even though it's not
specific to `mold`, we'll explain it here.

We use "ELF module" or just "module" as a collective term to refer to an
executable or a shared library file in the ELF format.

An ELF module may have lists of imported symbols and exported symbols, as well
as a list of shared library names from which imported symbols should be
imported. The point is that imported symbols are not bound to any specific
shared library until runtime.

Here is how the Unix dynamic linker resolves dynamic symbols. Upon the start
of an ELF program, the dynamic linker constructs a list of ELF modules which,
as a whole, make up a complete program. The executable file is always at
the beginning of the list followed by its dependent shared libraries. The
dynamic linker searches for an imported symbol from the beginning of the list
to the end. If two or more modules define the same symbol, the one that
appears first in the list takes precedence over the others.

These Unix semantics differ from those of systems such as Windows that have a
two-level namespace for dynamic symbols. On Windows, for example, dynamic
symbols are represented as a tuple of (`symbol-name`, `shared-library-name`),
so that each dynamic symbol is guaranteed to be resolved from some specific
library.

Typically, an ELF module that exports a symbol also imports the same symbol.
Such a symbol is usually resolved to itself, but that's not the case if a
module that appears before it in the symbol search list provides another
definition of the same symbol.

Let's take `malloc` as an example. Assume that you define your version of
`malloc` in your main executable file. Then, all `malloc` calls from any
module are resolved to your function instead of the one in libc, because the
executable is always at the beginning of the dynamic symbol search list. Note
that even `malloc` calls within libc are resolved to your definition since
libc exports and imports `malloc`. Therefore, by defining `malloc` yourself,
you can override a library function, and the `malloc` in libc becomes dead
code.

These Unix semantics are tricky and sometimes considered harmful. For example,
assume that you accidentally define `atoi` as a global function in your
executable that behaves completely differently from the one in the C standard.
Then, all `atoi` function calls from any module (even function calls within
libc) are redirected to your function instead of the one in libc, which will
very likely cause a problem. That is a somewhat surprising consequence of an
accidental name conflict. On the other hand, these semantics are sometimes
useful because they allow users to override library functions without
rebuilding modules containing them.

Whether good or bad, you should keep these semantics in mind to understand
Unix linkers' behaviors.

### Build reproducibility

`mold`'s output is deterministic. That is, if you pass the same object files
and the same command-line options to the same version of `mold`, it is
guaranteed that `mold` produces bit-for-bit identical output. The linker's
internal randomness, such as the timing of thread scheduling or iteration
orders of hash tables, doesn't affect the output.

`mold` does not have any host-specific default settings, unlike the GNU
linkers, which have some configurable values, such as system-dependent library
search paths, hard-coded at build time. `mold` depends only on its
command-line arguments.

## OPTION NOTATIONS

Long options are spelled with two dashes. Some also accept a single dash,
but which ones do is somewhat arbitrary for historical reasons. `mold`
follows the conventions of other linkers for compatibility. For example,
`--export-dynamic` can be spelled `-export-dynamic`, while `--execute-only`
requires two dashes.

This can cause unexpected behavior when constructing command-line options in
a script. For example, you might use `"-e$foo"` to set the entry point symbol
to the value of `$foo`. However, if `$foo` is `xport-dynamic`, the argument
becomes `-export-dynamic` and is interpreted as `--export-dynamic`.

To avoid such ambiguities, we recommend always using two dashes for long
options and separating a single-letter option from its argument with a
space. For example, use `-e $foo` or `--entry=$foo` to specify the entry
point symbol in a script.

## MOLD-SPECIFIC OPTIONS

* `--chroot`=_dir_:
  Treat _dir_ as the root directory. Absolute paths of input and output files
  are interpreted as paths under _dir_.

* `--color-diagnostics`=[ _auto_ | _always_ | _never_ ]:
  Show diagnostic messages in color using ANSI escape sequences. `auto` means
  that `mold` prints out messages in color only if the standard error is
  connected to a TTY. The default is `auto`.

* `--color-diagnostics`:
  Synonym for `--color-diagnostics=auto`.

* `--no-color-diagnostics`:
  Synonym for `--color-diagnostics=never`.

* `--detach`, `--no-detach`:
  Create a separate debug info file (see `--separate-debug-file`) in a
  background process, or do not. The default is `--detach`.

* `--fork`, `--no-fork`:
  Spawn a child process and let it do the actual linking. When linking a large
  program, the OS kernel can take a few hundred milliseconds to terminate a
  `mold` process. `--fork` hides that latency. This is the default.

  Note that `--fork` also hides the resource usage statistics reported by
  time(1), since the parent process exits without calling waitpid(2) on the
  child process. If you need those statistics, pass `--no-fork`.

* `--perf`:
  Print the time spent in each pass of the link.

* `--print-dependencies`:
  Print out dependency information for input files.

  Each line of the output for this option shows which file depends on which
  file to use a specific symbol. This option is useful for debugging why some
  object file in a static archive got linked or why some shared library is
  kept in an output file's dependency list even with `--as-needed`.

* `--quick-exit`, `--no-quick-exit`:
  Exit with _exit(2) instead of exit(3), which skips exit handlers and other
  cleanup in the process. The default is `--quick-exit`.

* `--relocatable-merge-sections`:
  By default, `mold` doesn't merge input sections by name when merging input
  object files into a single output object file for `-r`. For example,
  `.text.foo` and `.text.bar` aren't merged for `-r` even though they are
  merged into `.text` based on the default section merging rules.

  This option changes the behavior so that `mold` merges input sections by
  name according to the default section merging rules.

* `--repro`:
  Archive input files, as well as a text file containing command line options,
  in a tar file so that you can run `mold` with the exact same inputs again.
  This is useful for reporting a bug with a reproducer. The output filename is
  `path/to/output.tar`, where `path/to/output` is the output filename given
  by `-o`.

* `--reverse-sections`:
  Reverse the order of input sections before assigning them offsets in the
  output file.

  This option is useful for finding bugs that depend on the initialization
  order of global objects. In C++, constructors of global objects in a single
  source file are guaranteed to be executed in the source order, but there's
  no such guarantee across compilation units. Usually, constructors are
  executed in the order given to the linker, but depending on it is a mistake.

  By reversing the order of input sections using `--reverse-sections`, you can
  easily test that your program works in the reversed initialization order.

* `--run` _command_ _arg_...:
  Run _command_ with `mold` as `/usr/bin/ld`. Specifically, `mold` runs a
  given command with the `LD_PRELOAD` environment variable set to intercept
  exec(3) family functions, which replace `argv[0]` with `mold` if it is `ld`,
  `ld.bfd`, `ld.gold`, or `ld.lld`.

* `--separate-debug-file`, `--separate-debug-file`=_file_:
  Bundle debug info sections into a separate file instead of embedding them in
  an output executable or a shared library. `mold` creates a debug info file
  in the background by default, so that you can start running your executable
  as soon as possible.

  By default, the debug info file is created in the same directory as the
  output file, with the `.dbg` file extension. That filename is embedded into
  the output file so that `gdb` can automatically find the debug info file for
  the output file. For more information about gdb features related to separate
  debug files, see
  <https://sourceware.org/gdb/current/onlinedocs/gdb.html/Separate-Debug-Files.html>.

  `mold` holds a file lock with flock(2) while creating a debug info file in
  the background.

  If you don't want to create a debug info file in the background, pass the
  `--no-detach` option.

* `--shuffle-sections`, `--shuffle-sections`=_number_:
  Randomize the output by shuffling the order of input sections before
  assigning them offsets in the output file. If a _number_ is given, it's
  used as a seed for the random number generator, so that the linker produces
  the same output for the same seed. If no seed is given, a random number is
  used as a seed.

  This option is useful for benchmarking. Modern CPUs are sensitive to a
  program's memory layout. A seemingly benign change in program layout, such
  as a small size increase of a function in the middle of a program, can
  affect the program's performance. Therefore, even if you write new code and
  get a good benchmark result, it is hard to say whether the new code improves
  the program's performance; it is possible that the new memory layout happens
  to perform better.

  By running a benchmark multiple times with randomized memory layouts using
  `--shuffle-sections`, you can isolate your program's real performance number
  from the randomness caused by memory layout changes.

* `--spare-program-headers`=_number_:
  Append the given number of `PT_NULL` entries to the end of the program
  header, so that post-link processing tools can easily add new segments by
  overwriting the null entries.

  Note that ELF requires all `PT_LOAD` segments to be sorted by `p_vaddr`.
  Therefore, if you add a new LOAD segment, you may need to sort the entire
  program header.

* `--stats`:
  Print statistics about the input, such as the numbers of defined and
  undefined symbols, relocations, and comdat groups.

* `--thread-count`=_count_:
  Use _count_ threads.

* `--threads`, `--no-threads`:
  Use multiple threads. By default, `mold` uses as many threads as the number of
  cores or 32, whichever is smaller. It is capped at 32 because `mold`
  doesn't scale well beyond that point. To use only one thread, pass
  `--no-threads` or `--thread-count=1`.

* `--zero-to-bss`:
  Convert all-zero data sections into BSS.

  When this option is enabled, `mold` scans input data sections that are not
  of type `SHT_NOBITS` and checks whether their contents consist solely of
  zero bytes. Such sections are then converted into BSS (`SHT_NOBITS`)
  sections. This reduces the size of the output file, since BSS sections
  occupy no space in the file image.

  This behavior is especially useful for user-defined sections created with
  `__attribute__((section(".sectname")))` that contain uninitialized global
  variables. GCC and Clang do not automatically mark such sections as BSS
  even if their contents are entirely zero, and instead emit them as regular
  data sections.

  For example, consider `__attribute__((section(".sectname"))) int vec[256];`.

  By default, this results in a `.sectname` section of type `SHT_PROGBITS`
  filled with zeros. With `--zero-to-bss`, the linker will recognize it as
  empty data and convert it to a `SHT_NOBITS` section, reducing the output
  file size without changing runtime semantics.

* `-z rewrite-endbr`, `-z norewrite-endbr`:
  As a security measure, some CPU instruction sets have recently gained a
  feature to protect control flow integrity by disallowing indirect branches
  by default. If the feature is enabled, the instruction that is executed
  immediately after an indirect branch must be a branch target marker
  instruction, or the CPU raises a fault. The marker instruction is also
  known as a "landing pad" instruction, at which indirect branches can land.
  This feature makes ROP attacks harder to conduct.

  To use the feature, a function whose pointer is taken needs to begin with a
  landing pad because a function call via a function pointer is compiled to an
  indirect branch. On the other hand, if a function is called only directly
  (i.e. referred to only by _direct_ branch instructions), it doesn't have to
  begin with it.

  By default, the compiler always emits a landing pad at the beginning of each
  global function because it doesn't know whether or not the function's
  pointer is taken in another translation unit. As a result, the resulting
  binary has more attack surface than necessary.

  If `-z rewrite-endbr` is given, `mold` performs a whole-program analysis
  to identify functions whose addresses are actually taken and rewrites
  landing pads with no-ops for non-address-taken functions, reducing the
  attack surface.

  This feature is currently available only on x86-64 and arm64.

  On arm64, the landing pad is `bti c`. Note that code compiled with
  `-mbranch-protection=standard` (BTI + pointer authentication) starts
  functions with `paciasp`, which doubles as a landing pad, so there is no
  separate `bti c` to rewrite and this option has no effect on such code.

## GNU-COMPATIBLE OPTIONS

* `--help`:
  Report usage information to stdout and exit.

* `-v`, `--version`:
  Report version information to stdout.

* `-V`:
  Report version and target information to stdout.

* `-E`, `--export-dynamic`, `--no-export-dynamic`:
  When creating an executable, using the `-E` option causes all global symbols
  to be put into the dynamic symbol table, so that the symbols are visible
  from other ELF modules at runtime.

  By default, or if `--no-export-dynamic` is given, only symbols that are
  referenced by DSOs at link-time are exported from an executable.

* `-F` _libname_, `--filter`=_libname_:
  Set the `DT_FILTER` dynamic section field to _libname_.

* `-I` _file_, `--dynamic-linker`=_file_, `--no-dynamic-linker`:
  Set the dynamic linker path to _file_. If no `-I` option is given, or if
  `--no-dynamic-linker` is given, no dynamic linker path is set in the output
  file. This is unlike the GNU linkers, which set a default dynamic linker
  path in that case. This doesn't usually matter because the compiler driver
  always passes `-I` to the linker.

* `-L` _dir_, `--library-path`=_dir_:
  Add _dir_ to the list of library search paths in which `mold` searches for
  libraries for the `-l` option.

  Unlike the GNU linkers, `mold` does not have default search paths. This
  doesn't usually matter because the compiler driver always passes all
  necessary search paths to the linker.

* `-M`, `--print-map`:
  Write a map file to stdout.

* `-N`, `--omagic`, `--no-omagic`:
  Force `mold` to emit an output file with an old-fashioned memory layout.
  First, it makes the first data segment not aligned to a page boundary.
  Second, text segments are marked as writable if the option is given.

* `-S`, `--strip-debug`:
  Omit `.debug_*` sections from the output file.

* `-T` _file_, `--script`=_file_:
  Read a linker script from _file_.

* `-X`, `--discard-locals`:
  Discard temporary local symbols to reduce the sizes of the symbol table and
  the string table. Temporary local symbols are local symbols starting with
  `.L`. Compilers usually generate such symbols for unnamed program elements
  such as string literals or floating-point literals. This is the default;
  use `--discard-none` to keep temporary local symbols.

* `-e` _symbol_, `--entry`=_symbol_:
  Use _symbol_ as the entry point symbol instead of the default entry
  point symbol `_start`.

* `-f` _shlib_, `--auxiliary`=_shlib_:
  Set the `DT_AUXILIARY` dynamic section field to _shlib_.

* `-h` _libname_, `--soname`=_libname_:
  Set the `DT_SONAME` dynamic section field to _libname_. This option is used
  when creating a shared object file. Typically, when you create `libfoo.so`,
  you want to pass `--soname=libfoo.so` to a linker.

* `-l` _libname_:
  Search for `lib`_libname_`.so` or `lib`_libname_`.a` in the library search
  paths.

* `-m` _target_:
  Link for _target_, which is an emulation name such as `elf_x86_64` or
  `aarch64linux`. If this option is not given, `mold` infers the target from
  the input object files.

* `-n`, `--nmagic`, `--no-nmagic`:
  Do not align sections to page boundaries.

* `-o` _file_, `--output`=_file_:
  Use _file_ as the output file name instead of the default name `a.out`.

* `-r`, `-i`, `--relocatable`:
  Instead of generating an executable or a shared object file, combine input
  object files to generate another object file that can be used as an input to
  a linker.

* `-s`, `--strip-all`:
  Omit the `.symtab` section from the output file.

* `-t`, `--trace`:
  Print the name of each input file.

* `-u` _symbol_, `--undefined`=_symbol_:
  If _symbol_ remains undefined after reading all object files,
  and if there is a static archive that contains an object file defining
  _symbol_, pull out the object file and link it so that the output file
  contains a definition of _symbol_.

* `-y` _symbol_, `--trace-symbol`=_symbol_:
  Trace references to _symbol_.

* `--Bdynamic`:
  Link against shared libraries.

* `--Bstatic`:
  Do not link against shared libraries.

* `--Bsymbolic`:
  When creating a shared library, make global symbols export-only (i.e. do not
  import the same symbol). As a result, references within a shared library are
  always resolved locally, negating symbol override at runtime. See "Dynamic
  symbol resolution" for more information about symbol imports and exports.

* `--Bsymbolic-functions`:
  This option has the same effect as `--Bsymbolic` but works only for function
  symbols. Data symbols are still both imported and exported.

* `--Bsymbolic-non-weak`:
  This option has the same effect as `--Bsymbolic` but works only for non-weak
  symbols. Weak symbols are still both imported and exported.

* `--Bsymbolic-non-weak-functions`:
  This option has the same effect as `--Bsymbolic` but works only for non-weak
  function symbols. Data symbols and weak function symbols are still both
  imported and exported.

* `--Bno-symbolic`:
  Cancel `--Bsymbolic`, `--Bsymbolic-functions`, `--Bsymbolic-non-weak` and
  `--Bsymbolic-non-weak-functions`.

* `--Map`=_file_:
  Write a map file to _file_.

* `--Tbss`=_address_:
  Alias for `--section-start=.bss=`_address_.

* `--Tdata`=_address_:
  Alias for `--section-start=.data=`_address_.

* `--Ttext`=_address_:
  Alias for `--section-start=.text=`_address_.

* `--Ttext-segment`=_address_:
  Set the address of the first byte of the text segment.

* `--allow-multiple-definition`:
  Normally, the linker reports an error if there is more than one definition
  of a symbol. This option changes the default behavior so that it doesn't
  report an error for duplicate definitions and instead uses the first
  definition.

* `--allow-shlib-undefined`, `--no-allow-shlib-undefined`:
  Even if `mold` succeeds in linking a main executable without undefined
  symbol errors, you may still encounter symbol lookup errors at runtime
  because some symbols that shared libraries refer to are not defined in any
  ELF module. This occurs because `mold` ignores undefined symbols in shared
  libraries by default.

  If you pass `--no-allow-shlib-undefined`, `mold` verifies that undefined
  symbols in shared libraries given to the linker can be resolved at
  link-time. In other words, this converts the runtime error to a link-time
  error.

  Note that you need to pass all shared libraries, including indirectly
  dependent ones, to the linker as arguments for `-l`. If a shared library
  depends on a library that's not passed to the linker, the verification will
  be skipped for that file.

* `--as-needed`, `--no-as-needed`:
  By default, shared libraries given to the linker are unconditionally added
  to the list of required libraries in an output file. However, shared
  libraries after `--as-needed` are added to the list only when at least one
  symbol is actually used by the output file. In other words, shared libraries
  after `--as-needed` are not added to the list of needed libraries if they
  are not needed by a program.

  The `--no-as-needed` option restores the default behavior for subsequent
  files.

* `--build-id`=[ `md5` | `sha1` | `sha256` | `fast` | `uuid` | `0x`_hexstring_ | `none` ]:
  Create a `.note.gnu.build-id` section containing a byte string to uniquely
  identify an output file. `sha256` computes a 256-bit cryptographic hash of
  the output file and uses it as the build ID. `md5` and `sha1` compute the
  same hash but truncate it to 128 and 160 bits, respectively, before using it
  as the build ID. `uuid` uses a random 128-bit UUID. `0x`_hexstring_ uses
  _hexstring_. `fast` is a synonym for `sha256`.

* `--build-id`:
  Synonym for `--build-id=sha256`.

* `--no-build-id`:
  Synonym for `--build-id=none`.

* `--compress-debug-sections`=[ `zlib` | `zlib-gabi` | `zlib:`_N_ | `zstd` | `zstd:`_N_ | `none` ]:
  Compress DWARF debug info (`.debug_*` sections) using the zlib or zstd
  compression algorithm. `zlib-gabi` is an alias for `zlib`. `zlib:`_N_
  selects the zlib compression level, where _N_ is between 0 and 9 (0
  means no compression), and `zstd:`_N_ selects the zstd compression
  level, where _N_ is between 1 and 22. Higher levels achieve better
  compression ratios but are slower. `zlib` is equivalent to `zlib:1`
  and `zstd` is equivalent to `zstd:3`.

* `--defsym`=_symbol_=_value_:
  Define _symbol_ as an alias for _value_.

  _value_ is either an integer (in decimal or hexadecimal with `0x` prefix) or
  a symbol name. If an integer is given as a value, _symbol_ is defined as an
  absolute symbol with the given value.

* `--default-symver`:
  Use soname as a symbol version and append that version to all symbols.

* `--demangle`, `--no-demangle`:
  Demangle C++ and Rust symbols in log messages.

* `--dependency-file`=_file_:
  Write a dependency file to _file_. The written file is readable by make(1)
  and defines only one rule, with the linker's output file as the target and
  all input files as its prerequisites. Users are expected to include the
  generated dependency file in a Makefile to automate the dependency
  management. This option is analogous to the compiler's `-MM -MF` options.

* `--discard-none`:
  Keep all local symbols in the symbol table, including temporary ones
  starting with `.L`, except ones in mergeable sections. This restores the
  default behavior of GNU ld. `mold` and `ld.lld` discard temporary local
  symbols by default.

* `--dynamic-list`=_file_:
  Read a list of dynamic symbols from _file_. Same as
  `--export-dynamic-symbol-list`, except that it implies `--Bsymbolic`. If
  _file_ does not exist in the current directory, it is searched for in the
  library search paths for the sake of compatibility with GNU ld.

* `--eh-frame-hdr`, `--no-eh-frame-hdr`:
  Create an `.eh_frame_hdr` section.

* `--emit-relocs`:
  The linker usually "consumes" relocation sections. That is, the linker
  applies relocations to other sections, and relocation sections themselves
  are discarded.

  The `--emit-relocs` option instructs the linker to leave relocation sections
  in the output file. Some post-link binary analysis or optimization tools such
  as LLVM BOLT need them.

* `--enable-new-dtags`, `--disable-new-dtags`:
  By default, `mold` emits `DT_RUNPATH` for `--rpath`. If you pass
  `--disable-new-dtags`, `mold` emits `DT_RPATH` for `--rpath` instead.

* `--execute-only`:
  Traditionally, setting the executable bit to 1 for a memory page implies
  that the page also becomes readable, which allows machine code to be read
  as data at runtime. That is actually what an attacker often does after
  gaining limited control of a process to find pieces of machine code
  they can use to gain full control of the process. As a mitigation,
  recent processors including some ARM64 ones allow "execute-only" pages.
  If a page is execute-only, you can call a function there as long as you
  know its address but can't read it as data.

  This option marks text segments as execute-only by setting just the "X"
  bit instead of "RX". Note that on most systems, the absence of the "R"
  bit in the text segment serves only as a hint. If you run a program
  linked with `--execute-only` on a processor that doesn't support
  execute-only pages, your executable will likely still function normally,
  but the text segment will remain readable.

* `--exclude-libs`=_libraries_ ...:
  Mark all symbols in the given _libraries_ as hidden.

* `--export-dynamic-symbol`=_symbol_:
  Put symbols matching _symbol_ in the dynamic symbol table. _symbol_ may be a
  glob pattern in the same syntax as for the `--export-dynamic-symbol-list` or
  `--version-script` options.

* `--export-dynamic-symbol-list`=_file_:
  Read a list of dynamic symbols from _file_.

* `--fatal-warnings`, `--no-fatal-warnings`:
  Treat warnings as errors.

* `--fini`=_symbol_:
  Call _symbol_ at unload-time.

* `--gc-sections`, `--no-gc-sections`:
  Remove unreferenced sections.

* `--gdb-index`:
  Create a `.gdb_index` section to speed up the GNU debugger. To use this, you
  need to compile source files with the `-ggnu-pubnames` compiler flag.

* `--hash-style`=[ `sysv` | `gnu` | `both` | `none` ]:
  Choose the format of the hash table for dynamic symbols: `sysv` creates a
  `.hash` section, `gnu` creates a `.gnu.hash` section, `both` creates both,
  and `none` creates neither. The default is `both`.

* `--icf`=[ `safe` | `all` | `none` ], `--no-icf`:
  It is not uncommon for a program to contain many identical functions that
  differ only in name. For example, a C++ template `std::vector` is very
  likely to be instantiated into identical code for `std::vector<int>` and
  `std::vector<unsigned>` because the container cares only about the size of
  the parameter type. Identical Code Folding (ICF) is a size optimization to
  identify and merge such identical functions.

  If `--icf=all` is given, `mold` tries to merge all identical functions. This
  reduces the size of the output the most, but it is not a "safe"
  optimization. It is guaranteed in C and C++ that two pointers pointing to
  two different functions will never be equal, but `--icf=all` breaks that
  assumption as two identical functions have the same address after merging.
  So care must be taken when you use this flag that your program does not
  depend on the function pointer uniqueness.

  `--icf=safe` is a flag to merge functions only when it is safe to do so.
  That is, if a program does not take the address of a function, it is safe
  to merge that function with other functions, as you cannot compare a
  function pointer with something else without taking the address of a
  function.

  `--icf=safe` needs to be used with a compiler that supports the
  `.llvm_addrsig` section, which contains the information as to what symbols
  are address-taken. LLVM/Clang supports that section by default. Since GCC
  does not support it yet, you cannot use `--icf=safe` with GCC (it doesn't
  do any harm but can't optimize at all).

  `--icf=none` and `--no-icf` disable ICF.

* `--ignore-data-address-equality`:
  Make ICF merge not only functions but also data. This option should be
  used in combination with `--icf=all`.

* `--image-base`=_addr_:
  Set the base address to _addr_.

* `--init`=_symbol_:
  Call _symbol_ at load-time.

* `--mmap-output-file`, `--no-mmap-output-file`:
  Write or do not write the output file through a memory mapping. With
  `--no-mmap-output-file`, `mold` assembles the output in memory and writes it
  to the file in one go at the end, which may be faster on a filesystem that is
  slow at random writes. By default, the output file is memory-mapped.

* `--no-undefined`:
  Report undefined symbols (even with `--shared`).

* `--noinhibit-exec`:
  Create an output file even if errors occur.

* `--package-metadata`=_percent-encoded-string_:
  Embed a specified string into the `.note.package` section. This option
  is designed for build scripts that generate binary packages, such as
  `.rpm` or `.deb`, to include package metadata in each executable. It
  simplifies the process of identifying the corresponding package for a
  given executable or core file.

  The argument to this option is treated as percent-encoded and decoded
  before being inserted into the section, allowing you to avoid the use of
  the comma (`,`) character in the argument. This is useful because the
  compiler splits the argument of `-Wl,` at commas into separate arguments
  before forwarding them to the linker. Note that `mold` always interprets the
  argument as percent-encoded, so you also need to escape all occurrences
  of `%` as `%25`.

* `--pack-dyn-relocs`=[ `relr` | `android` | `android+relr` | `none` ]:
  Compress dynamic relocations in the output file.

  If `relr` is specified, all `R_*_RELATIVE` relocations are put into the
  `.relr.dyn` section instead of the `.rel.dyn` or `.rela.dyn` section. Since
  the `.relr.dyn` section uses a space-efficient encoding scheme, specifying
  this flag can reduce the size of the output. This is typically most
  effective for position-independent executables.

  If `android` is specified, all dynamic relocations are encoded in Android's
  packed relocation format (APS2) and stored in the `.rel.dyn` or `.rela.dyn`
  section, whose section type is changed to `SHT_ANDROID_REL` or
  `SHT_ANDROID_RELA`. The dynamic table is updated to use `DT_ANDROID_REL`
  / `DT_ANDROID_RELA` and their `SZ` variants in place of the standard tags.

  `android+relr` combines both: `R_*_RELATIVE` relocations go to `.relr.dyn`
  and the rest are APS2-encoded. This is the configuration used by most
  Android system binaries.

  Note that a runtime loader has to support `.relr.dyn` to run executables or
  shared libraries linked with `--pack-dyn-relocs=relr`. As of 2026, glibc
  (since 2.36), musl (since 1.2.4), Android, ChromeOS, and Fuchsia all
  support it. The APS2 format, on the other hand, is only handled by
  Android's bionic loader; the encoder itself is target-agnostic, but the
  produced binary can only be loaded on platforms where bionic (or a
  compatible loader) handles `DT_ANDROID_REL(A)`.

* `--pie`, `--pic-executable`, `--no-pie`, `--no-pic-executable`:
  Create a position-independent executable.

* `--print-gc-sections`, `--no-print-gc-sections`:
  Print removed unreferenced sections.

* `--print-icf-sections`, `--no-print-icf-sections`:
  Print folded identical sections.

* `--push-state`, `--pop-state`:
  `--push-state` saves the current values of `--as-needed`, `--whole-archive`,
  `--static`, and `--start-lib`. The saved values can be restored by
  `--pop-state`.

  `--push-state` and `--pop-state` pairs can nest.

  These options are useful when you want to construct linker command line
  options programmatically. For example, if you want to link `libfoo.so` on an
  as-needed basis but don't want to change the global state of `--as-needed`,
  you can append `--push-state --as-needed -lfoo --pop-state` to the linker
  command line options.

* `--relax`, `--no-relax`:
  Rewrite machine instructions with more efficient ones for some relocations.
  The feature is enabled by default.

* `--require-defined`=_symbol_:
  Like `--undefined`, except the new symbol must be defined by the end of the
  link.

* `--retain-symbols-file`=_file_:
  Keep only symbols listed in _file_. _file_ is a text file containing a
  symbol name on each line. `mold` discards all local symbols as well as
  global symbols that are not in _file_. Note that this option removes symbols
  only from the `.symtab` section and does not affect the `.dynsym` section,
  which is used for dynamic linking.

* `--rpath`=_dir_:
  Add _dir_ to runtime search paths.

* `--section-start`=_section_=_address_:
  Set the address of _section_ to _address_. _address_ is a hexadecimal number
  that may start with an optional `0x`.

* `--shared`, `--Bshareable`:
  Create a shared library.

* `--spare-dynamic-tags`=_number_:
  Append the given number of `DT_NULL` entries to the end of the `.dynamic`
  section, so that post-link processing tools can easily add new dynamic tags
  by overwriting the null entries.

* `--start-lib`, `--end-lib`:
  Handle object files between `--start-lib` and `--end-lib` as if they were in
  an archive file. That means object files between them are linked only when
  they are needed to resolve undefined symbols. The options are useful if you
  want to link object files only when they are needed but want to avoid the
  overhead of running ar(1).

* `--static`:
  Do not link against shared libraries.

* `--sysroot`=_dir_:
  Set the target system's root directory to _dir_.

* `--undefined-glob`=_pattern_:
  Same as `--undefined`, except that `--undefined-glob` takes a glob
  pattern instead of just a single symbol name.

* `--undefined-version`, `--no-undefined-version`:
  By default, `mold` warns about a symbol specified by a version script or by
  `--export-dynamic-symbol` if it is not defined. You can silence the warning
  with `--undefined-version`.

* `--unique`=_pattern_:
  Don't merge input sections that match the given glob pattern _pattern_.

* `--unresolved-symbols`=[ `report-all` | `ignore-all` | `ignore-in-object-files` | `ignore-in-shared-libs` ]:
  Specify how to handle undefined symbols.

* `--version-script`=_file_:
  Read a version script from _file_. If _file_ does not exist in the current
  directory, it is searched for in the library search paths for the sake of
  compatibility with GNU ld.

* `-w`, `--no-warnings`:
  Suppress warnings and cancel `--fatal-warnings`.

* `--warn-common`, `--no-warn-common`:
  Warn about common symbols.

* `--warn-unresolved-symbols`, `--error-unresolved-symbols`:
  Normally, the linker reports an error for unresolved symbols.
  The `--warn-unresolved-symbols` option turns it into a warning. The
  `--error-unresolved-symbols` option restores the default behavior.

* `--whole-archive`, `--no-whole-archive`:
  When archive files (`.a` files) are given to the linker, only object files
  that are needed to resolve undefined symbols are extracted from them and
  linked into the output file. `--whole-archive` changes that behavior for
  subsequent archives so that the linker extracts all object files and links
  them into the output. For example, if you are creating a shared object file
  and you want to include all archive members in the output, you should pass
  `--whole-archive`. `--no-whole-archive` restores the default behavior for
  subsequent archives.

* `--wrap`=_symbol_:
  Resolve references to _symbol_ to `__wrap_`_symbol_, and references to
  `__real_`_symbol_ to the original _symbol_. This option is typically used
  for wrapping an existing function.

* `-z cet-report`=[ `warning` | `error` | `none` ]:
  Intel Control-flow Enforcement Technology (CET) is an x86 feature available
  since Tiger Lake, which was released in 2020. It defines new instructions to
  protect programs from control hijacking attacks. You can tell the compiler
  to use the feature by specifying the `-fcf-protection` flag.

  The `-z cet-report` flag is used to make sure that all object files were
  compiled with a correct `-fcf-protection` flag. If `warning` or `error` is
  given, `mold` prints out a warning or an error message if an object file was
  not compiled with the compiler flag.

  `mold` looks for the `GNU_PROPERTY_X86_FEATURE_1_IBT` and
  `GNU_PROPERTY_X86_FEATURE_1_SHSTK` bits in the `.note.gnu.property` section
  to determine whether or not an object file was compiled with
  `-fcf-protection`.

* `-z bti-report`=[ `warning` | `error` | `none` ]:
  Branch Target Identification (BTI) is an ARM64 feature to protect programs
  from control hijacking attacks by requiring indirect branches to land on
  `bti` instructions. You can tell the compiler to use the feature by
  specifying the `-mbranch-protection=bti` or `-mbranch-protection=standard`
  flag. An output file is marked as BTI-enabled only if all object files are.

  The `-z bti-report` flag is used to make sure that all object files were
  compiled with BTI. If `warning` or `error` is given, `mold` prints out a
  warning or an error message if an object file has neither the
  `GNU_PROPERTY_AARCH64_FEATURE_1_BTI` bit in its `.note.gnu.property`
  section nor `Tag_Feature_BTI` in its build attributes.

* `-z force-bti`:
  Turn on the `GNU_PROPERTY_AARCH64_FEATURE_1_BTI` bit in the
  `.note.gnu.property` section even if some object files were not compiled
  with BTI, and generate a PLT with BTI landing pads. This option implies
  `-z bti-report=warning`.

* `-z gcs`=[ `implicit` | `always` | `never` ]:
  Guarded Control Stack (GCS) is an ARM64 shadow stack feature. With
  `implicit`, which is the default, the `GNU_PROPERTY_AARCH64_FEATURE_1_GCS`
  bit in the `.note.gnu.property` section is turned on only if all object
  files have the bit. `always` turns it on regardless of object files and
  implies `-z gcs-report=warning` and `-z gcs-report-dynamic=warning`.
  `never` turns it off.

* `-z gcs-report`=[ `warning` | `error` | `none` ]:
  Print out a warning or an error message if an object file has neither the
  `GNU_PROPERTY_AARCH64_FEATURE_1_GCS` bit in its `.note.gnu.property`
  section nor `Tag_Feature_GCS` in its build attributes.

* `-z gcs-report-dynamic`=[ `warning` | `error` | `none` ]:
  Print out a warning or an error message if a shared library lacks the
  `GNU_PROPERTY_AARCH64_FEATURE_1_GCS` bit in its `.note.gnu.property`
  section.

* `-z pac-plt`:
  Generate an ARM64 PLT that authenticates function addresses read from
  `.got.plt` using Pointer Authentication (PAC), and turn on the
  `GNU_PROPERTY_AARCH64_FEATURE_1_PAC` bit in the `.note.gnu.property`
  section.
  The output works only with a dynamic linker that signs `.got.plt` entries.

* `-z now`, `-z lazy`:
  By default, references to functions in other ELF modules are resolved by
  the dynamic linker when the functions are called for the first time. `-z
  now` marks an executable or a shared library file so that all dynamic
  symbols are resolved when the file is loaded into memory. `-z lazy` restores
  the default behavior.

* `-z origin`:
  Mark the object as requiring immediate `$ORIGIN` processing at runtime.

* `-z ibt`:
  Turn on the `GNU_PROPERTY_X86_FEATURE_1_IBT` bit in the `.note.gnu.property`
  section to indicate that the output uses an IBT-enabled PLT. This option
  implies `-z ibtplt`.

* `-z ibtplt`:
  Generate an Intel Indirect Branch Tracking (IBT)-enabled PLT. This is the
  default on x86-64.

* `-z execstack`, `-z noexecstack`:
  By default, the pages for the stack area (i.e. the pages where local
  variables reside) are not executable for security reasons. `-z execstack`
  makes them executable. `-z noexecstack` restores the default behavior.

* `-z keep-text-section-prefix`, `-z nokeep-text-section-prefix`:
  Keep `.text.hot`, `.text.unknown`, `.text.unlikely`, `.text.startup`, and
  `.text.exit` as separate sections in the final binary instead of merging
  them into `.text`.

* `-z rodynamic`:
  Make the `.dynamic` section read-only.

* `-z relro`, `-z norelro`:
  Some sections such as `.dynamic` have to be writable only while a module is
  being loaded into memory. Once the dynamic linker finishes its job, such
  sections won't be mutated by anyone. As a security mitigation, it is
  preferable to make such sections read-only during program execution.

  `-z relro` puts such sections into a special segment called `relro`. The
  dynamic linker makes a relro segment read-only after it finishes its job.

  By default, `mold` generates a relro segment. `-z norelro` disables the
  feature.

* `-z sectionheader`, `-z nosectionheader`:
  `-z nosectionheader` tells the linker to omit the section header.
  By default, the linker does not omit the section header.

* `-z separate-loadable-segments`, `-z separate-code`, `-z noseparate-code`:
  If one memory page contains multiple segments, the page protection bits are
  set in such a way that the needed attributes (writable or executable) are
  satisfied for all segments. This usually happens at a boundary of two
  segments with two different attributes.

  `separate-loadable-segments` adds padding between segments with different
  attributes so that they do not share the same page.

  `separate-code` adds padding only between executable and non-executable
  segments.

  `noseparate-code` does not add any padding between segments. This is the
  default.

* `-z defs`, `-z undefs`:
  Report undefined symbols (even with `--shared`).

* `-z shstk`:
  Enforce shadow stack by turning on the `GNU_PROPERTY_X86_FEATURE_1_SHSTK`
  bit in the `.note.gnu.property` output section. Shadow stack is part of
  Intel Control-flow Enforcement Technology (CET), which is available since
  Tiger Lake (2020).

* `-z start-stop-visibility`=[ `hidden` | `protected` ]:
  If a section name is valid as a C identifier (i.e., it matches
  `/^[_a-zA-Z][_a-zA-Z0-9]*$/`), `mold` creates `__start_SECNAME` and
  `__stop_SECNAME` symbols to mark the beginning and end of the section,
  where `SECNAME` is the section name.

  You can make these marker symbols visible from other ELF modules by passing
  `-z start-stop-visibility=protected`. The default is `hidden`.

* `-z text`, `-z notext`, `-z textoff`:
  `mold` by default reports an error if dynamic relocations are created in
  read-only sections. If `-z notext` or `-z textoff` is given, `mold` creates
  such dynamic relocations without reporting an error. `-z text` restores the
  default behavior.

* `-z max-page-size`=_number_:
  Some CPU ISAs support multiple memory page sizes. This option specifies the
  maximum page size that an output binary can run on. In general, binaries
  built for a larger page size can run on a system with a smaller page size,
  but not vice versa. The default value is 4 KiB for i386, x86-64, and RISC-V,
  and 64 KiB for ARM64.

* `-z nodefaultlib`:
  Make the dynamic loader ignore default search paths.

* `-z nodelete`:
  Mark the DSO as non-deletable at runtime.

* `-z nodlopen`:
  Mark the DSO as unavailable to dlopen(3). This option makes it possible for
  the linker to optimize thread-local variable accesses by rewriting
  instructions for some targets.

* `-z nodump`:
  Mark the DSO as unavailable to dldump(3).

* `-z nocopyreloc`:
  Do not create copy relocations.

* `-z global`:
  Set the `DF_1_GLOBAL` flag in the `DT_FLAGS_1` dynamic section field.

* `-z initfirst`:
  Mark the DSO to be initialized first at runtime.

* `-z interpose`:
  Mark the object as interposing all DSOs except the executable.

* `-(`, `-)`, `-EL`, `-O`_number_, `--dc`, `--dp`, `--end-group`, `--no-add-needed`, `--no-copy-dt-needed-entries`, `--nostdlib`, `--rpath-link`=_dir_, `--sort-common`, `--sort-section`, `--start-group`, `--warn-constructors`, `--warn-once`, `--fix-cortex-a53-835769`, `--fix-cortex-a53-843419`, `-z combreloc`, `-z common-page-size`, `-z nocombreloc`:
  Ignored.

## ENVIRONMENT VARIABLES

* `MOLD_JOBS`:
  If this variable is set to `1`, only one `mold` process will run at a time.
  If a new `mold` process is initiated while another is already active, the
  new process will wait until the active one completes before starting.

  The primary reason for this environment variable is to minimize peak memory
  usage. Since `mold` is designed to operate with high parallelism, running
  multiple `mold` instances simultaneously may not be beneficial. If you
  execute N instances of `mold` concurrently, it could require N times the
  time and N times the memory. On the other hand, running them one after the
  other might still take N times longer, but the peak memory usage would be
  the same as running just a single instance.

  If your build system invokes multiple linker processes simultaneously and
  some of them often get killed due to out-of-memory errors, you might
  consider setting this environment variable to `1` to see if it addresses the
  OOM issue.

  Currently, any value other than `1` is silently ignored.

* `MOLD_DEBUG`:
  If this variable is set to a non-empty string, `mold` embeds its
  command-line options in the output file's `.comment` section.

* `MOLD_REPRO`:
  Setting this variable to a non-empty string has the same effect as passing
  the `--repro` option.

## SEE ALSO

ld(1), ld.lld(1), elf(5), ld.so(8)

## AUTHOR

Rui Ueyama <ruiu@cs.stanford.edu>

## BUGS

Report bugs to <https://github.com/rui314/mold/issues>.
