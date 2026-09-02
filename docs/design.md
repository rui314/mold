[This document was written in 2020, and the contents are outdated.
Specifically, we no longer believe that object preloading is a good
idea. That being said, most of the points in this document still hold
even today. Therefore, I'll keep this document as-is.]

## Design and implementation of mold

For the rest of this documentation, I'll explain the design and the
implementation of mold. If you are only interested in using mold, you
don't need to read the below.

### Motivation

Here is why I'm writing a new linker:

- Even though lld has significantly improved the situation, linking is
  still one of the slowest steps in a build. It is especially
  annoying when I changed one line of code and had to wait for a few
  seconds or even more for a linker to complete. It should be
  instantaneous. There's a need for a faster linker.

- The number of cores on a PC has increased a lot lately, and this
  trend is expected to continue. However, the existing linkers can't
  take the advantage of the trend because they don't scale well for more
  cores. I have a 64-core/128-thread machine, so my goal is to create
  a linker that uses the CPU nicely. mold should be much faster than
  other linkers on 4 or 8-core machines too, though.

- It looks to me that the designs of the existing linkers are somewhat
  too similar, and I believe there are a lot of drastically different
  designs that haven't been explored yet. Developers generally don't
  care about linkers as long as they work correctly, and they don't
  even think about creating a new one. So there may be lots of low
  hanging fruits there in this area.

### Basic design

- In order to achieve a `cp`-like performance, the most important
  thing is to fix the layout of an output file as quickly as possible, so
  that we can start copying actual data from input object files to an
  output file as soon as possible.

- Copying data from input files to an output file is I/O-bounded, so
  there should be room for doing computationally-intensive tasks while
  copying data from one file to another.

- We should allow the linker to preload object files from disk and
  parse them in memory before a complete set of input object files
  is ready. To do so, we need
  to split the linker into two in such a way that the latter half of
  the process finishes as quickly as possible by speculatively parsing
  and preprocessing input files in the first half of the process.

- One of the most computationally-intensive stage among linker stages
  is symbol resolution. To resolve symbols, we basically have to throw
  all symbol strings into a hash table to match undefined symbols with
  defined symbols. But this can be done in the preloading stage using
  [string interning](https://en.wikipedia.org/wiki/String_interning).

- Object files may contain a special section called a mergeable string
  section. The section contains lots of null-terminated strings, and
  the linker is expected to gather all mergeable string sections and
  merge their contents. So, if two object files contain the same
  string literal, for example, the resulting output will contain a
  single merged string. This step is computationally intensive, but string
  merging can be done in the preloading stage using string interning.

- Static archives (.a files) contain object files, but the static
  archive's string table contains only defined symbols of member
  object files and lacks other types of symbols. That makes static
  archives unsuitable for speculative parsing. Therefore, the linker
  should ignore the symbol table of static archive and directly read
  static archive members.

- If there's a relocation that uses a GOT of a symbol, then we have to
  create a GOT entry for that symbol. Otherwise, we shouldn't. That
  means we need to scan all relocation tables to fix the length and
  the contents of a .got section. This is computationally intensive,
  but this step is parallelizable.

### Linker Script

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
language. But at the same time, we want to satisfy the user needs that
are currently satisfied with the linker script language. So, what
should we do? Here is my observation:

- Linker script allows doing a lot of tricky stuff, such as specifying
  the exact layout of a file, inserting arbitrary bytes between
  sections, etc. But most of them can be done with a post-link binary
  editing tool (such as `objcopy`).

- It looks like there are two things that truly cannot be done by a
  post-link editing tool: (a) mapping input sections to output
  sections, and (b) applying relocations.

From the above observation, I believe we need to provide only the
following features instead of the entire linker script language:

- A method to specify how input sections are mapped to output
  sections, and

- a method to set addresses to output sections, so that relocations
  are applied based on desired addresses.

I believe everything else can be done with a post-link binary editing
tool.

### Details

- As we aim to the 1-second goal for Chromium, every millisecond
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
  If you already have a large file in the buffer cache, writing to it is
  much faster than creating a new fresh file and writing to it.
  Based on this observation, mold overwrites to an existing
  executable file if exists. My quick benchmark showed that I could
  save 300 milliseconds when creating a 2 GiB output file.
  Linux doesn't allow to open an executable for writing if it is
  running (you'll get a "text busy" error if you attempt). mold
  falls back to the usual way if it fails to open an output file.

- The output from the linker should be deterministic for the sake of
  [build reproducibility](https://en.wikipedia.org/wiki/Reproducible_builds)
  and ease of debugging. This might add a little bit of overhead to
  the linker, but that shouldn't be too much.

- A .build-id, a unique ID embedded to an output file, is usually
  computed by applying a cryptographic hash function (e.g. SHA-1) to
  an output file. This is a slow step, but we can speed it up by
  splitting a file into small chunks, computing SHA-1 for each chunk,
  and then computing SHA-1 of the concatenated SHA-1 hashes
  (i.e. constructing a [Merkle
  Tree](https://en.wikipedia.org/wiki/Merkle_tree) of height 2).
  Modern x86 processors have purpose-built instructions for SHA-1 and
  can compute SHA-1 pretty quickly at about 2 GiB/s. Using 16
  cores, a build-id for a 2 GiB executable can be computed in 60 to 70
  milliseconds.

- BFD, gold, and lld support section garbage collection. That is, a
  linker runs a mark-sweep garbage collection on an input graph, where
  sections are vertices and relocations are edges, to discard all
  sections that are not reachable from the entry point symbol
  (i.e. `_start`) or a few other root sections. In mold, we are using
  multiple threads to mark sections concurrently.

- Similarly, BFD, gold and lld support Identical Comdat Folding (ICF)
  as yet another size optimization. ICF merges two or more read-only
  sections that happen to have the same contents and relocations.
  To do that, we have to find isomorphic subgraphs from larger graphs.
  I implemented a new algorithm for mold, which is 5x faster than lld
  to do ICF for Chromium (from 5 seconds to 1 second).

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

### Size of the problem

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

### Internals

In this section, I'll explain the internals of mold linker.

#### A brief history of Unix and the Unix linker

Conceptually, what a linker does is pretty simple. A compiler compiles
a fragment of a program (a single source file) into a fragment of
machine code and data (an object file, which typically has the .o
extension), and a linker stitches them together into a single
executable or a shared library image.

In reality, modern linkers for Unix-like systems are much more
complicated than the naive understanding because they have gradually
gained one feature at a time over the 50 years history of Unix, and
they are now something like a bag of lots of miscellaneous features in
which none of the features is more important than the others. It is
very easy to miss the forest for the trees, since for those who don't
know the details of the Unix linker, it is not clear which feature is
essential and which is not.

That being said, one thing is clear that at any point of Unix history,
a Unix linker has a coherent feature set for the Unix of that age. So,
let me entangle the history to see how the operating system, runtime,
and linker have gained features that we see today. That should give
you an idea of why a particular feature has been added to a linker in the
first place.

1. Original Unix didn't support shared libraries, and a program was
   always loaded to a fixed address. An executable was something like
   a memory dump that was just loaded to a particular address by the
   kernel. After loading, the kernel started executing the program by
   setting the instruction pointer to a particular address.

   The most essential feature for any linker is relocation processing.
   The original Unix linker of course supported that. Let me explain
   what that is.

   Individual object files are inevitably incomplete as a program,
   because when a compiler created them, it only see a part of an
   entire program. For example, if an object file contains a function
   call that refers to another object file, the `call` instruction in the
   object cannot be complete, as the compiler has no idea as to what
   is the called function's address. To deal with this, the compiler
   emits a placeholder value (typically just zero) instead of a real
   address and leaves metadata in an object file saying "fix offset X
   of this file with an address of Y". That metadata is called
   "relocation". Relocations are typically processed by the linker.

   It is easy for a linker to apply relocations for the original Unix
   because a program is always loaded to a fixed address. It exactly
   knows the addresses of all functions and data when linking a
   program.

   Static library support, which is still an important feature of Unix
   linker, also dates back to this early period of Unix history.
   To understand what it is, imagine that you are trying to compile
   a program for the early Unix. You don't want to waste time to
   compile libc functions every time you compile your program (the
   computers of the era were incredibly slow), so you have already
   placed each libc function into a separate source file and compiled
   them individually. That means you have object files for each libc
   function, e.g., printf.o, scanf.o, atoi.o, write.o, etc.

   Given this configuration, all you have to do to link your program
   against libc functions is to pick up the right set of libc object
   files and give them to the linker along with the object files of your
   program. But, keeping the linker command line in sync with the
   libc functions you are using in your program is bothersome. You can
   be conservative; you can specify all libc object files to the
   command line, but that leads to program bloat because the linker
   unconditionally link all object files given to it no matter whether
   they are used or not. So, a new feature was added to the linker to
   fix the problem. That is the static library, which is also called
   the archive file.

   An archive file is just a bundle of object files, just like zip
   file but in an uncompressed form. An archive file typically has the
   .a file extension and named after its contents. For example, the
   archive file containing all libc objects is named `libc.a`.

   If you pass an archive file along with other object files to the
   linker, the linker pulls out an object file from the archive _only
   when_ it is referenced by other object files. In other words,
   unlike object files directly given to a linker, object files
   wrapped in an archive are not linked to the output by default.
   An archive works as a supplement to complete your program.

   Even today, you can still find a libc archive file. Run `ar t
   /usr/lib/x86_64-linux-gnu/libc.a` on Linux should give you a list
   of object files in the libc archive.

2. In the '80s, Sun Microsystems, a leading commercial Unix vendor at the
   time, added shared library support to their Unix variant, SunOS.

(This section is incomplete.)

### Concurrency strategy

In this section, I'll explain the high-level concurrency strategy of
mold.

In most places, mold adopts data parallelism. That is, we have a huge
number of pieces of data of the same kind, and we process each of them
individually using parallel for-loop. For example, after identifying
the exact set of input object files, we need to scan all relocation
tables to determine the sizes of .got and .plt sections. We do that
using a parallel for-loop. The granularity of parallel processing in
this case is the relocation table.

Data parallelism is very efficient and scalable because there's no
need for threads to communicate with each other while working on each
element of data. In addition to that, data parallelism is easy to
understand, as it is just a for-loop in which multiple iterations may
be executed in parallel. We don't use high-level communication or
synchronization mechanisms such as channels, futures, promises,
latches or something like that in mold.

In some cases, we need to share a little bit of data between threads
while executing a parallel for-loop. For example, the loop to scan
relocations turns on "requires GOT" or "requires PLT" flags in a
symbol. Symbol is a shared resource, and writing to them from multiple
threads without synchronization is unsafe. To deal with it, we made
the flag an atomic variable.

The other common pattern you can find in mold which is build on top of
the parallel for-loop is the map-reduce pattern. That is, we run a
parallel for-loop on a large data set to produce a small data set and
process the small data set with a single thread. Let me take a
build-id computation as an example. Build-id is typically computed by
applying a cryptographic hash function such as SHA-1 on a linker's
output file. To compute it, we first consider an output as a sequence
of 1 MiB blocks and compute a SHA-1 hash for each block in parallel.
Then, we concatenate the SHA-1 hashes and compute a SHA-1 hash on the
hashes to get a final build-id.

Finally, we use concurrent hashmap at a few places in mold. Concurrent
hashmap is a hashmap to which multiple threads can safely insert items
in parallel. We use it in the symbol resolution stage, for example.
To resolve symbols, we basically have to throw in all defined symbols
into a hash table, so that we can find a matching defined symbol for
an undefined symbol by name. We do the hash table insertion from a
parallel for-loop which iterates over a list of input files.

Overall, even though mold is highly scalable, it succeeded to avoid
complexties you often find in complex parallel programs. From high
level, mold just serially executes the linker's internal passes one by
one. Each pass is parallelized using parallel for-loops.

### Rejected ideas

In this section, I'll explain the alternative designs I currently do
not plan to implement and why I turned them down.

- Placing variable-length sections at end of an output file and start
  copying file contents before fixing the output file layout

  Idea: Fixing the layout of regular sections seems easy, and if we
  place them at beginning of a file, we can start copying their
  contents from their input files to an output file. While copying
  file contents, we can compute the sizes of variable-length sections
  such as .got or .plt and place them at end of the file.

  Reason for rejection: I did not choose this design because I doubt
  if it could actually shorten link time and I think I don't need it
  anyway.

  The linker has to de-duplicate comdat sections (i.e. inline
  functions that are included in multiple object files), so we
  cannot compute the layout of regular sections until we resolve all
  symbols and de-duplicate comdats. That takes a few hundred
  milliseconds. After that, we can compute the sizes of
  variable-length sections in less than 100 milliseconds. It's quite
  fast, so it doesn't seem to make much sense to proceed without
  fixing the final file layout.

  The other reason to reject this idea is because there's good a
  chance for this idea to have a negative impact on the linker's overall
  performance. If we copy file contents before fixing the layout, we
  can't apply relocations to them while copying because symbol
  addresses are not available yet. If we fix the file layout first, we
  can apply relocations while copying, which is effectively zero-cost
  due to a very good data locality. On the other hand, if we apply
  relocations long after we copy file contents, it's pretty expensive
  because section contents are very likely to have been evicted from
  CPU cache.

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
  function. The same is true for adding malloc to your program. Making
  a local change doesn't necessarily result in a local change in the
  binary level.  It can easily have cascading effects.

  Some ELF fancy features make incremental linking even harder to
  implement. Take the weak symbol as an example. If you define `atoi`
  as a weak symbol in your program, and if you are not using `atoi`
  at all in your program, that symbol will be resolved to address
  0. But if you start using some libc function that indirectly calls
  `atoi`, then `atoi` will be included in your program, and your weak
  symbol will be resolved to that function. I don't know how to
  efficiently fix up a binary for this case.

  This is a hard problem, so existing linkers don't try too hard to
  solve it. For example, IIRC, gold falls back to full link if any
  function is removed from a previous build. If you want to not annoy
  users in the fallback case, you need to make full link fast anyway.

  Second, incremental linking itself has an overhead. It has to detect
  updated files, patch an existing output file and write additional
  data to an output file for future incremental linking. GNU gold, for
  instance, takes almost 30 seconds on my machine to do a null
  incremental link (i.e. no object files are updated from a previous
  build) for chrome. It's just too slow.

  Third, there are other practical issues in incremental linking. It's
  not reproducible, so your binary isn't going to be the same as other
  binaries even if you are compiling the same source tree using the
  same compiler toolchain. Or, it is complex and there might be a bug
  in it. If something doesn't work correctly, "remove --incremental
  from your Makefile and try again" could be a piece of advice, but
  that isn't ideal.

  So, all in all, incremental linking is tricky. I wanted to make full
  link as fast as possible, so that we don't have to think about how
  to work around the slowness of full link.

- Defining a completely new file format and use it

  Idea: Sometimes, the ELF file format itself seems to be a limiting
  factor in improving the linker's performance. We might be able to make a
  far better one if we create a new file format.

  Reason for rejection: I rejected the idea because it apparently has
  a practical issue (backward compatibility issue) and also doesn't
  seem to improve the performance of linkers that much. As clearly
  demonstrated by mold, we can create a fast linker for ELF. I believe
  ELF isn't that bad, after all. The semantics of the existing Unix
  linkers, such as the name resolution algorithm or the linker script,
  have slowed the linkers down, but that's not a problem of the file
  format itself.

- Watching object files using inotify(2)

  Idea: When mold is running as a daemon for preloading, use
  inotify(2) to watch file system updates so that it can reload files
  as soon as they are updated.

  Reason for rejection: Just like the maximum number of files you can
  simultaneously open, the maximum number of files you can watch using
  inotify(2) isn't that large. Maybe just a single instance of mold is
  fine with inotify(2), but it may fail if you run multiple of it.

  The other reason for not doing it is because mold is quite fast
  without it anyway. Invoking stat(2) on each file for file update
  check takes less than 100 milliseconds for Chrome, and if most of
  the input files are not updated, parsing updated files takes almost
  no time.
