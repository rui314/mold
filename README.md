# mold: A Modern Linker

![mold image](mold.jpg)

This is a repository of a linker I'm currently developing as a
replacement for existing Unix linkers such as GNU BFD, GNU gold or
LLVM lld.

My goal was to make a linker that is as fast as concatenating input
object files with `cat` command. It may sound like an impossible goal,
but it's not entirely impossible because of the following two reasons:

1. `cat` is a simple single-threaded program which isn't the fastest
   one as a file copy command. My linker can use multiple threads to
   copy file contents more efficiently to save time to do extra work.

2. Copying file contents is I/O-bounded, and many CPU cores should be
   available during file copy. We can use them to do extra work while
   copying file contents.

Concretely speaking, I wanted to use the linker to link a Chromium
executable with full debug info (~2 GiB in size) just in 1 second.
LLVM's lld, the fastest open-source linker which I originally created
a few years ago, takes about 12 seconds to link Chromium on my machine.
So the goal is 12x performance bump over lld. Compared to GNU gold,
it's more than 50x.

It looks like mold has achieved the goal. It can link Chromium in 2
seconds with 8-cores/16-threads, and if I enable the preloading
feature (I'll explain it later), the latency of the linker for an
interactive use is less than 900 milliseconds. It is actualy faster
than `cat`.

Note that even though mold can create a runnable Chrome executable,
it is far from complete and not usable for production. mold is still
just a toy linker, and this is still just my pet project.

## Background

- Even though lld has significantly improved the situation, linking is
  still one of the slowest steps in a build. It is especially
  annoying when I changed one line of code and had to wait for a few
  seconds or even more for a linker to complete. It should be
  instantaneous. There's a need for a faster linker.

- The number of cores on a PC has increased a lot lately, and this
  trend is expected to continue. However, the existing linkers can't
  take the advantage of that because they don't scale well for more
  cores. I have a 64-core/128-thread machine, so my goal is to create
  a linker that uses the CPU nicely. mold should be much faster than
  other linkers on 4 or 8-core machines too, though.

- It looks to me that the designs of the existing linkers are somewhat
  similar, and I believe there are a lot of drastically different
  designs that haven't been explored yet. Develoeprs generally don't
  care about linkers as long as they work correctly, and they don't
  even think about creating a new one. So there may be lots of low
  hanging fruits there in this area.

## Basic design

- In order to achieve a `cat`-like performance, the most important
  thing is to fix the layout of an output file as quickly as possible, so
  that we can start copying actual data from input object files to an
  output file as soon as possible.

- Copying data from input files to an output file is I/O-bounded, so
  there should be room for doing computationally-intensive tasks while
  copying data from one file to another.

- We should allow the linker to preload object files from disk and
  parse them in memory before a complete set of input object files
  is ready. My idea is this: if a user invokes the linker with
  `--preload` flag along with other command line flags a few seconds
  before the actual linker invocation, then the following actual
  linker invocation with the same command line options (except
  `--preload` flag) becomes magically faster. Behind the scenes, the
  linker starts preloading object files on the first invocation and
  becomes a daemon. The second invocation of the linker notifies the
  daemon to reload updated object files and then proceed.

- Daemonizing alone wouldn't make the linker magically faster. We need
  to split the linker into two in such a way that the latter half of
  the process finishes as quickly as possible by speculatively parsing
  and preprocessing input files in the first half of the process. The
  key factor of success would be to design nice data structures that
  allows us to offload as much processing as possible from the second
  to the first half.

- One of the most time-consuming stage among linker stages is symbol
  resolution. To resolve symbols, we basically have to throw all
  symbol strings into a hash table to match undefined symbols with
  defined symbols. But this can be done in the daemon using [string
  interning](https://en.wikipedia.org/wiki/String_interning).

- Object files may contain a special section called a mergeable string
  section. The section contains lots of null-terminated strings, and
  the linker is expected to gather all mergeable string sections and
  merge their contents. So, if two object files contain the same
  string literal, for example, the resulting output will contain a
  single merged string. This step is time-consuming, but string
  merging can be done in the daemon using string interning.

- Static archives (.a files) contain object files, but the static
  archive's string table contains only defined symbols of member
  object files and lacks other types of symbols. That makes static
  archives unsuitable for speculative parsing. The daemon should
  ignore the string table of static archive and directly read all
  member object files of all archives to get the whole picture of
  all possible input files.

- If there's a relocation that uses a GOT of a symbol, then we have to
  create a GOT entry for that symbol. Otherwise, we shouldn't. That
  means we need to scan all relocation tables to fix the length and
  the contents of a .got section. This is perhaps time-consuming, but
  this step is parallelizable.

## Compatibility

- GNU ld, GNU gold and LLVM lld support essentially the same set of
  command line options and features. mold doesn't have to be
  completely compatible with them. As long as it can be used for
  linking large user-land programs, I'm fine with that. It is OK to
  leave some command line options unimplemented; if mold is blazingly
  fast, other projects would still be happy to adopt it by modifying
  their projects' build files.

- mold emits Linux executables and runs only on Linux. I won't avoid
  Unix-ism when writing code (e.g. I'll probably use fork(2)).
  I don't want to think about portability until mold becomes a thing
  that's worth to be ported.

## Linker Script

Linker script is an embedded language for the linker. It is mainly
used to control how input sections are mapped to output sections and
the layout of the output, but it can also do a lot of tricky stuff.
Its feature is useful especially for embedded programming, but it's
also an awfully underdocumented and complex language.

We have to implement a subset of the linker script language anwyay,
because on Linux, /usr/lib/x86_64-linux-gnu/libc.so is (despite its
name) not a shared object file but actually an ASCII file containing
linker script code to load the _actual_ libc.so file. But the feature
set for this purpose is very limited, and it is okay to implement them
to mold.

Besides that, we really don't want to implement the linker script
langauge. But at the same time, we want to satisfy the user needs that
are currently satisfied with the linker script langauge. So, what
should we do? Here is my observation:

- Linker script allows to do a lot of tricky stuff, such as specifying
  the exact layout of a file, inserting arbitrary bytes between
  sections, etc. But most of them can be done with a post-link binary
  editing tool (such as `objcopy`).

- It looks like there are two things that truely cannot be done by a
  post-link editing tool: (a) mapping input sections to output
  sections, and (b) applying relocations.

From the above observation, I believe we need to provide only the
following features instead of the entire linker script langauge:

- A method to specify how input sections are mapped to output
  sections, and

- a method to set addresses to output sections, so that relocations
  are applied based on desired adddresses.

I believe everything else can be done with a post-link binary editing
tool.

## Details

- If we aim to the 1 second goal for Chromium, every millisecond
  counts. We can't ignore the latency of process exit. If we mmap a
  lot of files, \_exit(2) is not instantaneous but takes a few hundred
  milliseconds because the kernel has to clean up a lot of
  resources. As a workaround, we should organize the linker command as
  two processes; the first process forks the second process, and the
  second process does the actual work. As soon as the second process
  writes a result file to a filesystem, it notifies the first process,
  and the first process exits. The second process can take time to
  exit, because it is not an interactive process.

- At least on Linux, it looks like the filesystem's performance to
  allocate new blocks to a new file is the limiting factor when
  creating a new large file and filling its contents using mmap.
  If you already have a large file on a filesystem, writing to it is
  much faster than creating a new fresh file and writing to it.
  Based on this observation, mold should overwrite to an existing
  executable file if exists. My quick benchmark showed that I could
  save 300 milliseconds when creating a 2 GiB output file.

  Linux doesn't allow to open an executable for writing if it is
  running (you'll get "text busy" error if you attempt). mold should
  fall back to the usual way if it fails to open an output file.

- The output from the linker should be deterministic for the sake of
  [build reproducibility](https://en.wikipedia.org/wiki/Reproducible_builds)
  and ease of debugging. This might add a little bit of overhead to
  the linker, but that shouldn't be too much.

- A .build-id, a unique ID embedded to an output file, is usually
  computed by applying a cryptographic hash function (e.g. SHA-1) to
  an output file. This is a slow step, but we can speed it up by
  splitting a file into small chunks, computing SHA-1 for each chunk,
  and then computing SHA-1 of the concatenated SHA-1 hashes
  (i.e. constructing a [Markle
  Tree](https://en.wikipedia.org/wiki/Merkle_tree) of height 2).
  Modern x86 processors have purpose-built instructions for SHA-1 and
  can compute SHA-1 pretty quickly at about 2 GiB/s rate. Using 16
  cores, a build-id for a 2 GiB executable can be computed in 60 to 70
  milliseconds.

- [Intel Threading Building
  Blocks](https://github.com/oneapi-src/oneTBB) (TBB) is a good
  library for parallel execution and has several concurrent
  containers. We are particularly interested in using
  `parallel_for_each` and `concurrent_hash_map`.

- TBB provides `tbbmalloc` which works better for multi-threaded
  applications than the glib'c malloc, but it looks like
  [jemalloc](https://github.com/jemalloc/jemalloc) and
  [mimalloc](https://github.com/microsoft/mimalloc) are a little bit
  more scalable than `tbbmalloc`.

## Size of the problem

When linking Chrome, a linker reads 3,430,966,844 bytes of data in
total. The data contains the following items:

| Data item                | Number
| ------------------------ | ------
| Object files             | 30,723
| Public undefined symbols | 1,428,149
| Mergeable strings        | 1,579,996
| Comdat groups            | 9,914,510
| Regular sections¹        | 10,345,314
| Public defined symbols   | 10,512,135
| Symbols                  | 23,953,607
| Sections                 | 27,543,225
| Relocations against SHF_ALLOC sections | 39,496,375
| Relocations              | 62,024,719

¹ Sections that have to be copied from input object files to an
output file. Sections that contain relocations or symbols are for
example excluded.

## Rejected ideas

In this section, I'll explain the alternative designs I currently do
not plan to implement and why I turned them down.

- Placing variable-length sections at end of an output file and start
  copying file contents before fixing the output file layout

  Idea: fixing the layout of regular sections seems easy, and if we
  place them at beginning of a file, we can start copying their
  contents from their input files to an output file. While copying
  file contents, we can compute the sizes of variable-length sections
  such as .got or .plt.

  Reason for rejection: I did not choose this design because I doubt
  if it could actually shorten link time and I think I don't need it
  anyway.

  The linker has to de-duplicate comdat sections (i.e. inline
  functions that are included into multiple object files), so we
  cannot compute the layout of regular sections until we resolve all
  symbols and de-duplicate comdats. That takes a few hundred
  milliseconds. After that, we can compute the sizes of
  variable-length sections in less than 100 milliseconds. It's quite
  fast, so it doesn't seem to make much sense to proceed without
  fixing the final file layout.

- Incremental linking

  Idea: Incremental linking is a technique to patch a previous linker's
  output file so that only functions or data that are updated from the
  previous build are written to it. It is expected to significantly
  reduce the amount of data copied from input files to an output file
  and thus speed up linking. GNU BFD and gold linkers support it.

  Reason for rejection: I turned it down because it (1) is
  complicated, (2) doesn't seem to speed it up that much and (3) has
  several practical issues. Let me explain each of them.

  First, incremental linking for real C/C++ programs is not as easy as
  one might think. Let me take malloc as an example. malloc is usually
  defined by libc, but you can implement it in your program, and if
  that's the case, the symbol `malloc` will be resolved to your
  function instead of the one in libc. If you include a library that
  defines malloc (such as libjemalloc or libtbbmallc) before libc,
  their malloc will override libc's malloc.

  Assume that you are using a nonstandard malloc. What if you remove
  your malloc from your code, or remove `-ljemalloc` from your
  Makefile? The linker has to include a malloc from libc, which may
  include more object files to satisfy its dependencies. Such code
  change can affect the entire program rather than just replacing one
  function. That's not just removing one function from libc. The same
  is true to adding malloc to your program. Making a local change
  doesn't necessarily result in a local change in the binary level.
  It can easily have cascading effects.

  Some ELF fancy features make incremental linking harder to implement.
  Take the weak symbol as an example. If you define `atoi` as an weak
  symbol in your program, and if you are not using `atoi` at all in
  your program, that symbol will be resolved to address 0. But if you
  start using some libc function that indirectly calls `atoi`, then
  `atoi` will be included to your program, and your weak symbol will
  be resolved to that function. I don't know how to efficiently fix up
  a binary for this case.

  This is a hard problem, so existing linkers don't try too hard to
  solve it. For example, IIRC, gold falls back to full link if any
  function is removed from a previous build. If you want to not annoy
  users in the fallback case, you need to make full link fast anyway.

  Second, incremental linking itself has an overhead. It has to detect
  updated sections, patch an existing output file and write additional
  data to an output file for future incremental linking. GNU gold, for
  instance, takes almost 30 seconds on my machine to do a null
  incremental link (i.e. no object files are updated from a previous
  build) for chrome. It's just too slow.

  Third, there are other practical issues in incremental linking. It's
  not reproducible, so your binary isn't the same as other binaries
  even if you are compiling the same source tree using the same
  compiler toolchain. Or, it is complex and there might be a bug in
  it. If something doesn't work correctly, "remove --incremental from
  your Makefile and try again" could be a piece of advise, but that
  isn't ideal.

  So, all in all, incremental linking is tricky. I wanted to make the
  full link as fast as possible, so that we don't have to think about
  how to workaround the slowness of the full link.

- Defining a completely new file format and use it

  Idea: Sometimes, the ELF file format itself seems to be a limiting
  factor of improving linker's performance. We might be able to make a
  far better one if we create a new file format.

  Reason for rejection: I rejected the idea because it apparently has
  a practical issue (backward compatibility issue) and also doesn't
  seem to improve performance of linkers that much. As clearly
  demonstrated by mold, we can create a fast linker for ELF. I believe
  ELF isn't that bad, after all. The semantics of the existing Unix
  linkers, such as the name resolution algorithm or the linker script,
  have slowed the linkers down, but that's not a problem of the file
  format itself.

- Watching object files using inotify(2)

  Idea: When mold is running as a daemon for preloading, use
  inotify(2) to watch file system updates using so that it can reload
  files as soon as they are updated.

  Reason for rejection: Just like the maximum number of files you can
  simultaneously open, the maximum number of files you can watch using
  inotify(2) isn't that large. Maybe just a single instance of mold
  would work fine with inotify(2), but it may fail if you run multiple
  of it.

  Other reason for not doing it is because mold is quite fast without
  it anyway. Invoking stat(2) on each file for file update check takes
  less than 100 milliseconds for Chrome, and if most of the input
  files are not updated, parsing updated files takes almost no time.
