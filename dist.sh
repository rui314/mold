#!/bin/bash -x
# This script creates a mold binary distribution. The output is
# written in this directory as `mold-$version-$arch-linux.tar.gz`
# (e.g. `mold-1.0.3-x86_64-linux.tar.gz`).
#
# The mold executable created by this script is statically-linked to
# libstdc++ and libcrypto but dynamically-linked to libc, libm, libz
# and librt, as they almost always exist on any Linux systems.

set -e

# Unlike Linux, macOS's uname returns arm64 for aarch64.
arch=$(uname -m)
[ $arch = arm64 ] && arch=aarch64

if [ $arch != x86_64 -a $arch != aarch64 ]; then
  echo "Error: no docker image for $arch"
  exit 1
fi

version=$(grep '^VERSION =' $(dirname $0)/Makefile | sed 's/.* = //')
dest=mold-$version-$arch-linux
image=rui314/mold-builder:v1-$arch

docker images -q $image 2> /dev/null || docker pull $image

docker run -it --rm -v "$(pwd):/mold:Z" -u "$(id -u):$(id -g)" $image \
  bash -c "cp -r /mold /tmp/mold &&
cd /tmp/mold &&
make clean &&
make -j\$(nproc) CXX=clang++-14 CXXFLAGS='-I/openssl/include -O2 $CXXFLAGS' LDFLAGS='-static-libstdc++ /openssl/libcrypto.a' NEEDS_LIBCRYPTO=0 LTO=${LTO:-0} &&
make install PREFIX=/ DESTDIR=$dest &&
tar czf /mold/$dest.tar.gz $dest &&
cp mold /mold &&
cp mold-wrapper.so /mold"
