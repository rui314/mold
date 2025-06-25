#!/bin/bash
#
# This script creates a mold binary distribution. The output is written to
# the `dist` directory as `mold-$version-$arch-linux.tar.gz` (e.g.
# `mold-2.40.0-x86_64-linux.tar.gz`).
#
# The mold executable created by this script is statically linked to
# libstdc++, but dynamically linked to libc, libm, libpthread and some
# other libraries, as these libraries are almost always available on any
# Linux system. We can't statically link libc because doing so would
# disable dlopen(), which is required to load the LTO linker plugin.
#
# This script aims to produce reproducible outputs. That means if you run
# the script twice on the same git commit, it should produce bit-for-bit
# identical binary files. This property is crucial as a countermeasure
# against supply chain attacks. With it, you can verify that the binary
# files distributed on the GitHub release pages were created from the
# commit with release tags by rebuilding the binaries yourself.
#
# Debian provides snapshot.debian.org to host all historical binary
# packages. We use it to construct Podman images pinned to a
# particular timestamp. snapshot.debian.org is known to be very slow,
# but that shouldn't be a big problem for us because we only need that
# site the first time.
#
# We aim to use a reasonably old Debian version because we'll dynamically
# link glibc to mold, and a binary linked against a newer version of glibc
# won't work on a system with an older version of glibc.
#
# We prefer to build mold with Clang rather than GCC because mold's
# Identical Code Folding works best with the LLVM address significance
# table (.llvm_addrsig). Building a release binary with GCC produces a
# slightly larger binary than with Clang.
#
# We need a recent version of Clang to build mold. If it's not available
# via apt-get, we'll build it ourselves.
#
# This script can be used to create non-native binaries (e.g., building
# aarch64 binary on x86-64) because Podman automatically runs everything
# under QEMU if the container image is not native. To use this script for
# non-native builds, you may need to install the qemu-user-static package.

set -e -x
cd "$(dirname $0)"

usage() {
  echo "Usage: $0 [ x86_64 | aarch64 | arm | riscv64 | ppc64le | s390x | loongarch64 ]"
  exit 1
}

case $# in
0)
  arch=$(uname -m)
  if [ $arch = arm64 ]; then
    arch=aarch64
  elif [[ $arch = arm* ]]; then
    arch=arm
  fi
  ;;
1)
  arch="$1"
  ;;
*)
  usage
esac

# Create a Podman image.
if [ "$GITHUB_REPOSITORY" = '' ]; then
  image=mold-builder-$arch
  image_build="podman build --arch $arch -t $image -"
else
  # If this script is running on GitHub Actions, we want to cache
  # the created container image in GitHub's container repostiory.
  image=ghcr.io/$GITHUB_REPOSITORY/mold-builder-$arch
  image_build="podman build --arch $arch -t $image --output=type=registry --layers --cache-to $image --cache-from $image -"
fi

case $arch in
x86_64)
  # Debian 9 (Stretch) released in June 2017.
  #
  # We use a Google-provided mirror (gcr.io) of the official Docker hub
  # (docker.io) because docker.io has a strict rate limit policy.
  #
  # The toolchain in Debian 9 is too old to build mold, so we rebuild it
  # from source. We download source archives from official sites and build
  # them locally, rather than using pre-built binaries, to avoid relying
  # on unverifiable third-party binary blobs. Podman caches the result of
  # each RUN command, so rebuilding is done only once per host.
  cat <<EOF | $image_build
FROM mirror.gcr.io/library/debian:stretch@sha256:c5c5200ff1e9c73ffbf188b4a67eb1c91531b644856b4aefe86a58d2f0cb05be
ENV DEBIAN_FRONTEND=noninteractive TZ=UTC
RUN sed -i -e '/^deb/d' -e 's/^# deb /deb /g' /etc/apt/sources.list && \
  echo 'Acquire::Retries "10"; Acquire::http::timeout "10"; Acquire::Check-Valid-Until "false";' > /etc/apt/apt.conf.d/80-retries && \
  apt-get update && \
  apt-get install -y --no-install-recommends wget file make gcc g++ zlib1g-dev libssl-dev ca-certificates && \
  rm -rf /var/lib/apt/lists

# Build CMake 3.27
RUN mkdir /build && \
  cd /build && \
  wget -O- --progress=dot:mega https://cmake.org/files/v3.27/cmake-3.27.7.tar.gz | tar xzf - --strip-components=1 && \
  ./bootstrap --parallel=\$(nproc) && \
  make -j\$(nproc) && \
  make install && \
  rm -rf /build

# Build GCC 14
RUN mkdir /build && \
  cd /build && \
  wget -O- --progress=dot:mega https://ftpmirror.gnu.org/gcc/gcc-14.2.0/gcc-14.2.0.tar.gz | tar xzf - --strip-components=1 && \
  mkdir gmp mpc mpfr && \
  wget -O- --progress=dot:mega https://ftpmirror.gnu.org/gmp/gmp-6.3.0.tar.gz | tar xzf - --strip-components=1 -C gmp && \
  wget -O- --progress=dot:mega https://ftpmirror.gnu.org/mpc/mpc-1.3.1.tar.gz | tar xzf - --strip-components=1 -C mpc && \
  wget -O- --progress=dot:mega https://ftpmirror.gnu.org/mpfr/mpfr-4.2.1.tar.gz | tar xzf - --strip-components=1 -C mpfr && \
  ./configure --prefix=/usr --enable-languages=c,c++ --disable-bootstrap --disable-multilib && \
  make -j\$(nproc) && \
  make install && \
  ln -sf /usr/lib64/libstdc++.so.6 /usr/lib/x86_64-linux-gnu/libstdc++.so.6 && \
  rm -rf /build

# Build GNU binutils 2.43
RUN mkdir /build && \
  cd /build && \
  wget -O- --progress=dot:mega https://ftpmirror.gnu.org/binutils/binutils-2.43.tar.gz | tar xzf - --strip-components=1 && \
  ./configure --prefix=/usr && \
  make -j\$(nproc) && \
  make install && \
  rm -fr /build

# Build Python 3.12.7
RUN mkdir /build && \
  cd /build && \
  wget -O- --progress=dot:mega https://www.python.org/ftp/python/3.12.7/Python-3.12.7.tgz | tar xzf - --strip-components=1 && \
  ./configure && \
  make -j\$(nproc) && \
  make install && \
  rm -rf /build

# Build LLVM 20
RUN mkdir /build && \
  cd /build && \
  wget -O- --progress=dot:mega https://github.com/llvm/llvm-project/archive/refs/tags/llvmorg-20.1.3.tar.gz | tar xzf - --strip-components=1 && \
  mkdir b && \
  cd b && \
  cmake -DCMAKE_BUILD_TYPE=Release -DLLVM_ENABLE_PROJECTS=clang ../llvm && \
  cmake --build . -j\$(nproc) && \
  cmake --install . --strip && \
  rm -rf /build
EOF
  ;;
aarch64 | arm | ppc64le | s390x)
  # Debian 11 (Bullseye) released in August 2021
  #
  # We don't want to build Clang for these targets on QEMU becuase it
  # would take an extremely long time. Also, I believe old Linux boxes
  # are typically x86-64.
  cat <<EOF | $image_build
FROM mirror.gcr.io/library/debian:bullseye-20240904@sha256:8ccc486c29a3ad02ad5af7f1156e2152dff3ba5634eec9be375269ef123457d8
ENV DEBIAN_FRONTEND=noninteractive TZ=UTC
RUN sed -i -e '/^deb/d' -e 's/^# deb /deb /g' /etc/apt/sources.list && \
  echo 'Acquire::Retries "10"; Acquire::http::timeout "10"; Acquire::Check-Valid-Until "false";' > /etc/apt/apt.conf.d/80-retries && \
  apt-get update && \
  apt-get install -y --no-install-recommends build-essential gcc-10 g++-10 clang-16 cmake && \
  ln -sf /usr/bin/clang-16 /usr/bin/clang && \
  ln -sf /usr/bin/clang++-16 /usr/bin/clang++ && \
  rm -rf /var/lib/apt/lists
EOF
  ;;
riscv64)
  cat <<EOF | $image_build
FROM mirror.gcr.io/riscv64/debian:unstable-20240926@sha256:25654919c2926f38952cdd14b3300d83d13f2d820715f78c9f4b7a1d9399bf48
ENV DEBIAN_FRONTEND=noninteractive TZ=UTC
RUN sed -i -e '/^URIs/d' -e 's/^# http/URIs: http/' /etc/apt/sources.list.d/debian.sources && \
  echo 'Acquire::Retries "10"; Acquire::http::timeout "10"; Acquire::Check-Valid-Until "false";' > /etc/apt/apt.conf.d/80-retries && \
  apt-get update && \
  apt-get install -y --no-install-recommends build-essential gcc-14 g++-14 clang-18 cmake && \
  ln -sf /usr/bin/clang-18 /usr/bin/clang && \
  ln -sf /usr/bin/clang++-18 /usr/bin/clang++ && \
  rm -rf /var/lib/apt/lists
EOF
  ;;
loongarch64)
  cat <<EOF | $image_build
FROM mirror.gcr.io/loongarch64/debian:sid@sha256:0356df4e494bbb86bb469377a00789a5b42bbf67d5ff649a3f9721b745cbef77
ENV DEBIAN_FRONTEND=noninteractive TZ=UTC
RUN sed -i -e 's!http[^ ]*!http://snapshot.debian.org/archive/debian-ports/20250620T014755Z!g' /etc/apt/sources.list && \
  echo 'Acquire::Retries "10"; Acquire::http::timeout "10"; Acquire::Check-Valid-Until "false";' > /etc/apt/apt.conf.d/80-retries && \
  apt-get update && \
  apt-get install -y --no-install-recommends build-essential gcc-14 g++-14 clang-19 cmake && \
  ln -sf /usr/bin/clang-19 /usr/bin/clang && \
  ln -sf /usr/bin/clang++-19 /usr/bin/clang++ && \
  rm -rf /var/lib/apt/lists
EOF
  ;;
*)
  usage
  ;;
esac

version=$(sed -n 's/^project(mold VERSION \(.*\))/\1/p' CMakeLists.txt)
dest=mold-$version-$arch-linux

# Source tarballs available on GitHub don't contain .git history.
# Clone the repo if missing.
[ -d .git ] || git clone --branch v$version --depth 1 --bare https://github.com/rui314/mold .git

# We use the timestamp of the last Git commit as the file timestamp
# for build artifacts.
timestamp=$(git log -1 --format=%ct)

# `uname -m` in an ARM32 container running on an ARM64 host reports it
# not as ARM32 but as ARM64. That confuses BLAKE3's cmake script and
# erroneously enables NEON SIMD instructions. `setarch` can be used to
# change the output of `uname -m`.
setarch=
[ $arch = arm ] && setarch='setarch linux32'

mkdir -p dist

# Build mold in a container.
#
# SOURCE_DATE_EPOCH is a standardized environment variable that allows
# build artifacts to appear as if they were built at a specific time.
# We use it to control how the compiler expands the C/C++ __DATE__ and
# __TIME__ macros.
podman run --arch $arch -it --rm --userns=host --pids-limit=-1 --network=none \
  --pull=never -v "$(pwd):/mold:ro" -v "$(pwd)/dist:/dist" $image \
   $setarch bash -c "
set -e
export SOURCE_DATE_EPOCH=$timestamp
mkdir /build
cd /build
cmake -DCMAKE_BUILD_TYPE=Release -DMOLD_MOSTLY_STATIC=1 -DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++ /mold
cmake --build . -j\$(nproc)
cmake --install .
cmake -DMOLD_USE_MOLD=1 .
cmake --build . -j\$(nproc)
ctest --output-on-failure -j\$(nproc)
cmake --install . --prefix $dest --strip
find $dest -print | xargs touch --no-dereference --date=@$timestamp
find $dest -print | sort | tar -cf - --no-recursion --files-from=- | gzip -9nc > /dist/$dest.tar.gz
cp mold /dist
sha256sum /dist/$dest.tar.gz
"
