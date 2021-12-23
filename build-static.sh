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
  apt-get install -y --no-install-recommends build-essential clang lld \
    bsdmainutils file gcc-multilib \
    cmake libstdc++-10-dev zlib1g-dev libssl-dev && \
  apt clean && \
  rm -rf /var/lib/apt/lists/*
EOF

EXTRA_LDFLAGS='-fuse-ld=lld -static'

# libstdc++'s `std::__glibcxx_rwlock_rdlock` refers these symbols
# as weak symbols, although they need to be defined. Otherwise,
# the program crashes after juping to address 0.
# So, we force loading symbols as a workaround.
EXTRA_LDFLAGS="$EXTRA_LDFLAGS -Wl,-u,pthread_rwlock_rdlock"
EXTRA_LDFLAGS="$EXTRA_LDFLAGS -Wl,-u,pthread_rwlock_unlock"
EXTRA_LDFLAGS="$EXTRA_LDFLAGS -Wl,-u,pthread_rwlock_wrlock"

docker_args=(-v "`pwd`:/mold:Z" -u "$(id -u)":"$(id -g)")
if docker --version | grep -q podman; then
  docker_args+=(--userns=keep-id)
fi

docker run -it --rm "${docker_args[@]}" \
  mold-build-ubuntu20 \
  make -C /mold -j$(nproc) EXTRA_LDFLAGS="$EXTRA_LDFLAGS"
