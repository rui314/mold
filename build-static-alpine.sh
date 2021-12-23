#!/bin/bash -x
# This is a shell script to build a statically-linked mold
# executable using Docker, so that it is easy to build mold
# on non-Ubuntu 20.04 machines.
#
# A docker image used for building mold is persistent.
# Run `docker image rm mold-build-ubuntu20` to remove the image
# from disk.

set -e


# Required so that users of non Docker desktop on macos can run this
export _BINARY="docker"

if [ -x "$(command -v lima)" ]; then
  echo 'lima is installed so using it instead' >&2
  _BINARY="lima nerdctl "
elif [ -x "$(command -v nerdctl)" ]; then
  echo 'nerdctl is installed so using it instead' >&2
  _BINARY="nerdctl "
fi

# If the existing file is not statically-linked, remove it.
[ -f mold ] && ! ldd mold 2>&1 | grep -q 'not a dynamic executable' && \
  rm mold

$_BINARY build -t mold-build-alpine-3.15 -f ./Dockerfiles/Dockerfile-static-alpine .

LDFLAGS='-fuse-ld=lld -static'

# libstdc++'s `std::__glibcxx_rwlock_rdlock` refers these symbols
# as weak symbols, although they need to be defined. Otherwise,
# the program crashes after juping to address 0.
# So, we force loading symbols as a workaround.
LDFLAGS="$LDFLAGS -Wl,-u,pthread_rwlock_rdlock"
LDFLAGS="$LDFLAGS -Wl,-u,pthread_rwlock_unlock"
LDFLAGS="$LDFLAGS -Wl,-u,pthread_rwlock_wrlock"

$_BINARY run -it --rm -v "`pwd`:/mold" -u $(id -u):$(id -g) \
  mold-build-alpine-3.15 \
  make -C /mold -j$(nproc) LDFLAGS="$LDFLAGS"
