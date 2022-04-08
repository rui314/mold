#!/bin/bash -x
# This script creates a mold executable suitable for binary distribution.
# The output is written in this directory as `mold-$version-$arch-linux.tar.gz`
# (e.g. `mold-1.0.3-x86_64-linux.tar.gz`).
#
# The mold executable created by this script is statically-linked to
# libstdc++ and libcrypto but dynamically-linked to libc, libm, libz
# and librt, as they almost always exist on any Linux systems.

set -e

ver=$(grep '^VERSION =' $(dirname $0)/Makefile | sed 's/.* = //')
dest=mold-$ver-$(uname -m)-linux

cat <<'EOF' | docker build -t mold-build-ubuntu18 -
FROM ubuntu:18.04
RUN apt-get update && \
  TZ=Europe/London apt-get install -y tzdata && \
  apt-get install -y --no-install-recommends software-properties-common && \
  add-apt-repository -y ppa:ubuntu-toolchain-r/test && \
  apt-get install -y --no-install-recommends build-essential git \
    wget pkg-config cmake libstdc++-11-dev zlib1g-dev gpg gpg-agent && \
  bash -c "$(wget -O- https://apt.llvm.org/llvm.sh)" && \
  apt-get install -y --no-install-recommends clang-14 && \
  apt clean && \
  rm -rf /var/lib/apt/lists/* && \
  cd / && \
  wget -O- https://www.openssl.org/source/openssl-3.0.2.tar.gz | tar xzf - && \
  mv openssl-3.0.2 openssl && \
  cd openssl && \
  ./Configure --prefix=/openssl && \
  make -j$(nproc)
EOF

docker run -it --rm -v "$(pwd):/mold:Z" -u "$(id -u):$(id -g)" \
  mold-build-ubuntu18 \
  bash -c "cp -r /mold /tmp/mold &&
cd /tmp/mold &&
make clean &&
make -j$(nproc) CXX=clang++-14 CXXFLAGS='-I/openssl/include -O2' LDFLAGS='-static-libstdc++ /openssl/libcrypto.a' NEEDS_LIBCRYPTO=0 &&
make install PREFIX=/ DESTDIR=$dest &&
ln -sfr $dest/bin/mold $dest/libexec/mold/ld &&
tar czf /mold/$dest.tar.gz $dest &&
cp mold /mold"
