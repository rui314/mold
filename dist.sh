#!/bin/bash
#
# This script creates a mold binary distribution. The output is
# written in this directory as `mold-$version-$arch-linux.tar.gz`
# (e.g. `mold-1.0.3-x86_64-linux.tar.gz`).
#
# The mold executable created by this script is statically-linked to
# libstdc++ and libcrypto but dynamically-linked to libc, libm, libz
# and librt, as they almost always exist on any Linux systems.

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

version=$(sed -n 's/^project(mold VERSION \(.*\))/\1/p' $(dirname $0)/CMakeLists.txt)
dest=mold-$version-$arch-linux
set -e -x

docker run --platform linux/$arch -i --rm -v "$(pwd):/mold" \
  -e "OWNER=$(id -u):$(id -g)" rui314/mold-builder-$arch:latest \
  bash -c "mkdir /build &&
cd /build &&
cmake -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_COMPILER=gcc-10 -DCMAKE_CXX_COMPILER=g++-10 -DMOLD_MOSTLY_STATIC=On /mold &&
cmake --build . -j\$(nproc) &&
[ $arch = arm ] || ctest -j\$(nproc) &&
cmake --install . --prefix $dest --strip &&
tar czf /mold/$dest.tar.gz $dest &&
chown \$OWNER /mold/mold /mold/mold-wrapper.so /mold/$dest.tar.gz"
