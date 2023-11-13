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
# packages. We use it to construct Docker images pinned to a
# particular timestamp.
#
# We aim to use a reasonably old Debian version because we'll dynamically
# link glibc to mold, and a binary linked against a newer version of glibc
# won't work on a system with an older version of glibc.
#
# We need GCC 10 or newer to build mold. If GCC 10 is not available on an
# old Debian version, we'll build it ourselves.

set -e -x

usage() {
  echo "Usage: $0 [ x86_64 | aarch64 | arm | riscv64 | ppc64le | s390x ]"
  exit 1
}

case $# in
0)
  arch=$(uname -m)
  [[ $arch = arm* ]] && arch=arm
  ;;
1)
  arch="$1"
  ;;
*)
  usage
esac

echo "$arch" | grep -Eq '^(x86_64|aarch64|arm|riscv64|ppc64le|s390x)$' || usage

image=mold-builder-$arch
version=$(sed -n 's/^project(mold VERSION \(.*\))/\1/p' $(dirname $0)/CMakeLists.txt)
dest=mold-$version-$arch-linux

# Create a Docker image.
case $arch in
x86_64)
  cat <<EOF | docker build --platform $arch -t $image -
FROM debian:buster-20231030
ENV DEBIAN_FRONTEND=noninteractive TZ=UTC
RUN echo 'deb http://snapshot.debian.org/archive/debian/20231030T000000Z buster main' > /etc/apt/sources.list && \
  echo 'deb http://snapshot.debian.org/archive/debian-security/20231030T000000Z buster/updates main' >> /etc/apt/sources.list && \
  echo 'deb http://snapshot.debian.org/archive/debian/20231030T000000Z buster-updates main' >> /etc/apt/sources.list && \
  apt-get update -o Acquire::Check-Valid-Until=false && \
  apt-get install -y wget bzip2 file make autoconf gcc g++ libssl-dev && \
  rm -rf /var/lib/apt/lists

# Build CMake 3.27
RUN mkdir -p /build/cmake && \
  cd /build/cmake && \
  wget -O- -q https://github.com/Kitware/CMake/releases/download/v3.27.7/cmake-3.27.7.tar.gz | tar xzf - --strip-components=1 && \
  ./bootstrap --parallel=$(nproc) && \
  make -j$(nproc) && \
  make install && \
  rm -rf /build

# Build GCC 10
RUN mkdir -p /build/gcc && \
  cd /build/gcc && \
  wget -O- -q https://ftp.gnu.org/gnu/gcc/gcc-10.5.0/gcc-10.5.0.tar.gz | tar xzf - --strip-components=1 && \
  mkdir isl gmp mpc mpfr && \
  wget -O- -q https://gcc.gnu.org/pub/gcc/infrastructure/isl-0.24.tar.bz2 | tar xjf - --strip-components=1 -C isl && \
  wget -O- -q https://ftp.gnu.org/gnu/gmp/gmp-6.3.0.tar.gz | tar xzf - --strip-components=1 -C gmp && \
  wget -O- -q https://ftp.gnu.org/gnu/mpc/mpc-1.3.1.tar.gz | tar xzf - --strip-components=1 -C mpc && \
  wget -O- -q https://ftp.gnu.org/gnu/mpfr/mpfr-4.2.1.tar.gz | tar xzf - --strip-components=1 -C mpfr && \
  ./configure --enable-languages=c,c++ --disable-bootstrap --disable-multilib && \
  make -j$(nproc) && \
  make install && \
  ln -sf /usr/local/bin/gcc /usr/local/bin/cc && \
  ln -sf /usr/local/bin/g++ /usr/local/bin/c++ && \
  rm -rf /build
EOF
  ;;
aarch64 | arm | ppc64le | s390x)
  cat <<EOF | docker build --platform $arch -t $image -
FROM debian:bullseye-20231030
ENV DEBIAN_FRONTEND=noninteractive TZ=UTC
RUN echo 'deb http://snapshot.debian.org/archive/debian/20231030T000000Z bullseye main' > /etc/apt/sources.list && \
  echo 'deb http://snapshot.debian.org/archive/debian-security/20231030T000000Z bullseye-security main' >> /etc/apt/sources.list && \
  echo 'deb http://snapshot.debian.org/archive/debian/20231030T000000Z bullseye-updates main' >> /etc/apt/sources.list && \
  apt-get update -o Acquire::Check-Valid-Until=false && \
  apt-get install -y --no-install-recommends build-essential gcc-10 g++-10 cmake && \
  ln -s /usr/bin/gcc-10 /usr/local/bin/cc && \
  ln -s /usr/bin/g++-10 /usr/local/bin/c++ && \
  rm -rf /var/lib/apt/lists
EOF
  ;;
riscv64)
  # snapshot.debian.org is not available for RISC-V binaries
  cat <<EOF | docker build --platform $arch -t $image -
FROM riscv64/debian:sid-20231030
ENV DEBIAN_FRONTEND=noninteractive TZ=UTC
RUN apt-get update && \
  apt-get install -y --no-install-recommends build-essential gcc-12 g++-12 cmake && \
  ln -s /usr/bin/gcc-12 /usr/local/bin/cc && \
  ln -s /usr/bin/g++-12 /usr/local/bin/c++ && \
  rm -rf /var/lib/apt/lists
EOF
  ;;
esac

# We use the timestamp of the last Git commit as the file timestamp
# for build artifacts.
timestamp="$(git log -1 --format=%ci)"

# Build mold in a container.
docker run --platform linux/$arch -i --rm -v "$(realpath $(dirname $0)):/mold" $image \
  bash -c "mkdir -p /build/mold &&
cd /build/mold &&
cmake -DCMAKE_BUILD_TYPE=Release -DMOLD_MOSTLY_STATIC=On /mold &&
cmake --build . -j$(nproc) &&
cmake --install . --prefix $dest --strip &&
find $dest -print | xargs touch --no-dereference --date='$timestamp' &&
tar -cf - --sort=name $dest | gzip -9nc > /mold/$dest.tar.gz &&
chown $(id -u):$(id -g) /mold/$dest.tar.gz"
