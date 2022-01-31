# mold: A Modern Linker

mold is a faster drop-in replacement for existing Unix linkers.
It is several times faster than LLVM lld linker, the second-fastest
open-source linker which I originally created a few years ago.
mold is created for increasing developer productivity by reducing
build time especially in rapid debug-edit-rebuild cycles.

Here is a performance comparison of GNU gold, LLVM lld, and mold for
linking final debuginfo-enabled executables of major large programs
on a simulated 8-core 16-threads machine.

![Link speed comparison](docs/comparison.png)

| Program (linker output size)  | GNU gold | LLVM lld | mold
|-------------------------------|----------|----------|--------
| Chrome 96 (1.89 GiB)          | 53.86s   | 11.74s   | 2.21s
| Clang 13 (3.18 GiB)           | 64.12s   | 5.82s    | 2.90s
| Firefox 89 libxul (1.64 GiB)  | 32.95s   | 6.80s    | 1.42s

mold is so fast that it is only 2x _slower_ than `cp` on the same
machine. Feel free to [file a bug](https://github.com/rui314/mold/issues)
if you find mold is not faster than other linkers.

mold currently supports x86-64, i386 and ARM64.

## Why does the speed of linking matter?

If you are using a compiled language such as C, C++ or Rust, a build
consists of two phases. In the first phase, a compiler compiles a
source file into an object file (`.o` files). In the second phase,
a linker takes all object files to combine them into a single executable
or a shared library file.

The second phase takes a long time if your build output is large.
mold can make it faster, saving your time and keeping you from being
distracted while waiting for a long build to finish. The difference is
most noticeable when you are in rapid debug-edit-rebuild cycles.

## Install

Binary packages for the following distros are currently available.

[![Packaging status](https://repology.org/badge/vertical-allrepos/mold.svg)](https://repology.org/project/mold/versions)

## How to build

mold is written in C++20, so if you build mold yourself, you need a
recent version of a C++ compiler and a C++ standard library. GCC 10.2
or Clang 12.0.0 as well as libstdc++ 10 or libc++ 7 are recommended.

I'm using Ubuntu 20.04 as a development platform. In that environment,
you can build mold by the following commands.

### Install dependencies

#### Ubuntu 20.04 and later / Debian 11 and later

```shell
sudo apt-get update
sudo apt-get install -y build-essential git clang cmake libstdc++-10-dev libsodium-dev libxxhash-dev zlib1g-dev pkg-config
```

#### Fedora 34 and later

```shell
sudo dnf install -y git clang cmake libsodium-devel xxhash-devel zlib-devel libstdc++-devel
```

### Compile mold

```shell
git clone https://github.com/rui314/mold.git
cd mold
git checkout v1.0.3
make -j$(nproc) CXX=clang++
sudo make install
```

You may need to pass a C++20 compiler command name to `make`.
In the above case, `clang++` is passed. If it doesn't work for you,
try a specific version of a compiler such as `g++-10` or `clang++-12`.

By default, `mold` is installed to `/usr/local/bin`.

If you don't use a recent enough Linux distribution, or if for any reason `make`
in the above commands doesn't work for you, you can use Docker to build it in
a Docker environment. To do so, just run `./build-static.sh` in this
directory instead of running `make -j$(nproc)`. The shell script creates a
Ubuntu 20.04 Docker image, installs necessary tools and libraries to it,
and builds mold as a statically-linked executable.

`make test` depends on a few more packages. To install, run the following commands:

```shell
sudo dpkg --add-architecture i386
sudo apt update
sudo apt-get install bsdmainutils dwarfdump libc6-dev:i386 lib32gcc-10-dev libstdc++-10-dev-arm64-cross gcc-10-aarch64-linux-gnu g++-10-aarch64-linux-gnu
```

## How to use

<details><summary>A classic way to use mold</summary>

On Unix, the linker command (which is usually `/usr/bin/ld`) is
invoked indirectly by the compiler driver (which is usually `cc`,
`gcc` or `clang`), which is typically in turn indirectly invoked by
`make` or some other build system command.

If you can specify an additional command line option to your compiler
driver by modifying build system's config files, add one of the
following flags to use `mold` instead of `/usr/bin/ld`:

- Clang: pass `-fuse-ld=mold`

- GCC 12.1.0 (upcoming version) or later: pass `-fuse-ld=mold`

- GCC before 12.1.0: `-fuse-ld` does not accept `mold` as a valid
  argument, so you need to use `-B` option instead. `-B` is an option
  to tell GCC where to look for external commands such as `ld`.

  If you have installed mold with `make install`, there should be a
  directory named `/usr/libexec/mold` (or `/usr/local/libexec/mold`,
  depending on your $PREFIX), and `ld` command should be there. The
  `ld` is actually a symlink to `mold`. So, all you need is to pass
  `-B/usr/libexec/mold` (or `-B/usr/local/libexec/mold`) to GCC.

If you haven't installed `mold` to any `$PATH`, you can still pass
`-fuse-ld=/absolute/path/to/mold` to clang to use mold. GCC does not
take an absolute path as an argument for `-fuse-ld` though.

</details>

<details><summary>If you are using Rust</summary>

Create `.cargo/config.toml` in your project directory with the following:

```
[target.x86_64-unknown-linux-gnu]
linker = "clang"
rustflags = ["-C", "link-arg=-fuse-ld=/path/to/mold"]
```

where `/path/to/mold` is an absolute path to `mold` exectuable.
Please make sure you have installed `clang`.

If you want to use mold for all projects, put the above snippet to
`~/.cargo/config.toml`.

</details>

<details><summary>mold -run</summary>

It is sometimes very hard to pass an appropriate command line option
to `cc` to specify an alternative linker.  To deal with the situation,
mold has a feature to intercept all invocations of `ld`, `ld.lld` or
`ld.gold` and redirect it to itself. To use the feature, run `make`
(or another build command) as a subcommand of mold as follows:

```shell
mold -run make <make-options-if-any>
```

Internally, mold invokes a given command with `LD_PRELOAD` environment
variable set to its companion shared object file. The shared object
file intercepts all function calls to `exec(3)`-family functions to
replace `argv[0]` with `mold` if it is `ld`, `ld.gold` or `ld.lld`.

</details>

mold leaves its identification string in `.comment` section in an output
file. You can print it out to verify that you are actually using mold.

```shell
readelf -p .comment <executable-file>

String dump of section '.comment':
  [     0]  GCC: (Ubuntu 10.2.0-5ubuntu1~20.04) 10.2.0
  [    2b]  mold 9a1679b47d9b22012ec7dfbda97c8983956716f7
```

If `mold` is in `.comment`, the file is created by mold.

## Why is mold so fast?

One reason is because it simply uses faster algorithms and efficient
data structures than other linkers do. The other reason is that the
new linker is highly parallelized.

Here is a side-by-side comparison of per-core CPU usage of lld (left)
and mold (right). They are linking the same program, Chromium
executable.

![CPU usage comparison in htop animation](docs/htop.gif)

As you can see, mold uses all available cores throughout its execution
and finishes quickly. On the other hand, lld failed to use available
cores most of the time. In this demo, the maximum parallelism is
artificially capped to 16 so that the bars fit in the GIF.

For details, please read [design notes](docs/design.md).

# Logo

-![mold image](docs/mold.jpg)
