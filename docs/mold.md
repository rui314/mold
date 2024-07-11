mold(1) -- a modern linker
==========================

## SYNOPSIS

`mold` [_option_...] _file_...

## DESCRIPTION
`mold` is a faster drop-in replacement for the default GNU ld(1).

### How to use
See <https://github.com/rui314/mold#how-to-use>.

### Compatibility

**Mold** is designed to be a drop-in replacement for the GNU linkers for
linking user-land programs. If your user-land program cannot be built due to
missing command-line options, please file a bug at
<https://github.com/rui314/mold/issues>.

Mold supports a very limited set of linker script features, which is just
sufficient to read `/usr/lib/x86_64-linux-gnu/libc.so` on Linux systems (on
Linux, that file is contrary to its name not a shared library but an ASCII
linker script that loads a real `libc.so` file.)

Beyond that, we have no plan to support any additional linker script features.
The linker script is an ad-hoc, over-designed, complex language which we
believe needs to be replaced by a simpler mechanism. We have a plan to add a
replacement for the linker script to `mold` instead.

### Archive symbol resolution

Traditionally, Unix linkers are sensitive to the order in which input files
appear on the command line. They process input files from the first (leftmost)
file to the last (rightmost) file one-by-one. While reading input files, they
maintain sets of defined and undefined symbols. When visiting an archive file
(`.a` files), they pull out object files to resolve as many undefined symbols
as possible and move on to the next input file. Object files that weren't
pulled out will never have a chance for a second look.

Due to this behavior, you usually have to add archive files at the end of a
command line, so that when a linker reaches archive files, it knows what
symbols remain as undefined.

If you put archive files at the beginning of a command line, a linker doesn't
have any undefined symbols, and thus no object files will be pulled out from
archives. You can change the processing order by using the `--start-group` and
`--end-group` options, though they make a linker slower.

`mold`, as well as the LLVM lld(1) linker, takes a different approach. They
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
as a whole, consist of a complete program. The executable file is always at
the beginning of the list followed by its dependent shared libraries. An
imported symbol is searched from the beginning of the list to the end. If two
or more modules define the same symbol, the one that appears first in the list
takes precedence over the others.

This Unix semantics are contrary to systems such as Windows that have a
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
you can overwrite a library function, and the `malloc` in libc becomes dead
code.

These Unix semantics are tricky and sometimes considered harmful. For example,
assume that you accidentally define `atoi` as a global function in your
executable that behaves completely differently from the one in the C standard.
Then, all `atoi` function calls from any modules (even function calls within
libc) are redirected to your function instead of the one in libc, which will
very likely cause a problem.

That is a somewhat surprising consequence for an accidental name conflict. On
the other hand, this semantic is sometimes useful because it allows users to
override library functions without rebuilding modules containing them.

Whether good or bad, you should keep these semantics in mind to understand
Unix linkers' behaviors.

### Build reproducibility

`mold`'s output is deterministic. That is, if you pass the same object files
and the same command-line options to the same version of `mold`, it is
guaranteed that `mold` produces the bit-by-bit identical output. The linker's
internal randomness, such as the timing of thread scheduling or iteration
orders of hash tables, doesn't affect the output.

`mold` does not have any host-specific default settings. This is contrary to the
GNU linkers, for which some configurable values, such as system-dependent
library search paths, are hard-coded. `mold` depends only on its command-line
arguments.

## OPTION NOTATIONS

Multi-letter long options may precede either a single dash or double dashes,
except for those starting with the letter "o". For historical reasons, long
options beginning with "o" must precede double dashes.

For example, you can spell `--as-needed` as `-as-needed`, but `--omagic` must
not be spelled as `-omagic`. `-omagic` will be interpreted not as `--omagic`
but as `-o magic`.

## MOLD-SPECIFIC OPTIONS

* `--chroot`=_dir_:
  Set _dir_ as the root directory.

* `--color-diagnostics`=[ _auto_ | _always_ | _never_ ]:
  Show diagnostic messages in color using ANSI escape sequences. `auto` means
  that `mold` prints out messages in color only if the standard output is
  connected to a TTY. Default is `auto`.

* `--color-diagnostics`:
  Synonym for `--color-diagnostics=auto`.

* `--no-color-diagnostics`:
  Synonym for `--color-diagnostics=never`.

* `--detach`, `--no-detach:
  Permit or do not permit mold to create a debug info file in the background.

* `--fork`, `--no-fork`:
  Spawn a child process and let it do the actual linking. When linking a large
  program, the OS kernel can take a few hundred milliseconds to terminate a
  `mold` process. `--fork` hides that latency. By default, it does fork.

* `--perf`:
  Print performance statistics.

* `--print-dependencies`:
  Print out dependency information for input files.

  Each line of the output for this option shows which file depends on which
  file to use a specific symbol. This option is useful for debugging why some
  object file in a static archive got linked or why some shared library is
  kept in an output file's dependency list even with `--as-needed`.

* `--relocatable-merge-sections`:
  By default, `mold` doesn't merge input sections by name when merging input
  object files into a single output object file for `-r`. For example,
  `.text.foo` and `.text.bar` aren't merged for `-r` even though they are
  merged into `.text` based on the default section merging rules.

  This option changes the behavior so that `mold` merges input sections by
  name by the default section merging rules.

* `--repro`:
  Archive input files, as well as a text file containing command line options,
  in a tar file so that you can run `mold` with the exact same inputs again.
  This is useful for reporting a bug with a reproducer. The output filename is
  `path/to/output.tar`, where `path/to/output` is an output filename specified
  by `-o`.

* `--reverse-sections`:
  Reverse the order of input sections before assigning them the offsets in the
  output file.

  This option is useful for finding bugs that depend on the initialization
  order of global objects. In C++, constructors of global objects in a single
  source file are guaranteed to be executed in the source order, but there's
  no such guarantee across compilation units. Usually, constructors are
  executed in the order given to the linker, but depending on it is a mistake.

  By reversing the order of input sections using `--reverse-sections`, you can
  easily test that your program works in the reversed initialization order.

* `--run` _command_ _arg_...:
  Run _command_ with `mold` `/usr/bin/ld`. Specifically, `mold` runs a given
  command with the `LD_PRELOAD` environment set to intercept exec(3) family
  functions and replaces `argv[0]` with itself if it is `ld`, `ld.gold`, or
  `ld.lld`.

* `--separate-debug-file`, `--separate-debug-file=_file_`:
  Bundle debug info sections into a separate file instead of embedding them in
  an output executable or a shared library. mold creates a debug info file in
  the background by default, so that you can start running your executable as
  soon as possible.

  By default, the debug info file is created in the same directory as is the
  output file, with the `.dbg` file extension. That filename is embedded into
  the output file so that `gdb` can automatically find the debug info file for
  the output file. For more info about gdb features related to separate debug
  files, see
  <https://sourceware.org/gdb/current/onlinedocs/gdb.html/Separate-Debug-Files.html>.

  mold holds a file lock with flock(2) while creating a debug info file in the
  background.

  If you don't want to create a debug info file in the background, pass the
  `--no-detach` option.

* `--shuffle-sections`, `--shuffle-sections`=_number_:
  Randomize the output by shuffling the order of input sections before
  assigning them the offsets in the output file. If a _number_ is given, it's
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
  Print input statistics.

* `--thread-count`=_count_:
  Use _count_ number of threads.

* `--threads`, `--no-threads`:
  Use multiple threads. By default, `mold` uses as many threads as the number of
  cores or 32, whichever is smaller. The reason it is capped at 32 is because
  `mold` doesn't scale well beyond that point. To use only one thread, pass
  `--no-threads` or `--thread-count=1`.

* `--quick-exit`, `--no-quick-exit`:
  Use or do not use `quick_exit` to exit.

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
  `--no-dynamic-linker` is given, no dynamic linker path is set to an output
  file. This is contrary to the GNU linkers which set a default dynamic linker
  path in that case. This difference doesn't usually make any difference
  because the compiler driver always passes `-I` to the linker.

* `-L` _dir_, `--library-path`=_dir_:
  Add _dir_ to the list of library search paths from which `mold` searches
  libraries for the `-l` option.

  Unlike the GNU linkers, `mold` does not have default search paths. This
  difference doesn't usually make any difference because the compiler driver
  always passes all necessary search paths to the linker.

* `-M`, `--print-map`:
  Write a map file to stdout.

* `-N`, `--omagic`, `--no-omagic`:
  Force `mold` to emit an output file with an old-fashioned memory layout.
  First, it makes the first data segment not aligned to a page boundary.
  Second, text segments are marked as writable if the option is given.

* `-S`, `--strip-debug`:
  Omit `.debug_*` sections from the output file.

* `-T` _file_, `--script`=_file_:
  Read linker script from _file_.

* `-X`, `--discard-locals`:
  Discard temporary local symbols to reduce the sizes of the symbol table and
  the string table. Temporary local symbols are local symbols starting with
  `.L`. Compilers usually generate such symbols for unnamed program elements
  such as string literals or floating-point literals.

* `-e` _symbol_, `--entry`=_symbol_:

  Use _symbol_ as the entry point symbol instead of the default entry
  point symbol _start.

* `-f` _shlib_, `--auxiliary`=_shlib_:
  Set the `DT_AUXILIARY` dynamic section field to _shlib_.

* `-h` _libname_, `--soname`=_libname_:
  Set the `DT_SONAME` dynamic section field to _libname_. This option is used
  when creating a shared object file. Typically, when you create `libfoo.so`,
  you want to pass `--soname=foo` to a linker.

* `-l` _libname_:
  Search for `lib`_libname_`.so` or `lib`_libname_`.a` from library search
  paths.

* `-m` _target_:
  Choose a _target_.

* `-o` _file_, `--output`=_file_:
  Use _file_ as the output file name instead of the default name `a.out`.

* `-r`, `--relocatable`:
  Instead of generating an executable or a shared object file, combine input
  object files to generate another object file that can be used as an input to
  a linker.

* `-s`, `--strip-all`:
  Omit `.symtab` section from the output file.

* `-u` _symbol_, `--undefined`=_symbol_:
  If _symbol_ remains as an undefined symbol after reading all object files,
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
  symbols. Data symbols remain being both imported and exported.

* `--Bsymbolic-non-weak`:
  This option has the same effect as `--Bsymbolic` but works only for non-weak
  symbols. Weak symbols remain being both imported and exported.

* `--Bsymbolic-non-weak-functions`:
  This option has the same effect as `--Bsymbolic` but works only for non-weak
  function symbols. Data symbols and weak function symbols remain being both
  imported and exported.

* `--Bno-symbolic`:
  Cancel `--Bsymbolic`, `--Bsymbolic-functions`, `--Bsymbolic-non-weak` and
  `--Bsymbolic-non-weak-functions`.

* `--Map`=_file_:
  Write map file to _file_.

* `--Tbss`=_address_:
  Alias for `--section-start=.bss=`_address_.

* `--Tdata`=_address_:
  Alias for `--section-start=.data=`_address_.

* `--Ttext`=_address_:
  Alias for `--section-start=.text=`_address_.

* `--allow-multiple-definition`:
  Normally, the linker reports an error if there are more than one definition
  of a symbol. This option changes the default behavior so that it doesn't
  report an error for duplicate definitions and instead use the first
  definition.

* `--as-needed`, `--no-as-needed`:
  By default, shared libraries given to the linker are unconditionally added
  to the list of required libraries in an output file. However, shared
  libraries after `--as-needed` are added to the list only when at least one
  symbol is actually used by the output file. In other words, shared libraries
  after `--as-needed` are not added to the list of needed libraries if they
  are not needed by a program.

  The `--no-as-needed` option restores the default behavior for subsequent
  files.

* `--build-id`=[ `md5` | `sha1` | `sha256` | `uuid` | `0x`_hexstring_ | `none` ]:
  Create a `.note.gnu.build-id` section containing a byte string to uniquely
  identify an output file. `sha256` compute a 256-bit cryptographic hash of an
  output file and set it to build-id. `md5` and `sha1` compute the same hash
  but truncate it to 128 and 160 bits, respectively, before setting it to
  build-id. `uuid` sets a random 128-bit UUID. `0x`_hexstring_ sets
  _hexstring_.

* `--build-id`:
  Synonym for `--build-id=sha256`.

* `--no-build-id`:
  Synonym for `--build-id=none`.

* `--compress-debug-sections`=[ `zlib` | `zlib-gabi` | `zstd` | `none` ]:
  Compress DWARF debug info (`.debug_*` sections) using the zlib or zstd
  compression algorithm. `zlib-gabi` is an alias for `zlib`.

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
  Write a dependency file to _file_. The contents of the written file is
  readable by make(1), which defines only one rule with the linker's output
  file as a target and all input files as its prerequisites. Users are
  expected to include the generated dependency file into a Makefile to
  automate the dependency management. This option is analogous to the
  compiler's `-MM -MF` options.

* `--dynamic-list`=_file_:
  Read a list of dynamic symbols from _file_. Same as
  `--export-dynamic-symbol-list`, except that it implies `--Bsymbolic`. If
  _file_ does not exist in the current directory, it is searched from library
  search paths for the sake of compatibility with GNU ld.

* `--eh-frame-hdr`, `--no-eh-frame-hdr`:
  Create `.eh_frame_hdr` section.

* `--emit-relocs`:
  The linker usually "consumes" relocation sections. That is, the linker
  applies relocations to other sections, and relocation sections themselves
  are discarded.

  The `--emit-relocs` instructs the linker to leave relocation sections in the
  output file. Some post-link binary analysis or optimization tools such as
  LLVM Bolt need them.

* `--enable-new-dtags`, `--disable-new-dtags`:
  By default, `mold` emits `DT_RUNPATH` for `--rpath`. If you pass
  `--disable-new-dtags`, `mold` emits `DT_RPATH` for `--rpath` instead.

* `--execute-only`:
  Traditionally, most processors require both executable and readable bits to
  1 to make the page executable, which allows machine code to be read as data
  at runtime. This is actually what an attacker often does after gaining a
  limited control of a process to find pieces of machine code they can use to
  gain the full control of the process. As a mitigation, some recent
  processors allows "execute-only" pages. If a page is execute-only, you can
  call a function there as long as you know its address but can't read it as
  data.

  This option marks text segments execute-only. This option currently works
  only on some ARM64 processors.

* `--exclude-libs`=_libraries_ ...:
  Mark all symbols in the given _libraries_ hidden.

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
  Create a `.gdb_index` section to speed up GNU debugger. To use this, you
  need to compile source files with the `-ggnu-pubnames` compiler flag.

* `--hash-style`=[ `sysv` | `gnu` | `both` | `none` ]:
  Set hash style.

* `--icf`=[ `safe` | `all` | `none` ], `--no-icf`:
  It is not uncommon for a program to contain many identical functions that
  differ only in name. For example, a C++ template `std::vector` is very
  likely to be instantiated to the identical code for `std::vector<int>` and
  `std::vector<unsigned>` because the container cares only about the size of
  the parameter type. Identical Code Folding (ICF) is a size optimization to
  identify and merge such identical functions.

  If `--icf=all` is given, `mold` tries to merge all identical functions. This
  reduces the size of the output most, but it is not a "safe" optimization. It
  is guaranteed in C and C++ that two pointers pointing two different
  functions will never be equal, but `--icf=all` breaks that assumption as two
  identical functions have the same address after merging. So a care must be
  taken when you use this flag that your program does not depend on the
  function pointer uniqueness.

  `--icf=safe` is a flag to merge functions only when it is safe to do so.
  That is, if a program does not take an address of a function, it is safe to
  merge that function with other function, as you cannot compare a function
  pointer with something else without taking an address of a function.

  `--icf=safe` needs to be used with a compiler that supports `.llvm_addrsig`
  section which contains the information as to what symbols are address-taken.
  LLVM/Clang supports that section by default. Since GCC does not support it
  yet, you cannot use `--icf=safe` with GCC (it doesn't do any harm but can't
  optimize at all.)

  `--icf=none` and `--no-icf` disables ICF.

* `--ignore-data-address-equality`:
  Make ICF to merge not only functions but also data. This option should be
  used in combination with `--icf=all`.

* `--image-base`=_addr_:
  Set the base address to _addr_.

* `--init`=_symbol_:
  Call _symbol_ at load-time.

* `--no-undefined`:
  Report undefined symbols (even with `--shared`).

* `--noinhibit-exec`:
  Create an output file even if errors occur.

* `--pack-dyn-relocs`=[ `relr` | `none` ]:
  If `relr` is specified, all `R_*_RELATIVE` relocations are put into
  `.relr.dyn` section instead of `.rel.dyn` or `.rela.dyn` section. Since
  `.relr.dyn` section uses a space-efficient encoding scheme, specifying this
  flag can reduce the size of the output. This is typically most effective for
  position-independent executable.

  Note that a runtime loader has to support `.relr.dyn` to run executables or
  shared libraries linked with `--pack-dyn-relocs=relr`. As of 2022, only
  ChromeOS, Android and Fuchsia support it.

* `--package-metadata`=_string_:
  Embed _string_ to a `.note.package` section. This option is intended to be
  used by a package management command such as rpm(8) to embed metadata
  regarding a package to each executable file.

* `--pie`, `--pic-executable`, `--no-pie`, `--no-pic-executable`:
  Create a position-independent executable.

* `--print-gc-sections`, `--no-print-gc-sections`:
  Print removed unreferenced sections.

* `--print-icf-sections`, `--no-print-icf-sections`:
  Print folded identical sections.

* `--push-state`, `--pop-state`:
  `--push-state` saves the current values of `--as-needed`, `--whole-archive`,
  `--static`, and `--start-lib`. The saved values can be restored by
  pop-state.

  `--push-state` and `--pop-state` pairs can nest.

  These options are useful when you want to construct linker command line
  options programmatically. For example, if you want to link `libfoo.so` by
  as-needed basis but don't want to change the global state of `--as-needed`,
  you can append `--push-state --as-needed -lfoo --pop-state` to the linker
  command line options.

* `--relax, --no-relax`:
  Rewrite machine instructions with more efficient ones for some relocations.
  The feature is enabled by default.

* `--require-defined`=_symbol_:
  Like `--undefined`, except the new symbol must be defined by the end of the
  link.

* `--retain-symbols-file`=_file_:
  Keep only symbols listed in _file_. _file_ is a text file containing a
  symbol name on each line. `mold` discards all local symbols as well as
  global symbol that are not in _file_. Note that this option removes symbols
  only from `.symtab` section and does not affect `.dynsym` section, which is
  used for dynamic linking.

* `--rpath`=_dir_:
  Add _dir_ to runtime search paths.

* `--section-start`=_section_=_address_:
  Set _address_ to section. _address_ is a hexadecimal number that may start
  with an optional `0x`.

* `--shared`, `--Bshareable`:
  Create a share library.

* `--spare-dynamic-tags`=_number_:
  Append the given number of `DT_NULL` entries to the end of the `.dynamic`
  section, so that post-link processing tools can easily add new dynamic tags
  by overwriting the null entries.

* `--start-lib`, `--end-lib`:
  Handle object files between `--start-lib` and `--end-lib` as if they were in
  an archive file. That means object files between them are linked only when
  they are needed to resolve undefined symbols. The options are useful if you
  want to link object files only when they are needed but want to avoid the
  overhead of running ar(3).

* `--static`:
  Do not link against shared libraries.

* `--sysroot`=_dir_:
  Set target system root directory to _dir_.

* `--trace`:
  Print name of each input file.

* `--undefined-glob`=_pattern_:
  Synonym for `--undefined`, except that `--undefined-glob` takes a glob
  pattern instead of just a single symbol name.

* `--undefined-version`, `--no-undefined-version`:
  By default, `mold` warns on a symbol specified by a version script or by
  `--export-dynamic-symbol` if it is not defined. You can silence the warning
  by `--undefined-version`.

* `--unique`=_pattern_:
  Don't merge input sections that match the given glob pattern _pattern_.

* `--unresolved-symbols`=[ `report-all` | `ignore-all` | `ignore-in-object-files` | `ignore-in-shared-libs` ]:
  How to handle undefined symbols.

* `--version-script`=_file_:
  Read version script from _file_. If _file_ does not exist in the current
  directory, it is searched from library search paths for the sake of
  compatibility with GNU ld.

* `--warn-common`, `--no-warn-common`:
  Warn about common symbols.

* `--warn-once`:
  Only warn once for each undefined symbol instead of warn for each relocation
  referring an undefined symbol.

* `--warn-unresolved-symbols`, `--error-unresolved-symbols`:
  Normally, the linker reports an error for unresolved symbols.
  `--warn-unresolved-symbols` option turns it into a warning.
  `--error-unresolved-symbols` option restores the default behavior.

* `--whole-archive`, `--no-whole-archive`:
  When archive files (`.a` files) are given to the linker, only object files
  that are needed to resolve undefined symbols are extracted from them and
  linked to an output file. `--whole-archive` changes that behavior for
  subsequent archives so that the linker extracts all object files and links
  them to an output. For example, if you are creating a shared object file and
  you want to include all archive members to the output, you should pass
  `--whole-archive`. `--no-whole-archive` restores the default behavior for
  subsequent archives.

* `--wrap`=_symbol_:
  Make _symbol_ be resolved to `__wrap_`_symbol_. The original symbol can be
  resolved as `__real_`_symbol_. This option is typically used for wrapping an
  existing function.

* `-z cet-report`=[ `warning` | `error` | `none` ]:
  Intel Control-flow Enforcement Technology (CET) is a new x86 feature
  available since Tiger Lake which is released in 2020. It defines new
  instructions to harden security to protect programs from control hijacking
  attacks. You can tell the compiler to use the feature by specifying the
  `-fcf-protection` flag.

  `-z cet-report` flag is used to make sure that all object files were
  compiled with a correct `-fcf-protection` flag. If `warning` or `error` are
  given, `mold` prints out a warning or an error message if an object file was
  not compiled with the compiler flag.

  `mold` looks for `GNU_PROPERTY_X86_FEATURE_1_IBT` bit and
  `GNU_PROPERTY_X86_FEATURE_1_SHSTK` bit in `.note.gnu.property` section to
  determine whether or not an object file was compiled with `-fcf-protection`.

* `-z now`, `-z lazy`:
  By default, functions referring to other ELF modules are resolved by the
  dynamic linker when they are called for the first time. `-z now` marks an
  executable or a shared library file so that all dynamic symbols are resolved
  when a file is loaded to memory. `-z lazy` restores the default behavior.

* `-z origin`:
  Mark object requiring immediate `$ORIGIN` processing at runtime.

* `-z ibt`:
  Turn on `GNU_PROPERTY_X86_FEATURE_1_IBT` bit in `.note.gnu.property` section
  to indicate that the output uses IBT-enabled PLT. This option implies `-z
  ibtplt`.

* `-z ibtplt`:
  Generate Intel Branch Tracking (IBT)-enabled PLT which is the default on
  x86-64. This is the default.

* `-z execstack`, `-z noexecstack`:
  By default, the pages for the stack area (i.e. the pages where local
  variables reside) are not executable for security reasons. `-z execstack`
  makes it executable. `-z noexecstack` restores the default behavior.

* `-z keep-text-section-prefix`, `-z nokeep-text-section-prefix`:
  Keep `.text.hot`, `.text.unknown`, `.text.unlikely`, `.text.startup`, and
  `.text.exit` as separate sections in the final binary instead of merging
  them as `.text`.

* `-z rodynamic`:
  Make the `.dynamic` section read-only.

* `-z relro`, `-z norelro`:
  Some sections such as `.dynamic` have to be writable only during a module is
  being loaded to memory. Once the dynamic linker finishes its job, such
  sections won't be mutated by anyone. As a security mitigation, it is
  preferred to make such segments read-only during program execution.

  `-z relro` puts such sections into a special segment called `relro`. The
  dynamic linker makes a relro segment read-only after it finishes its job.

  By default, `mold` generates a relro segment. `-z norelro` disables the
  feature.

* `-z sectionheader`, `-z nosectionheader`:
  `-z nosectionheader` tell the linker to omit the section header.
  By default, the linker does not omit the section header.

* `-z separate-loadable-segments`, `-z separate-code`, `-z noseparate-code`:
  If one memory page contains multiple segments, the page protection bits are
  set in such a way that the needed attributes (writable or executable) are
  satisfied for all segments. This usually happens at a boundary of two
  segments with two different attributes.

  `separate-loadable-segments` adds paddings between segments with different
  attributes so that they do not share the same page. This is the default.

  `separate-code` adds paddings only between executable and non-executable
  segments.

  `noseparate-code` does not add any paddings between segments.

* `-z defs`, `-z nodefs`:
  Report undefined symbols (even with `--shared`).

* `-z shstk`:
  Enforce shadow stack by turning `GNU_PROPERTY_X86_FEATURE_1_SHSTK` bit in
  `.note.gnu.property` output section. Shadow stack is part of Intel
  Control-flow Enforcement Technology (CET), which is available since Tiger
  Lake (2020).

* `-z start_stop_visibility`=[ `hidden` | `protected` ]:
  If a section name is valid as a C identifier (i.e., it matches
  `/^[_a-zA-Z][_a-zA-Z0-9]*$/`), mold creates `__start_SECNAME` and
  `__stop_SECNAME` symbols to mark the beginning and end of the section,
  where `SECNAME` is the section name.

  You can make these marker symbols visible from other ELF modules by passing
  `-z start_stop_visibility=protected`. Default is `hidden`.

* `-z text`, `-z notext`, `-z textoff`:
  `mold` by default reports an error if dynamic relocations are created in
  read-only sections. If `-z notext` or `-z textoff` are given, `mold` creates
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
  Mark DSO non-deletable at runtime.

* `-z nodlopen`:
  Mark DSO not available to dlopen(3). This option makes it possible for the
  linker to optimize thread-local variable accesses by rewriting instructions
  for some targets.

* `-z nodump`:
  Mark DSO not available to dldump(3).

* `-z nocopyreloc`:
  Do not create copy relocations.

* `-z initfirst`:
  Mark DSO to be initialized first at runtime.

* `-z interpose`:
  Mark object to interpose all DSOs but executable.

* `-(`, `-)`, `-EL`, `-O`_number_, `--allow-shlib-undefined`, `--dc`, `--dp`, `--end-group`, `--no-add-needed`, `--no-allow-shlib-undefined`, `--no-copy-dt-needed-entries`, `--nostdlib`, `--rpath-link=Ar dir`, `--sort-common`, `--sort-section`, `--start-group`, `--warn-constructors`, `--warn-once`, `--fix-cortex-a53-835769`, `--fix-cortex-a53-843419`, `-z combreloc`, `-z common-page-size`, `-z nocombreloc`:
  Ignored

## ENVIRONMENT VARIABLES

* `MOLD_JOBS`:
  If this variable is set to `1`, only one `mold` process will run at a time.
  If a new mold process is initiated while another is already active, the new
  process will wait until the active one completes before starting.

  The primary reason for this environment variable is to minimize peak memory
  usage. Since mold is designed to operate with high parallelism, running
  multiple mold instances simultaneously may not be beneficial. If you execute
  N instances of mold concurrently, it could require N times the time and N
  times the memory. On the other hand, running them one after the other might
  still take N times longer, but the peak memory usage would be the same as
  running just a single instance.

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

gold(1), ld(1), elf(5), ld.so(8)

## AUTHOR

Rui Ueyama <ruiu@cs.stanford.edu>

## BUGS

Report bugs to <https://github.com/rui314/mold/issues>.
