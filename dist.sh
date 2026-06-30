#!/bin/bash
#
# This script creates a mold binary distribution. The output is written to
# the `dist` directory as `mold-$version-$arch-linux.tar.gz` (e.g.
# `mold-2.40.0-x86_64-linux.tar.gz`).
#
# This script aims to produce reproducible outputs. That means each time
# it's run on the same git commit, it generates a bit-for-bit identical
# binary file regardless of when or where it's executed. This property
# serves as a strong safeguard against supply chain attacks. With a
# reproducible build, anyone can independently verify that the binary
# files published on our GitHub release page were built from the git
# commit tagged for release by rebuilding the binaries themselves.
#
# Debian provides snapshot.debian.org to host all historical binary
# packages. We use it to construct a container image pinned to a
# particular timestamp. snapshot.debian.org is known to be very slow,
# but that shouldn't be a big problem for us because we only need that
# site the first time.
#
# The mold executable created by this script is statically linked to
# libc++, but dynamically linked to glibc, libm and a few other
# libraries, as these libraries are almost always available on any Linux
# system. We can't statically link glibc because doing so would disable
# dlopen(), which is required to load the LTO linker plugin.
#
# We use a reasonably old Debian version for the build environment because
# a binary dynamically linked against a newer version of glibc won't work
# on a system with an older version of glibc.
#
# We prefer to build mold with Clang rather than GCC because mold's
# Identical Code Folding works best with the LLVM address significance
# table (.llvm_addrsig). Building a release binary with GCC produces a
# slightly larger binary than with Clang.
#
# We need a recent version of Clang to build mold. If it's not available
# via apt-get, we'll build it ourselves.
#
# This script can be used to create non-native binaries (e.g., building
# aarch64 binary on x86-64) because Podman automatically runs everything
# under QEMU if the container image is not native. To use this script for
# non-native builds, you may need to install the qemu-user-static package.

set -e -x
cd "$(dirname $0)"

usage() {
  echo "Usage: $0 [ x86_64 | aarch64 | arm | riscv64 | ppc64le | s390x | loongarch64 ]"
  exit 1
}

case $# in
0)
  arch=$(uname -m)
  if [ $arch = arm64 ]; then
    arch=aarch64
  elif [[ $arch = arm* ]]; then
    arch=arm
  fi
  ;;
1)
  arch="$1"
  ;;
*)
  usage
esac

# Create a Podman image.
if [ "$GITHUB_REPOSITORY" = '' ]; then
  image=mold-builder-$arch
  image_build="podman build --arch $arch -t $image -"
else
  # If this script is running on GitHub Actions, we want to cache
  # the created container image in GitHub's container repostiory.
  image=ghcr.io/$GITHUB_REPOSITORY/mold-builder-$arch
  image_build="podman build --arch $arch -t $image --output=type=registry --layers --cache-to $image --cache-from $image -"
fi

case $arch in
x86_64)
  # Debian 10 was initially released on July 6th, 2019.
  #
  # We use a Google-provided mirror (gcr.io) instead of the official Docker
  # Hub (docker.io) because docker.io has a strict rate limit policy.
  #
  # The toolchain in Debian 10 is too old to build mold, so we rebuild it
  # from source. We download source archives from official sites and build
  # them locally, rather than downloading pre-built binaries from somewhere
  # else, to avoid relying on unverifiable third-party binary blobs. Podman
  # caches the result of each RUN command, so rebuilding is done only once
  # per host.
  cat <<EOF | $image_build
FROM mirror.gcr.io/library/debian:buster@sha256:58ce6f1271ae1c8a2006ff7d3e54e9874d839f573d8009c20154ad0f2fb0a225
ENV DEBIAN_FRONTEND=noninteractive TZ=UTC
RUN sed -i -e '/^deb/d' -e 's/^# deb /deb /g' /etc/apt/sources.list && \
  echo 'Acquire::Retries "10"; Acquire::http::timeout "10"; Acquire::Check-Valid-Until "false";' > /etc/apt/apt.conf.d/80-retries && \
  apt-get update && \
  apt-get install -y --no-install-recommends wget file make gcc g++ zlib1g-dev libssl-dev ca-certificates && \
  rm -rf /var/lib/apt/lists

# Build GNU binutils 2.46.1
RUN mkdir /build && \
  cd /build && \
  wget -O- --progress=dot:mega https://ftpmirror.gnu.org/binutils/binutils-2.46.1.tar.gz | \
  tar xzf - --strip-components=1 && \
  ./configure --prefix=/usr && \
  make -j\$(nproc) && \
  make install && \
  rm -fr /build

# Build Python 3.14.6
RUN mkdir /build && \
  cd /build && \
  wget -O- --progress=dot:mega https://www.python.org/ftp/python/3.14.6/Python-3.14.6.tgz | \
  tar xzf - --strip-components=1 && \
  ./configure && \
  make -j\$(nproc) && \
  make install && \
  rm -rf /build

# Build CMake 4.3
RUN mkdir /build && \
  cd /build && \
  wget -O- --progress=dot:mega https://cmake.org/files/v4.3/cmake-4.3.3.tar.gz | \
  tar xzf - --strip-components=1 && \
  ./bootstrap --parallel=\$(nproc) -- -DCMAKE_USE_OPENSSL=OFF && \
  make -j\$(nproc) && \
  make install && \
  rm -rf /build

# Build LLVM 22 w/ clang and libc++
RUN mkdir /build && \
  cd /build && \
  wget -O- --progress=dot:mega https://github.com/llvm/llvm-project/archive/refs/tags/llvmorg-22.1.8.tar.gz | \
  tar xzf - --strip-components=1 && \
  mkdir b && \
  cmake -S llvm -B b -DCMAKE_BUILD_TYPE=Release -DLLVM_ENABLE_PROJECTS=clang \
  -DLLVM_ENABLE_RUNTIMES='libcxx;libcxxabi;libunwind;compiler-rt' \
  -DLLVM_RUNTIME_TARGETS=x86_64-unknown-linux-gnu \
  -DLLVM_INCLUDE_TESTS=OFF && \
  cmake --build b -j\$(nproc) && \
  cmake --install b --strip && \
  rm -rf /build
EOF
  ;;
aarch64 | arm | ppc64le | s390x)
  # Debian 11 (Bullseye) was initially released on August 14th, 2021
  #
  # We don't want to build Clang for these targets on QEMU becuase it
  # would take an extremely long time. Also, I believe old Linux boxes
  # are typically x86-64.
  cat <<EOF | $image_build
FROM mirror.gcr.io/library/debian:bullseye-20260610@sha256:68cf0d859b046494f3c4288171bc477580e424f981d08f2a77742b982c32a38f
ENV DEBIAN_FRONTEND=noninteractive TZ=UTC
RUN sed -i -e '/^deb/d' -e 's/^# deb /deb /g' /etc/apt/sources.list && \
  echo 'Acquire::Retries "10"; Acquire::http::timeout "10"; Acquire::Check-Valid-Until "false";' > /etc/apt/apt.conf.d/80-retries && \
  apt-get update && \
  apt-get install -y --no-install-recommends build-essential gcc-10 g++-10 clang-19 libc++-19-dev cmake && \
  ln -sf /usr/bin/clang-19 /usr/bin/clang && \
  ln -sf /usr/bin/clang++-19 /usr/bin/clang++ && \
  rm -rf /var/lib/apt/lists
EOF
  ;;
riscv64)
  # Debian 13 (Trixie) was initially released on August 9th, 2025
  #
  # This was the first Debian stable release that included riscv64
  cat <<EOF | $image_build
FROM mirror.gcr.io/riscv64/debian:trixie-20260610@sha256:514de625d1bf895c317bb512d021c366c7a51c1ef0e92b0ce0700cb1f3408a77
ENV DEBIAN_FRONTEND=noninteractive TZ=UTC
RUN sed -i -e '/^URIs/d' -e 's/^# http/URIs: http/' /etc/apt/sources.list.d/debian.sources && \
  echo 'Acquire::Retries "10"; Acquire::http::timeout "10"; Acquire::Check-Valid-Until "false";' > /etc/apt/apt.conf.d/80-retries && \
  apt-get update && \
  apt-get install -y --no-install-recommends build-essential gcc-14 g++-14 clang-19 libc++-19-dev cmake && \
  ln -sf /usr/bin/clang-19 /usr/bin/clang && \
  ln -sf /usr/bin/clang++-19 /usr/bin/clang++ && \
  rm -rf /var/lib/apt/lists
EOF
  ;;
loongarch64)
  # Debian sid snapshot from September 24, 2024
  #
  # This is the only available Debian OCI for loongarch64 as of June 2026
  cat <<EOF | $image_build
FROM mirror.gcr.io/loongarch64/debian:sid@sha256:0356df4e494bbb86bb469377a00789a5b42bbf67d5ff649a3f9721b745cbef77
ENV DEBIAN_FRONTEND=noninteractive TZ=UTC
RUN sed -i -e 's!http[^ ]*!http://snapshot.debian.org/archive/debian-ports/20250620T014755Z!g' /etc/apt/sources.list && \
  echo 'Acquire::Retries "10"; Acquire::http::timeout "10"; Acquire::Check-Valid-Until "false";' > /etc/apt/apt.conf.d/80-retries && \
  apt-get update && \
  apt-get install -y --no-install-recommends build-essential gcc-14 g++-14 clang-19 libc++-19-dev cmake && \
  ln -sf /usr/bin/clang-19 /usr/bin/clang && \
  ln -sf /usr/bin/clang++-19 /usr/bin/clang++ && \
  rm -rf /var/lib/apt/lists
EOF
  ;;
*)
  usage
  ;;
esac

version=$(sed -n 's/^project(mold VERSION \(.*\))/\1/p' CMakeLists.txt)
dest=mold-$version-$arch-linux

# Source tarballs available on GitHub don't contain .git directory.
# Clone the repo if missing.
[ -d .git ] || git clone --branch v$version --depth 1 --bare https://github.com/rui314/mold .git

# We use the timestamp of the last Git commit as the file timestamp
# for build artifacts.
timestamp=$(git log -1 --format=%ct)

# `uname -m` in an ARM32 container running on an ARM64 host reports it
# not as ARM32 but as ARM64. That confuses BLAKE3's cmake script and
# erroneously enables NEON SIMD instructions. `setarch` can be used to
# change the output of `uname -m`.
setarch=
[ $arch = arm ] && setarch='setarch linux32'

mkdir -p dist

# Build mold in a container.
#
# SOURCE_DATE_EPOCH is a standardized environment variable that allows
# build artifacts to appear as if they were built at a specific time.
# We use it to control how the compiler expands the C/C++ __DATE__ and
# __TIME__ macros.
podman run --arch $arch -it --rm --userns=host --pids-limit=-1 --network=none \
  --pull=never -v "$(pwd):/mold:ro" -v "$(pwd)/dist:/dist" $image \
   $setarch bash -c "
set -e
export SOURCE_DATE_EPOCH=$timestamp
mkdir /build
cd /build
cmake -DCMAKE_BUILD_TYPE=Release -DMOLD_MOSTLY_STATIC=1 -DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++ /mold
cmake --build . -j\$(nproc)
cmake --install .
cmake -DMOLD_USE_MOLD=1 .
cmake --build . -j\$(nproc)
ctest --output-on-failure -j\$(nproc)
cmake --install . --prefix $dest --strip
find $dest -print | xargs touch --no-dereference --date=@$timestamp
find $dest -print | sort | tar -cf - --no-recursion --files-from=- | gzip -9nc > /dist/$dest.tar.gz
cp mold /dist
sha256sum /dist/$dest.tar.gz
"
