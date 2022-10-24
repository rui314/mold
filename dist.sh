#!/bin/bash
# This script creates a mold binary distribution. The output is
# written in this directory as `mold-$version-$arch-linux.tar.gz`
# (e.g. `mold-1.0.3-x86_64-linux.tar.gz`).
#
# The mold executable created by this script is statically-linked to
# libstdc++ and libcrypto but dynamically-linked to libc, libm, libz
# and librt, as they almost always exist on any Linux systems.

if [ $# -ne 1 ]; then
  echo "Usage: $0 [ x86_64 | aarch64 | arm | ppc64le | s390x ]"
  exit 1
fi

arch=$1
echo $arch | grep -Eq '^(x86_64|aarch64|arm|ppc64le|s390x)$' || \
  { echo "Error: no docker image for $arch"; exit 1; }

version=$(sed -n 's/^project(mold VERSION \(.*\))/\1/p' $(dirname $0)/CMakeLists.txt)
dest=mold-$version-$arch-linux
set -e -x

docker run --platform linux/$arch -it --rm -v "$(pwd):/mold" \
  -e "OWNER=$(id -u):$(id -g)" rui314/mold-builder:latest \
  bash -c "mkdir /tmp/build &&
cd /tmp/build &&
cmake -DCMAKE_C_COMPILER=gcc-10 -DCMAKE_CXX_COMPILER=g++-10 -DMOLD_MOSTLY_STATIC=On -DCMAKE_BUILD_TYPE=Release /mold &&
cmake --build . -j\$(nproc) &&
cmake --install . --prefix $dest --strip &&
tar czf /mold/$dest.tar.gz $dest &&
cp mold mold-wrapper.so /mold &&
chown \$OWNER /mold/mold /mold/mold-wrapper.so /mold/$dest.tar.gz"
