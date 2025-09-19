#!/bin/bash
set -e -x
cd "$(dirname $0)"/../..

if [ "$GITHUB_REPOSITORY" = '' ]; then
  image=mold-msan
  image_build="podman build -t $image -"
else
  # If this script is running on GitHub Actions, we want to cache
  # the created container image in GitHub's container repostiory.
  image=ghcr.io/$GITHUB_REPOSITORY/mold-msan
  image_build="podman build -t $image --output=type=registry --layers --cache-to $image --cache-from $image -"
fi

cat <<EOF | $image_build
FROM mirror.gcr.io/library/ubuntu:24.04
RUN apt-get update && \
  apt-get install -y --no-install-recommends build-essential gcc g++ wget ca-certificates make cmake ninja-build python3 clang-20 llvm-20-dev libclang-20-dev libclang-rt-20-dev libtbb-dev mold && \
  rm -rf /var/lib/apt/lists
RUN mkdir /llvm && \
  cd /llvm && \
  wget -O- --progress=dot:mega https://github.com/llvm/llvm-project/archive/refs/tags/llvmorg-21.1.1.tar.gz | tar xzf - --strip-components=1 && \
  cmake -S ./runtimes -B build -G Ninja -DCMAKE_BUILD_TYPE=Release -DLLVM_ENABLE_RUNTIMES='libcxx;libcxxabi;libunwind' -DCMAKE_C_COMPILER=clang-20 -DCMAKE_CXX_COMPILER=clang++-20 -DLLVM_USE_SANITIZER=MemoryWithOrigins && \
  cmake --build build -j\$(nproc) && \
  cmake --install build --prefix /msan && \
  rm -rf /llvm
EOF

# Build mold and run its tests
podman run -it --rm --pull=never -v "$(pwd):/mold:ro" --security-opt seccomp=unconfined $image bash -c "
set -e
mkdir /build
cd /build
cmake -GNinja -DCMAKE_BUILD_TYPE=Debug -DCMAKE_C_COMPILER=clang-20 -DCMAKE_CXX_COMPILER=clang++-20 -DMOLD_USE_MSAN=ON -DMOLD_STDLIB_PREFIX=/msan -DMOLD_USE_MIMALLOC=0 -DMOLD_USE_SYSTEM_TBB=1 -DMOLD_USE_MOLD=1 /mold
cmake --build . -j\$(nproc)
setarch -R ctest --output-on-failure -j\$(nproc)
"
