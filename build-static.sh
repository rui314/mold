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
  apt-get install -y build-essential git clang lld cmake ninja-build \
    libstdc++-10-dev libxxhash-dev zlib1g-dev libssl-dev && \
  rm -rf /var/lib/apt/lists/*
EOF

rm -rf out/build-static

docker run -it --rm -v `pwd`:/mold -u $(id -u):$(id -g) \
  mold-build-ubuntu20 bash -c \
  'cmake -GNinja -DCMAKE_CXX_COMPILER=clang++ -DCMAKE_BUILD_TYPE=Release -DMOLD_BUILD_STATIC_EXE=On -DCMAKE_EXE_LINKER_FLAGS=-fuse-ld=lld -S /mold -B /mold/out/build-static; ninja -C /mold/out/build-static'
