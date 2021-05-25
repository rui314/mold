#!/bin/bash
#
# This test script builds lots of programs using mold in a Docker
# environment to make sure that mold can build programs in the wild.
# We are using Gentoo Linux as a build environment, because its
# source-based package allows us to build programs without any hassle.

if [ "$1" = "" ]; then
  echo "Usage: $0 gentoo-package-name"
  exit 1
fi

set -x

# Create a Docker image
if ! docker image ls mold-gentoo | grep -q mold-gentoo; then
  set -e
  cat <<EOF | docker build -t mold-gentoo -
FROM gentoo/stage3
RUN echo 'FEATURES="${FEATURE} noclean nostrip -ipc-sandbox -network-sandbox -pid-sandbox -sandbox"' >> /etc/portage/make.conf
RUN emerge-webrsync
EOF
  set +e
fi

# Build mold as a statically-linked executable
if ! [ -f mold ] || ! ldd mold 2>&1 | grep -q 'not a dynamic executable'; then
  make clean
  ./build-static.sh
fi

# Build a given package in Docker
run() {
  package="$1"
  cmd="FEATURES=test MAKEOPTS=-j8 emerge $package"
  filename=`echo "$package" | sed 's!/!_!g'`
  link="ln -sf /mold/mold /usr/x86_64-pc-linux-gnu/bin/ld"

  mkdir -p gentoo/success gentoo/failure
  docker run --rm --cap-add=SYS_PTRACE -v `pwd`:/mold mold-gentoo \
    bash -c "$cmd" > gentoo/$filename.ld

  if [ $? = 0 ]; then
    mv gentoo/$filename.ld gentoo/success
  else
    mv gentoo/$filename.ld gentoo/failure
  fi

  docker run --rm --cap-add=SYS_PTRACE -v `pwd`:/mold mold-gentoo \
    bash -c "$link; $cmd" > gentoo/$filename.mold

  if [ $? = 0 ]; then
    mv gentoo/$filename.mold gentoo/success
  else
    mv gentoo/$filename.mold gentoo/failure
  fi
}

# Run a test
run "$1"
