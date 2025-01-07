#!/bin/bash
#
# This script creates a mold binary distribution. The output is written in
# this directory as `mold-$version-$arch-linux.tar.gz` (e.g.,
# `mold-1.0.3-x86_64-linux.tar.gz`).
#
# The mold executable created by this script is statically linked to
# libstdc++ but dynamically linked to libc, libm, libz, and librt, as
# these libraries almost always exist on any Linux system. We can't
# statically link libc because doing so would disable dlopen(), which is
# necessary to open the LTO linker plugin.
#
# This script aims to produce reproducible outputs. That means if you run
# the script twice on the same git commit, it should produce bit-by-bit
# identical binary files. This property is crucial as a countermeasure
# against supply chain attacks. With this, you can verify that the binary
# files distributed on the GitHub release pages are created from the
# commit with release tags by rebuilding the binaries yourself.
#
# Debian provides snapshot.debian.org to host all historical binary
# packages. We use it to construct Podman images pinned to a
# particular timestamp.
#
# We aim to use a reasonably old Debian version because we'll dynamically
# link glibc to mold, and a binary linked against a newer version of glibc
# won't work on a system with an older version of glibc.
#
# We want to build mold with Clang rather than GCC because mold's
# Identical Code Folding works best with the LLVM address significance
# table (.llvm_addrsig). Building a release binary with GCC yields a
# slightly larger binary than Clang's.
#
# We need a recent version of Clang to build mold. If it's not available
# via apt-get, we'll build it ourselves.
#
# You may need to install qemu-user-static package to build non-native
# binaries.

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
  # Debian 8 (Jessie) released in April 2015
  cat <<EOF | $image_build
FROM debian:jessie-20210326@sha256:32ad5050caffb2c7e969dac873bce2c370015c2256ff984b70c1c08b3a2816a0
ENV DEBIAN_FRONTEND=noninteractive TZ=UTC
RUN sed -i -e '/^deb/d' -e 's/^# deb /deb [trusted=yes] /g' /etc/apt/sources.list && \
  echo 'Acquire::Retries "10"; Acquire::http::timeout "10"; Acquire::Check-Valid-Until "false";' > /etc/apt/apt.conf.d/80-retries && \
  apt-get update && \
  apt-get install -y --no-install-recommends wget bzip2 file make autoconf gcc g++ libssl-dev && \
  rm -rf /var/lib/apt/lists

# Build CMake 3.27
RUN mkdir /build && \
  cd /build && \
  wget -O- --no-check-certificate https://cmake.org/files/v3.27/cmake-3.27.7.tar.gz | tar xzf - --strip-components=1 && \
  ./bootstrap --parallel=\$(nproc) && \
  make -j\$(nproc) && \
  make install && \
  rm -rf /build

# Build GCC 10
RUN mkdir /build && \
  cd /build && \
  wget -O- --no-check-certificate https://ftpmirror.gnu.org/gnu/gcc/gcc-10.5.0/gcc-10.5.0.tar.gz | tar xzf - --strip-components=1 && \
  mkdir isl gmp mpc mpfr && \
  wget -O- --no-check-certificate https://gcc.gnu.org/pub/gcc/infrastructure/isl-0.18.tar.bz2 | tar xjf - --strip-components=1 -C isl && \
  wget -O- --no-check-certificate https://ftpmirror.gnu.org/gnu/gmp/gmp-6.1.2.tar.bz2 | tar xjf - --strip-components=1 -C gmp && \
  wget -O- --no-check-certificate https://ftpmirror.gnu.org/gnu/mpc/mpc-1.2.1.tar.gz | tar xzf - --strip-components=1 -C mpc && \
  wget -O- --no-check-certificate https://ftpmirror.gnu.org/gnu/mpfr/mpfr-4.1.0.tar.gz | tar xzf - --strip-components=1 -C mpfr && \
  ./configure --prefix=/usr --enable-languages=c,c++ --disable-bootstrap --disable-multilib && \
  make -j\$(nproc) && \
  make install && \
  ln -sf /usr/lib64/libstdc++.so.6 /usr/lib/x86_64-linux-gnu/libstdc++.so.6 && \
  rm -rf /build

# Build GNU binutils 2.43
RUN mkdir /build && \
  cd /build && \
  wget -O- --no-check-certificate https://ftp.gnu.org/gnu/binutils/binutils-2.43.tar.gz | tar xzf - --strip-components=1 && \
  ./configure --prefix=/usr && \
  make -j\$(nproc) && \
  make install && \
  rm -fr /build

# Build Python 3.12.7
RUN mkdir /build && \
  cd /build && \
  wget -O- --no-check-certificate https://www.python.org/ftp/python/3.12.7/Python-3.12.7.tgz | tar xzf - --strip-components=1 && \
  ./configure && \
  make -j\$(nproc) && \
  make install && \
  ln -sf /usr/local/bin/python3 /usr/local/bin/python && \
  rm -rf /build

# Build LLVM 18.1.8
RUN mkdir /build && \
  cd /build && \
  wget -O- --no-check-certificate https://github.com/llvm/llvm-project/archive/refs/tags/llvmorg-18.1.8.tar.gz | tar xzf - --strip-components=1 && \
  mkdir b && \
  cd b && \
  cmake -DCMAKE_BUILD_TYPE=Release -DLLVM_ENABLE_PROJECTS=clang ../llvm && \
  cmake --build . -j\$(nproc) && \
  cmake --install . --strip && \
  rm -rf /build
EOF
  ;;
aarch64 | arm | ppc64le | s390x)
  # Debian 10 (Bullseye) released in July 2019
  #
  # We don't want to build Clang for these targets with Qemu becuase
  # that'd take extremely long time. Also I believe old build machines
  # are usually x86-64.
  cat <<EOF | $image_build
FROM debian:bullseye-20240904@sha256:8ccc486c29a3ad02ad5af7f1156e2152dff3ba5634eec9be375269ef123457d8
ENV DEBIAN_FRONTEND=noninteractive TZ=UTC
RUN sed -i -e '/^deb/d' -e 's/^# deb /deb [trusted=yes] /g' /etc/apt/sources.list && \
  echo 'Acquire::Retries "10"; Acquire::http::timeout "10"; Acquire::Check-Valid-Until "false";' > /etc/apt/apt.conf.d/80-retries && \
  apt-get update && \
  apt-get install -y --no-install-recommends build-essential gcc-10 g++-10 clang-16 cmake && \
  ln -sf /usr/bin/clang-16 /usr/bin/clang && \
  ln -sf /usr/bin/clang++-16 /usr/bin/clang++ && \
  rm -rf /var/lib/apt/lists
EOF
  ;;
riscv64)
  cat <<EOF | $image_build
FROM docker.io/riscv64/debian:unstable-20240926@sha256:25654919c2926f38952cdd14b3300d83d13f2d820715f78c9f4b7a1d9399bf48
ENV DEBIAN_FRONTEND=noninteractive TZ=UTC
RUN sed -i -e '/^URIs/d' -e 's/^# http/URIs: http/' /etc/apt/sources.list.d/debian.sources && \
  echo 'Acquire::Retries "10"; Acquire::http::timeout "10"; Acquire::Check-Valid-Until "false";' > /etc/apt/apt.conf.d/80-retries && \
  apt-get update && \
  apt-get install -y --no-install-recommends build-essential gcc-14 g++-14 clang-18 cmake && \
  ln -sf /usr/bin/clang-18 /usr/bin/clang && \
  ln -sf /usr/bin/clang++-18 /usr/bin/clang++ && \
  rm -rf /var/lib/apt/lists
EOF
  ;;
loongarch64)
  # LoongArch build is not reproducible yet
  cat <<EOF | $image_build
FROM docker.io/loongarch64/debian:sid
ENV DEBIAN_FRONTEND=noninteractive TZ=UTC
RUN apt-get update && \
  apt-get install -y --no-install-recommends build-essential gcc-14 g++-14 clang-17 cmake && \
  ln -sf /usr/bin/clang-17 /usr/bin/clang && \
  ln -sf /usr/bin/clang++-17 /usr/bin/clang++ && \
  rm -rf /var/lib/apt/lists
EOF
  ;;
*)
  usage
  ;;
esac

version=$(sed -n 's/^project(mold VERSION \(.*\))/\1/p' CMakeLists.txt)
dest=mold-$version-$arch-linux

# Source tarballs available on GitHub don't contain .git history.
# Clone the repo if missing.
[ -d .git ] || git clone --branch v$version --depth 1 --bare https://github.com/rui314/mold .git

# We use the timestamp of the last Git commit as the file timestamp
# for build artifacts.
timestamp="$(git log -1 --format=%ci)"

# Build mold in a container.
mkdir -p dist

podman run --arch $arch -it --rm --userns=host --pids-limit=-1 \
  -v "$(pwd):/mold:ro" -v "$(pwd)/dist:/dist" $image bash -c "
set -e
mkdir /build
cd /build
cmake -DCMAKE_BUILD_TYPE=Release -DMOLD_MOSTLY_STATIC=1 -DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++ /mold
cmake --build . -j\$(nproc)
cmake --install .
cmake -DMOLD_USE_MOLD=1 .
cmake --build . -j\$(nproc)
ctest --output-on-failure -j\$(nproc)
cmake --install . --prefix $dest --strip
find $dest -print | xargs touch --no-dereference --date='$timestamp'
find $dest -print | sort | tar -cf - --no-recursion --files-from=- | gzip -9nc > /dist/$dest.tar.gz
cp mold /dist
sha256sum /dist/$dest.tar.gz
"
