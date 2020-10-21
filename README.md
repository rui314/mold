# mold: A Modern Linker

![mold image](mold.jpg)

This is a repository of a linker I'm currently developing as an
independent project for my Masters degree.

My goal is to make a linker that is almost as fast as concatenating
object files with `cat` command. Concretely speaking, I want to use the
linker to link a Chromium executable (about 1.8 GiB in size) just in 2
seconds. LLVM's lld, the fastest open-source linker which I originally
created a few years ago takes about 12 seconds to link Chromium on my
machine. So the goal is 6x performance bump over lld. I don't know if
I can ever achieve that, but it's worth a try. I need to create
something anyway to earn units to graduate, and I want to (at least
try to) create something useful.

I have quite a few new ideas as to how to achieve that speedup, though
they are still just random unproved thoughts which need to be
implemented and tested with benchmarks. Here is a brain dump:

- In order to achieve a `cat`-like performance, the most important
  thing is to fix the layout of an output file as quickly as possible, so
  that we can start copying actual data from input object files to an
  output executable/shared library file.

- The number of cores on a PC has increased a lot lately, and this
  trend is expected to continue. We'll see many more cores on our
  development machines. We should design the linker in such a way that
  it scales well with many more cores. I have a 64-core/128-thread
  machine, so my goal is to use that CPU nicely.

- Copying data from input files to an output file is I/O-bounded, so
  there should be room for doing computationally-intensive tasks while
  copying data from one file to another.

- We should focus on shortening the link time for the usual
  modify-build-debug development cycle because the linker speed
  matters most in that situation. It is OK to take a little bit more
  time for cold build because for cold build, the time taken by the
  compiler is a dominant factor anyway.

- After the first invocation of the linker, the linker should not exit
  but instead become a daemon to keep parsed input files in memory.
  The daemonized linker keeps an eye on the build directories using
  [inotify(2)](https://en.wikipedia.org/wiki/Inotify), and as soon as
  a new file is created or an exiting file is updated, it reloads a
  file to memory.

- Daemonizing alone wouldn't make the linker magically faster. We need
  to split the linker into two in such a way that the latter half of
  the process finishes as quickly as possible by speculatively parsing
  and preprocessing input files in the first half of the process. The
  key factor of success would be to design nice data structures to
  pass data between the first half and the second half that allows us
  to offload as much processing as possible from the second to the
  first half.

- One of the most time-consuming stage among linker stages is symbol
  resolution. To resolve symbols, we basically have to throw all
  symbol strings into a hash table to match undefined symbols with
  defined symbols. But this can be done in the daemon rather than
  after the actual command line is specified using [string
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
  we can do that while copying data from input files to an output
  file. After the data copy is done, we can attach a .got section at
  the end of the output file.

- If we aim to the 2 seconds goal for Chromium, every millisecond
  counts. We can't ignore the latency of process exit. If we mmap a
  lot of files, \_exit(2) is not instantaneous but takes a few hundred
  milliseconds because the kernel has to clean up a lot of
  resources. As a workaround, we should organize the linker command as
  two processes; the first process forks the second process, and the
  second process does the actual work. As soon as the second process
  writes a result file to a filesystem, it notifies the first process,
  and the first process exits. The second process can take time to
  exit, because it is not an interactive process.

- GNU ld, GNU gold and LLVM lld support essentially the same set of
  command line options and features. mold doesn't have to be
  completely compatible with them. As long as it can be used for
  linking large user-land programs, I'm fine with that. It is OK to
  leave some command line options unimplemented; if mold is blazingly
  fast, other project would still be happy to adopt it by modifying
  their projects' build files.

- I don't want to support the linker script language in mold because
  it's so complicated and inevitably slows down the linker. User-land
  programs rarely use linker scripts, so it shouldn't be a roadblock
  for most projects.

- [Intel Threading Building
  Blocks](https://github.com/oneapi-src/oneTBB) (TBB) is a good
  library for parallel execution and has several concurrent
  containers. We are particularly interested in using
  `parallel_for_each` and `concurrent_hash_map`.

- The output from the linker should be deterministic for the sake of
  [build reproducibility](https://en.wikipedia.org/wiki/Reproducible_builds).
  This might add a little bit of overhead to the linker, but that
  shouldn't be too much.
