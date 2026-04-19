# Instrumenting mold with MemorySanitizer

## Introduction

Per <https://github.com/google/sanitizers/wiki/MemorySanitizerLibcxxHowTo>:

> If you want MemorySanitizer to work properly and not produce any false
> positives, you must ensure that all the code in your program and in
> libraries it uses is instrumented (i.e. built with `-fsanitize=memory`).
> In particular, you would need to link against an MSan-instrumented C++
> standard library. We recommend to use [libc++](https://libcxx.llvm.org/)
> for that purpose.

## Building instrumented libc++

Build an MSan-instrumented libc++ from source:

```sh
cd ~
git clone https://github.com/llvm/llvm-project
cd llvm-project
cmake -S ./runtimes -B build-libcxx -G Ninja  \
    -DCMAKE_BUILD_TYPE=Release                \
    -DLLVM_ENABLE_RUNTIMES="libcxx;libcxxabi" \
    -DCMAKE_C_COMPILER=clang                  \
    -DCMAKE_CXX_COMPILER=clang++              \
    -DLLVM_USE_SANITIZER=MemoryWithOrigins
cmake --build build-libcxx -- cxx cxxabi
export LIBCXX="$HOME/llvm-project/build-libcxx" # for subsequent build steps
```

Upon success, `./build-libcxx/{include,lib}` will contain the resulting
headers and shared libraries.

## Linking mold against instrumented libc++

Use the `MOLD_USE_MSAN` and `MOLD_STDLIB_PREFIX` cmake variables to link
mold against the instrumented build of libc++:

```sh
cd ~
git clone https://github.com/rui314/mold.git
cd mold
cmake -B build -G Ninja            \
    -DCMAKE_BUILD_TYPE=Debug       \
    -DCMAKE_C_COMPILER=clang       \
    -DCMAKE_CXX_COMPILER=clang++   \
    -DMOLD_USE_MSAN=ON             \
    -DMOLD_STDLIB_PREFIX="$LIBCXX" \
    -DMOLD_USE_MIMALLOC=OFF        \
    -DMOLD_USE_SYSTEM_TBB=ON
cmake --build build
```

Most of mold's tests (except those for `-flto`) should work at this
point. Run them like normal:

```sh
ctest --test-dir build
```

Any resulting MemorySanitizer errors should be visible in
`./build/Temporary/Testing/LastTest.log`.

## Building instrumented LTO plugin (experimental)

Exercising `-flto` with MemorySanitizer (and without false positives)
requires instrumenting the transitive dependencies of `lto-unix.cc`:

- `libiberty.a`
- `liblto_plugin.so`
- `LLVMgold.so`

> [!NOTE]
> This is more involved and time-consuming than building only libc++ from
> source. The cost-benefit ratio of this additional instrumentation and
> test coverage may be unfavorable in many cases.

> [!IMPORTANT]
> The following steps are experimental and unlikely to work exactly as-is
> under CI runners and individual developer environments. Consider the
> following a starting point rather than a complete HOWTO.

Build an MSan-instrumented GNU libiberty:

```sh
cd ~
git clone git@github.com:gcc-mirror/gcc.git
cd gcc/libiberty
export CC=clang
export CFLAGS="-g -Og -fsanitize=memory -fsanitize-memory-track-origins"
export LDFLAGS="-fsanitize=memory"
./configure
make -j$(nproc)
unset CC CFLAGS LDFLAGS
```

Install the resulting `./libiberty.a` into the build toolchain being
tested with mold. If the system image is ephemeral or disposable (e.g. a
short-lived VM or container), a quick-and-dirty install could look like:

```sh
sudo cp /usr/lib/libiberty.a{,.bak}
sudo cp ./libiberty.a /usr/lib/
```

Build an MSan-instrumented GCC LTO plugin library:

```sh
cd ~/gcc/lto-plugin # use already-cloned repo from earlier step
export CC=clang
export CFLAGS="-g -Og -fsanitize=memory -fsanitize-memory-track-origins"
export LDFLAGS="-fsanitize=memory"
./configure --with-libiberty=/usr/lib
make -j$(nproc)
unset CC CFLAGS LDFLAGS
```

Install the resulting `liblto_plugin.so`. With the same caveats discussed
above, a simple install into an ephemeral environment could look like:

```sh
export GCC_VERSION="$(gcc -dumpversion)"
sudo cp /usr/lib/gcc/x86_64-pc-linux-gnu/$GCC_VERSION/liblto_plugin.so{,.bak}
sudo cp .libs/liblto_plugin.so /usr/lib/gcc/x86_64-pc-linux-gnu/$GCC_VERSION/
```

Build an MSan-instrumented LLVM LTO plugin library:

```sh
cd ~/llvm-project # use already-cloned repo from earlier step

CF="-nostdinc++ -isystem $LIBCXX/include/c++/v1"
INCDIR="$(find /usr -name plugin-api.h -type f | head -1 | xargs dirname)"

function configure
{
    cmake -S ./llvm -B build-plugin -G Ninja   \
        -DLLVM_ENABLE_PROJECTS=clang           \
        -DLLVM_TARGETS_TO_BUILD=X86            \
        -DCMAKE_BUILD_TYPE=Release             \
        -DCMAKE_C_COMPILER=clang               \
        -DCMAKE_CXX_COMPILER=clang++           \
        -DCMAKE_C_FLAGS="$CF"                  \
        -DCMAKE_CXX_FLAGS="$CF"                \
        -DCMAKE_EXE_LINKER_FLAGS="$1"          \
        -DCMAKE_POSITION_INDEPENDENT_CODE=ON   \
        -DLLVM_BINUTILS_INCDIR="$INCDIR"       \
        -DLLVM_USE_SANITIZER=MemoryWithOrigins
}

# workaround for linker issues: configure twice with different LDFLAGS
configure "-nostdlib++ -L $LIBCXX/lib -Wl,--rpath=$LIBCXX/lib"
configure "-nostdlib++ -L $LIBCXX/lib -lc++ -Wl,--rpath=$LIBCXX/lib"

cmake --build build-plugin -- LLVMgold.so
```

Install the resulting `LLVMgold.so`. With the same caveats discussed
above, a simple install into an ephemeral environment could look like:

```sh
sudo cp /usr/lib/LLVMgold.so{,.bak}
sudo cp ./build-plugin/lib/LLVMgold.so /usr/lib/
```

mold's tests for `-flto` should now (mostly) work, though there do seem
to be issues around `gcc` inferring `-flto` when not explicitly specified.
One workaround is to use `clang` as `TEST_CC`:

```sh
cd ~/mold # use already-cloned and built repo from earlier step
TEST_CC=clang TEST_CXX=clang++ ctest --test-dir build
```

## References

- <https://github.com/google/sanitizers/wiki/MemorySanitizerBootstrappingClang>
- <https://llvm.org/docs/GoldPlugin.html#lto-how-to-build>
