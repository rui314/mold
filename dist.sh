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
#
# You may need to run the following command to use Docker with Qemu:
#
#  $ docker run --rm --privileged multiarch/qemu-user-static --reset -p yes

set -e -x

usage() {
  echo "Usage: $0 [ x86_64 | aarch64 | arm | riscv64 | ppc64le | s390x ]"
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

echo "$arch" | grep -Eq '^(x86_64|aarch64|arm|riscv64|ppc64le|s390x)$' || usage

version=$(sed -n 's/^project(mold VERSION \(.*\))/\1/p' $(dirname $0)/CMakeLists.txt)
dest=mold-$version-$arch-linux

if [ "$GITHUB_REPOSITORY" = '' ]; then
  image=mold-builder-$arch
  docker_build="docker build --platform linux/$arch -t $image"
else
  image=ghcr.io/$GITHUB_REPOSITORY/mold-builder-$arch
  docker_build="docker buildx build --platform linux/$arch -t $image --push --cache-to type=inline --cache-from type=registry,ref=ghcr.io/$GITHUB_REPOSITORY/mold-builder-$arch"
fi

# Create a Docker image.
case $arch in
x86_64)
  # Debian 8 (Jessie) released in April 2015
  cat <<EOF | $docker_build -
FROM debian:jessie-20210326
ENV DEBIAN_FRONTEND=noninteractive TZ=UTC
RUN sed -i -e '/^deb/d' -e 's/^# //g' /etc/apt/sources.list && \
  echo 'Acquire { Retries "10"; http::timeout "10"; Check-Valid-Until "false"; };' > /etc/apt/apt.conf.d/80-retries && \
  apt-get update && \
  apt-get install -y --force-yes --no-install-recommends wget bzip2 file make autoconf gcc g++ libssl-dev && \
  rm -rf /var/lib/apt/lists

# Build CMake 3.27
RUN mkdir -p /build/cmake && \
  cd /build/cmake && \
  wget -O- --no-check-certificate https://cmake.org/files/v3.27/cmake-3.27.7.tar.gz | tar xzf - --strip-components=1 && \
  ./bootstrap --parallel=\$(nproc) && \
  make -j\$(nproc) && \
  make install && \
  rm -rf /build

# Build GCC 10
RUN mkdir -p /build/gcc && \
  cd /build/gcc && \
  wget -O- http://ftp.gnu.org/gnu/gcc/gcc-10.5.0/gcc-10.5.0.tar.gz | tar xzf - --strip-components=1 && \
  mkdir isl gmp mpc mpfr && \
  wget -O- http://gcc.gnu.org/pub/gcc/infrastructure/isl-0.18.tar.bz2 | tar xjf - --strip-components=1 -C isl && \
  wget -O- http://ftp.gnu.org/gnu/gmp/gmp-6.1.2.tar.bz2 | tar xjf - --strip-components=1 -C gmp && \
  wget -O- http://ftp.gnu.org/gnu/mpc/mpc-1.2.1.tar.gz | tar xzf - --strip-components=1 -C mpc && \
  wget -O- http://ftp.gnu.org/gnu/mpfr/mpfr-4.1.0.tar.gz | tar xzf - --strip-components=1 -C mpfr && \
  ./configure --prefix=/usr --enable-languages=c,c++ --disable-bootstrap --disable-multilib && \
  make -j\$(nproc) && \
  make install && \
  ln -sf /usr/lib64/libstdc++.so.6 /usr/lib/x86_64-linux-gnu/libstdc++.so.6 && \
  rm -rf /build
EOF
  ;;
aarch64 | arm | ppc64le | s390x)
  # Debian 10 (Bullseye) released in July 2019
  cat <<EOF | $docker_build -
FROM debian:bullseye-20231030
ENV DEBIAN_FRONTEND=noninteractive TZ=UTC
RUN sed -i -e '/^deb/d' -e 's/^# //g' /etc/apt/sources.list && \
  echo 'Acquire { Retries "10"; http::timeout "10"; Check-Valid-Until "false"; };' > /etc/apt/apt.conf.d/80-retries && \
  apt-get update && \
  apt-get install -y --no-install-recommends build-essential gcc-10 g++-10 cmake && \
  ln -sf /usr/bin/gcc-10 /usr/bin/cc && \
  ln -sf /usr/bin/g++-10 /usr/bin/c++ && \
  rm -rf /var/lib/apt/lists
EOF
  ;;
riscv64)
  # snapshot.debian.org is not available for RISC-V binaries
  cat <<EOF | $docker_build -
FROM riscv64/debian:unstable-20231030
ENV DEBIAN_FRONTEND=noninteractive TZ=UTC
RUN apt-get update && \
  apt-get install -y --no-install-recommends build-essential gcc-12 g++-12 cmake && \
  ln -sf /usr/bin/gcc-12 /usr/bin/cc && \
  ln -sf /usr/bin/g++-12 /usr/bin/c++ && \
  rm -rf /var/lib/apt/lists
EOF
  ;;
esac

# We use the timestamp of the last Git commit as the file timestamp
# for build artifacts.
timestamp="$(git log -1 --format=%ci)"

# Build mold in a container.
docker run --platform linux/$arch -i --rm -v "$(realpath $(dirname $0)):/mold" $image \
  bash -c "set -e
mkdir -p /build/mold
cd /build/mold
cmake -DCMAKE_BUILD_TYPE=Release -DMOLD_MOSTLY_STATIC=On /mold
cmake --build . -j\$(nproc)
ctest -j\$(nproc)
cmake --install . --prefix $dest --strip
find $dest -print | xargs touch --no-dereference --date='$timestamp'
find $dest -print | sort | tar -cf - --no-recursion --files-from=- | gzip -9nc > /mold/$dest.tar.gz
chown $(id -u):$(id -g) /mold/$dest.tar.gz"
