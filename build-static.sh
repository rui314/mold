#!/bin/bash -x
# This is a shell script to build a statically-linked mold
# executable using Docker, so that it is easy to build mold
# on non-Ubuntu 20.04 machines.
#
# A docker image used for building mold is persistent.
# Run `docker image rm mold-build-ubuntu20` to remove the image
# from disk.

set -e

# If the existing file is not statically-linked, remove it.
[ -f mold ] && ! ldd mold 2>&1 | grep -q 'not a dynamic executable' && \
  rm mold

cat <<EOF | docker build -t mold-build-ubuntu20 -
FROM ubuntu:20.04
RUN apt-get update && \
  TZ=Europe/London apt-get install -y tzdata && \
  apt-get install -y build-essential git clang lld cmake \
    libstdc++-10-dev libxxhash-dev zlib1g-dev libssl-dev && \
  rm -rf /var/lib/apt/lists/*
EOF

if ! [ -d oneTBB ]; then
  git clone https://github.com/oneapi-src/oneTBB.git
  (cd oneTBB; git checkout --quiet v2020.3)
fi

docker run -it --rm -v `pwd`/oneTBB:/oneTBB -u $(id -u):$(id -g) \
  mold-build-ubuntu20 \
  make -C /oneTBB -j$(nproc) extra_inc=big_iron.inc

tbb_bindir=$(ls -d oneTBB/build/linux_intel64_*_release)

docker run -it --rm -v `pwd`:/mold -u $(id -u):$(id -g) \
  mold-build-ubuntu20 \
  make -C /mold -j$(nproc) \
       EXTRA_CPPFLAGS=-IoneTBB/include \
       EXTRA_LDFLAGS="-fuse-ld=lld -static -L$tbb_bindir"
