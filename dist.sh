#!/bin/bash -x
# This script creates a mold binary distribution. The output is
# written in this directory as `mold-$version-$arch-linux.tar.gz`
# (e.g. `mold-1.0.3-x86_64-linux.tar.gz`).
#
# The mold executable created by this script is statically-linked to
# libstdc++ and libcrypto but dynamically-linked to libc, libm, libz
# and librt, as they almost always exist on any Linux systems.

set -e

version=$(grep '^VERSION =' $(dirname $0)/Makefile | sed 's/.* = //')
arch=$(uname -m)
dest=mold-$version-$arch-linux

if [ $arch = x86_64 ]; then
  image=rui314/mold-builder:v1-x86_64
elif [ $arch = aarch64 -o $arch = arm64 ]; then
  image=rui314/mold-builder:v1-aarch64
else
  echo "Error: no docker image for $arch"
  exit 1
fi

docker images -q $image 2> /dev/null || docker pull $image

docker run -it --rm -v "$(pwd):/mold:Z" -u "$(id -u):$(id -g)" $image \
  bash -c "cp -r /mold /tmp/mold &&
cd /tmp/mold &&
make clean &&
make -j\$(nproc) CXX=clang++-14 CXXFLAGS='-I/openssl/include -O2 $CXXFLAGS' LDFLAGS='-static-libstdc++ /openssl/libcrypto.a' NEEDS_LIBCRYPTO=0 LTO=${LTO:-0} &&
make install PREFIX=/ DESTDIR=$dest &&
ln -sfr $dest/bin/mold $dest/libexec/mold/ld &&
tar czf /mold/$dest.tar.gz $dest &&
cp mold /mold"
