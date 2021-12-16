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

LDFLAGS='-fuse-ld=lld -static'

# libstdc++'s `std::__glibcxx_rwlock_rdlock` refers these symbols
# as weak symbols, although they need to be defined. Otherwise,
# the program crashes after juping to address 0.
# So, we force loading symbols as a workaround.
LDFLAGS="$LDFLAGS -Wl,-u,pthread_rwlock_rdlock"
LDFLAGS="$LDFLAGS -Wl,-u,pthread_rwlock_unlock"
LDFLAGS="$LDFLAGS -Wl,-u,pthread_rwlock_wrlock"

docker run -it --rm -v "`pwd`:/mold" -u $(id -u):$(id -g) \
  mold-build-ubuntu20 \
  make -C /mold -j$(nproc) LDFLAGS="$LDFLAGS"
