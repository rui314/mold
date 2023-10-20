#!/bin/bash
#
# This script creates a Docker image for ./dist.sh script.
#
# We want to use a reasonably old Linux distro because we'll dynamically
# link glibc to mold, and a binary linked against a newer version of
# glibc won't work on a system with a older version of glibc.
#
# We might want to instead statically-link everything, but that'll
# disable dlopen(), which means we can't use the linker plugin for LTO.
# So we don't want to do that.

set -x

if [ $# -ne 1 ]; then
  echo "Usage: $0 [ x86_64 | aarch64 | arm | riscv64 | ppc64le | s390x ]"
  exit 1
fi

arch="$1"
echo "$arch" | grep -Eq '^(x86_64|aarch64|arm|riscv64|ppc64le|s390x)$' || \
  { echo "Error: no docker image for $arch"; exit 1; }

if [ $arch = riscv64 ]; then
  # Ubuntu 18 didn't support RISC-V, so use Ubuntu 20 instead
  cat <<EOF | docker build --platform $arch -t rui314/mold-builder-$arch:latest --push -
FROM riscv64/ubuntu:20.04
RUN apt-get update && \
  DEBIAN_FRONTEND=noninteractive TZ=UTC apt-get install -y --no-install-recommends build-essential gcc-10 g++-10 cmake && \
  rm -rf /var/lib/apt/lists
EOF
else
  cat <<EOF | docker build --platform $arch -t rui314/mold-builder-$arch:latest --push -
FROM ubuntu:18.04
RUN apt-get update && \
  apt-get install -y --no-install-recommends software-properties-common && \
  add-apt-repository -y ppa:ubuntu-toolchain-r/test && \
  apt-get update && \
  DEBIAN_FRONTEND=noninteractive TZ=UTC apt-get install -y --no-install-recommends build-essential wget libstdc++-11-dev gcc-10 g++-10 python3 libssl-dev && \
  \
  mkdir /cmake && cd /cmake && \
  wget -O- -q https://github.com/Kitware/CMake/releases/download/v3.24.2/cmake-3.24.2.tar.gz | tar --strip-components=1 -xzf - && \
  ./bootstrap --parallel=$(nproc) && make -j$(nproc) && make -j$(nproc) install && \
  rm -rf /var/lib/apt/lists /cmake
EOF
fi
