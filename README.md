# mold: A Modern Linker

mold is a high-performance drop-in replacement for existing Unix linkers,
designed to speed up builds. In our August 2026 benchmarks, it links 4.9x
faster than [LLVM lld](https://lld.llvm.org) and 1.9x faster than
[wild](https://github.com/wild-linker/wild) at the median; see
[Benchmark](#benchmark) for the full results.

mold is written by the original developer of LLVM lld, the linker that
Android, Chrome, FreeBSD, PlayStation, Nintendo Switch, and other production
systems are built with. mold started as an effort to build an even faster
linker from scratch, free of the architectural limits its author had run into
while optimizing lld. It has been in production use since 2021, and today it
is the default linker of many large open-source projects and is used
internally by many companies.

mold supports x86-64, i386, ARM 32/64, RISC-V 32/64, PowerPC 32/64, s390x,
LoongArch 32/64, SPARC64, m68k, and SH-4.

## Why does linking speed matter?

If you are using a compiled language such as C, C++, or Rust, a build consists
of two phases. In the first phase, a compiler compiles source files into
object files (`.o` files). In the second phase, a linker takes all object
files and combines them into a single executable or shared library file.

The second phase can be time-consuming if your build output is large. mold can
speed up this process, saving you time and preventing distractions while
waiting for a lengthy build to finish. The difference is most noticeable
during rapid debug-edit-rebuild cycles.

## Benchmark

Here is a performance comparison of lld, wild, and mold when linking nine
large programs on two machines:

- AMD Ryzen Threadripper 7980X (64 cores) running Ubuntu 24.04
- Apple M1 Ultra (16 performance cores and 4 efficiency cores) running Fedora
  Asahi Remix 42; the benchmark is restricted to the performance cores

The Threadripper represents many-core workstation and server processors, and
the M1 Ultra represents high-performance desktop processors.

All three linkers were built from source in release configuration as of
2026-08-28 and ran with their default options. Times are wall-clock times as
seen by the caller, the median of three runs after a warm-up. The ARM64 rows
are the ARM64 builds of the same programs. The benchmark suite, including all
linker inputs, is available on [Zenodo](https://zenodo.org/records/21882261).

**AMD Ryzen Threadripper 7980X, debug builds**

| Program (output size)             | lld    | wild  | mold  | wild/mold | lld/mold
|-----------------------------------|--------|-------|-------|-----------|---------
| Blender 5.2 (2.46 GiB)            | 4.72s  | 1.98s | 0.86s | 2.3x      | 5.5x
| Chromium 145 (4.51 GiB)           | 16.64s | 3.98s | 1.65s | 2.4x      | 10.1x
| Chromium 145 ARM64 (4.76 GiB)     | 20.91s | N/A   | 1.83s | N/A       | 11.4x
| Clang 21 (4.19 GiB)               | 6.19s  | 3.71s | 1.34s | 2.8x      | 4.6x
| ClickHouse 26.1 (5.58 GiB)        | 6.62s  | 4.40s | 0.98s | 4.5x      | 6.7x
| Firefox 149 (2.37 GiB)            | 5.11s  | N/A   | 0.78s | N/A       | 6.5x
| Firefox 149 ARM64 (2.43 GiB)      | 6.40s  | N/A   | 0.93s | N/A       | 6.9x
| Godot 4.6 (1.01 GiB)              | 1.77s  | 1.08s | 0.46s | 2.3x      | 3.8x
| LibreOffice 26.2 (0.98 GiB)       | 3.46s  | 1.41s | 0.44s | 3.2x      | 7.9x
| PyTorch 2.9 (3.51 GiB)            | 4.35s  | 2.48s | 0.80s | 3.1x      | 5.4x
| TensorFlow 2.21 (9.55 GiB)        | 50.73s | N/A   | 3.15s | N/A       | 16.1x

**AMD Ryzen Threadripper 7980X, release builds**

| Program (output size)             | lld   | wild  | mold  | wild/mold | lld/mold
|-----------------------------------|-------|-------|-------|-----------|---------
| Blender 5.2 (0.24 GiB)            | 0.85s | 0.30s | 0.20s | 1.5x      | 4.2x
| Chromium 145 (0.58 GiB)           | 6.48s | N/A   | 0.64s | N/A       | 10.2x
| Chromium 145 ARM64 (0.60 GiB)     | 7.91s | N/A   | 0.73s | N/A       | 10.8x
| Clang 21 (0.21 GiB)               | 0.53s | 0.23s | 0.11s | 2.2x      | 5.0x
| ClickHouse 26.1 (1.23 GiB)        | 3.18s | 2.07s | 0.41s | 5.0x      | 7.7x
| Firefox 149 (0.22 GiB)            | 1.01s | 0.41s | 0.21s | 2.0x      | 4.9x
| Godot 4.6 (0.15 GiB)              | 0.44s | 0.21s | 0.08s | 2.7x      | 5.8x
| LibreOffice 26.2 (0.19 GiB)       | 1.13s | 0.57s | 0.19s | 3.0x      | 6.0x
| PyTorch 2.9 (0.31 GiB)            | 0.68s | 0.32s | 0.15s | 2.2x      | 4.7x
| TensorFlow 2.21 (0.73 GiB)        | 9.62s | N/A   | 0.70s | N/A       | 13.7x

**Apple M1 Ultra, debug builds**

| Program (output size)             | lld    | wild  | mold  | wild/mold | lld/mold
|-----------------------------------|--------|-------|-------|-----------|---------
| Blender 5.2 (2.46 GiB)            | 3.21s  | 1.81s | 1.56s | 1.2x      | 2.1x
| Chromium 145 (4.51 GiB)           | 9.54s  | 3.49s | 2.22s | 1.6x      | 4.3x
| Chromium 145 ARM64 (4.76 GiB)     | 12.67s | N/A   | 2.31s | N/A       | 5.5x
| Clang 21 (4.19 GiB)               | 4.40s  | 2.78s | 2.96s | 0.9x      | 1.5x
| ClickHouse 26.1 (5.58 GiB)        | 4.92s  | 3.50s | 1.71s | 2.1x      | 2.9x
| Firefox 149 (2.37 GiB)            | 3.21s  | N/A   | 1.12s | N/A       | 2.9x
| Firefox 149 ARM64 (2.43 GiB)      | 4.13s  | N/A   | 1.12s | N/A       | 3.7x
| Godot 4.6 (1.01 GiB)              | 1.12s  | 0.81s | 0.62s | 1.3x      | 1.8x
| LibreOffice 26.2 (0.98 GiB)       | 2.08s  | 0.94s | 0.63s | 1.5x      | 3.3x
| PyTorch 2.9 (3.51 GiB)            | 3.07s  | 2.10s | 1.45s | 1.4x      | 2.1x
| TensorFlow 2.21 (9.55 GiB)        | 43.68s | N/A   | 4.43s | N/A       | 9.9x

**Apple M1 Ultra, release builds**

| Program (output size)             | lld   | wild  | mold  | wild/mold | lld/mold
|-----------------------------------|-------|-------|-------|-----------|---------
| Blender 5.2 (0.24 GiB)            | 0.56s | 0.20s | 0.25s | 0.8x      | 2.3x
| Chromium 145 (0.58 GiB)           | 4.25s | N/A   | 0.78s | N/A       | 5.5x
| Chromium 145 ARM64 (0.60 GiB)     | 5.17s | N/A   | 0.91s | N/A       | 5.7x
| Clang 21 (0.21 GiB)               | 0.30s | 0.15s | 0.14s | 1.0x      | 2.1x
| ClickHouse 26.1 (1.23 GiB)        | 1.94s | 0.95s | 0.55s | 1.7x      | 3.5x
| Firefox 149 (0.22 GiB)            | 0.56s | 0.23s | 0.20s | 1.1x      | 2.7x
| Godot 4.6 (0.15 GiB)              | 0.26s | 0.11s | 0.11s | 1.0x      | 2.3x
| LibreOffice 26.2 (0.19 GiB)       | 0.62s | 0.30s | 0.23s | 1.3x      | 2.7x
| PyTorch 2.9 (0.31 GiB)            | 0.43s | 0.22s | 0.17s | 1.3x      | 2.5x
| TensorFlow 2.21 (0.73 GiB)        | 8.07s | N/A   | 0.68s | N/A       | 11.9x

N/A indicates that the linker cannot link that program. wild's Chromium
release links are also marked N/A because wild does not implement `--icf=all`
and links without identical code folding.

## Why is mold so fast?

mold owes its speed to pervasive parallelism and to efficient data structures
and algorithms. For details, read our paper "mold: A Massively Parallel
Linker" (ASPLOS 2027), available at https://arxiv.org/abs/2608.23228.

## Installation

Binary packages for the following systems are currently available:

[![Packaging status](https://repology.org/badge/vertical-allrepos/mold.svg)](https://repology.org/project/mold/versions)

Prebuilt binaries for Linux on x86-64, ARM64, ARM32, RISC-V, PPC64LE, s390x,
and LoongArch are also attached to each
[GitHub release](https://github.com/rui314/mold/releases).

## How to Build

mold is written in C++20, so if you build mold yourself, you will need a
recent version of a C++ compiler and a C++ standard library. We recommend GCC
10.2 or Clang 16.0.0 (or later) and libstdc++ 10 or libc++ 7 (or later).

### Install Dependencies

To install build dependencies, run `./install-build-deps.sh` in this
directory. It will detect your Linux distribution and attempt to install the
necessary packages.

### Compile mold

```shell
git clone --branch stable https://github.com/rui314/mold.git
cd mold
sudo ./install-build-deps.sh
cmake -DCMAKE_BUILD_TYPE=Release -DCMAKE_CXX_COMPILER=c++ -B build
cmake --build build -j$(nproc)
sudo cmake --build build --target install
```

You might need to pass a C++20 compiler command name to `cmake`. In the
example above, `c++` is passed. If that doesn't work for you, try a specific
version of a compiler, such as `g++-10` or `clang++-12`.

By default, `mold` is installed to `/usr/local/bin`. You can change the
installation location by passing `-DCMAKE_INSTALL_PREFIX=<directory>`.
For other cmake options, see the comments in `CMakeLists.txt`.

If you are not using a recent enough Linux distribution, or if `cmake` does
not work for you for any reason, you can use Podman to build mold in a
container. To do so, run `./dist.sh` in this directory instead of using
`cmake`. The shell script will pull a container image, build mold and auxiliary
files inside it, and package them into a single tar file named
`dist/mold-$version-$arch-linux.tar.gz`. You can extract the tar file anywhere
and use the mold executable in it.

## How to use

<details><summary>A classic way to use mold</summary>

On Unix, the linker command (usually `/usr/bin/ld`) is indirectly invoked by
the compiler driver (typically `cc`, `gcc`, or `clang`), which is in turn
indirectly invoked by `make` or other build system commands.

If you can specify an additional command line option for your compiler driver
by modifying the build system's config files, add one of the following flags
to use mold instead of `/usr/bin/ld`:

- For Clang: pass `-fuse-ld=mold`

- For GCC 12.1.0 or later: pass `-fuse-ld=mold`

- For GCC before 12.1.0: the `-fuse-ld` option does not accept `mold` as a
  valid argument, so you need to use the `-B` option instead. The `-B` option
  tells GCC where to look for external commands like `ld`.

  If you have installed mold with `make install`, there should be a directory
  named `/usr/libexec/mold` (or `/usr/local/libexec/mold`, depending on your
  `$PREFIX`), and the `ld` command should be there. The `ld` is actually a
  symlink to `mold`. So, all you need is to pass `-B/usr/libexec/mold` (or
  `-B/usr/local/libexec/mold`) to GCC.

If you haven't installed `ld.mold` to any `$PATH`, you can still pass
`-fuse-ld=/absolute/path/to/mold` to clang to use mold. However, GCC does not
accept an absolute path as an argument for `-fuse-ld`.

</details>

<details><summary>If you are using Rust</summary>

Create `.cargo/config.toml` in your project directory with the following:

```toml
[target.'cfg(target_os = "linux")']
linker = "clang"
rustflags = ["-C", "link-arg=-fuse-ld=/path/to/mold"]
```

where `/path/to/mold` is an absolute path to the mold executable. In the
example above, we use `clang` as a linker driver since it always accepts the
`-fuse-ld` option. If your GCC is recent enough to recognize the option, you
may be able to remove the `linker = "clang"` line.

```toml
[target.'cfg(target_os = "linux")']
rustflags = ["-C", "link-arg=-fuse-ld=mold"]
```

If you want to use mold for all projects, add the above snippet to
`~/.cargo/config.toml`.

</details>

<details><summary>If you are using Nim</summary>

Create `config.nims` in your project directory with the following:

```nim
when findExe("mold").len > 0 and defined(linux):
  switch("passL", "-fuse-ld=mold")
```

where `mold` must be included in the `PATH` environment variable. In this
example, `gcc` is used as the linker driver. Use the `-fuse-ld` option if your
GCC is recent enough to recognize this option.

If you want to use mold for all projects, add the above snippet to
`~/.config/config.nims`.

</details>

<details><summary>If you are using Conan package manager</summary>

You can configure [Conan](https://github.com/conan-io) to download the latest
version of `mold` and use it as the linker when building your dependencies and
projects from source. Please see the instructions [here](https://conan.io/center/recipes/mold).

</details>

<details><summary>mold -run</summary>

It is sometimes very hard to pass an appropriate command line option to `cc`
to specify an alternative linker. To address this situation, mold has a
feature to intercept all invocations of `ld`, `ld.bfd`, `ld.lld`, or `ld.gold`
and redirect them to itself. To use this feature, run `make` (or another build
command) as a subcommand of mold as follows:

```shell
mold -run make <make-options-if-any>
```

Internally, mold invokes a given command with the `LD_PRELOAD` environment
variable set to its companion shared object file. The shared object file
intercepts all function calls to `exec(3)`-family functions to replace
`argv[0]` with `mold` if it is `ld`, `ld.bf`, `ld.gold`, or `ld.lld`.

</details>

<details><summary>GitHub Actions</summary>

You can use our [setup-mold](https://github.com/rui314/setup-mold) GitHub
Action to speed up GitHub-hosted continuous builds. Although GitHub Actions
run on a 4 core machine, mold is still significantly faster than the default
GNU linker, especially when linking large programs.

</details>

<details><summary>Verify that you are using mold</summary>

mold leaves its identification string in the `.comment` section of an output
file. You can print it out to verify that you are actually using mold.

```shell
$ readelf -p .comment <executable-file>

String dump of section '.comment':
  [     0]  GCC: (Ubuntu 10.2.0-5ubuntu1~20.04) 10.2.0
  [    2b]  mold 9a1679b47d9b22012ec7dfbda97c8983956716f7
```

If `mold` is present in the `.comment` section, the file was created by mold.

</details>

<details><summary>Online manual</summary>

Since mold is a drop-in replacement, you should be able to use it without
reading its manual. However, if you need it, [mold's man page](docs/mold.md)
is available online. You can read the same manual by running `man mold`.

</details>

## Stability

mold has been developed in the open since 2020 and has more than 140
contributors. Its test suite, which covers every linker feature, runs in CI for
all supported target CPU architectures, natively or under QEMU, and under ASAN
and TSAN. Before each release, we try to build all of Gentoo Linux's roughly
19,000 packages with mold, using GNU ld as a control, to find regressions before
they reach a release.

## Sponsors

mold is free to use, but keeping it maintained is continuous work: supporting
new architectures and toolchain features, keeping up with the projects that
depend on it, and making it faster. That work is funded by sponsors. If mold
saves you or your company time, please consider becoming a
[GitHub sponsor](https://github.com/sponsors/rui314).

We thank everyone who sponsors the project. In particular, we'd like to
acknowledge the following people and organizations who have sponsored
$128/month or more:

### Corporate sponsors

<a href="https://mercury.com"><img src="docs/mercury-logo.png" align=center height=120 width=400 alt=Mercury></a>

<a href="https://cybozu-global.com"><img src="docs/cyboze-logo.png" align=center height=120 width=133 alt=Cybozu></a>

<a href="https://www.emergetools.com"><img src="docs/emerge-tools-logo.png" align=center height=120 width=240 alt="Emerge Tools"></a><br>

- [G-Research](https://www.gresearch.co.uk)
- [Signal Slot Inc.](https://github.com/signal-slot)
- [GlareDB](https://github.com/GlareDB)

### Individual sponsors

- [Wei Wu](https://github.com/lazyparser)
- [kyle-elliott](https://github.com/kyle-elliott)
- [Bryant Biggs](https://github.com/bryantbiggs)
- [kraptor23](https://github.com/kraptor23)
- [Jinkyu Yi](https://github.com/jincreator)
- [Pedro Navarro](https://github.com/pedronavf)
