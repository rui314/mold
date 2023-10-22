#!/bin/bash
#
# This script creates a mold binary distribution. The output is
# written in this directory as `mold-$version-$arch-linux.tar.gz`
# (e.g. `mold-1.0.3-x86_64-linux.tar.gz`).
#
# The mold executable created by this script is statically-linked to
# libstdc++ and libcrypto but dynamically-linked to libc, libm, libz
# and librt, as they almost always exist on any Linux systems.

set -e -x

case $# in
0)
  arch=$(uname -m)
  [[ $arch = arm* ]] && arch=arm
  ;;
1)
  arch="$1"
  ;;
*)
  echo "Usage: $0 [ x86_64 | aarch64 | arm | riscv64 | ppc64le | s390x ]"
  exit 1
esac

echo "$arch" | grep -Eq '^(x86_64|aarch64|arm|riscv64|ppc64le|s390x)$' || \
  { echo "Error: no docker image for $arch"; exit 1; }

image=mold-builder-$arch:1
version=$(sed -n 's/^project(mold VERSION \(.*\))/\1/p' $(dirname $0)/CMakeLists.txt)
dest=mold-$version-$arch-linux

# Create a Docker image if not exists.
#
# We want to use a reasonably old Linux distro because we'll dynamically
# link glibc to mold, and a binary linked against a newer version of
# glibc won't work on a system with a older version of glibc.
#
# We might want to instead statically-link everything, but that'll
# disable dlopen(), which means we can't use the linker plugin for LTO.
# So we don't want to do that.
if ! docker image ls $image | grep -q $image; then
  if [ $arch = riscv64 ]; then
    # Ubuntu 18 didn't support RISC-V, so use Ubuntu 20 instead
    cat <<EOF | docker build --platform $arch -t $image -
FROM riscv64/ubuntu:20.04
RUN apt-get update && \
  DEBIAN_FRONTEND=noninteractive TZ=UTC apt-get install -y --no-install-recommends build-essential gcc-10 g++-10 cmake && \
  rm -rf /var/lib/apt/lists
EOF
  else
    cat <<EOF | docker build --platform $arch -t $image -
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
fi

# Build mold in a container.
docker run --platform linux/$arch -it --rm -v "$(realpath $(dirname $0)):/mold" \
  -e "OWNER=$(id -u):$(id -g)" $image \
  bash -c "mkdir /build &&
cd /build &&
cmake -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_COMPILER=gcc-10 -DCMAKE_CXX_COMPILER=g++-10 -DMOLD_MOSTLY_STATIC=On /mold &&
cmake --build . -j\$(nproc) &&
[ $arch = arm ] || ctest -j\$(nproc) &&
cmake --install . --prefix $dest --strip &&
tar czf /mold/$dest.tar.gz $dest &&
chown \$OWNER /mold/$dest.tar.gz"
