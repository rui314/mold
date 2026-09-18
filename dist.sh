#!/bin/bash
#
# This script creates a mold binary distribution. The output is written to
# the `dist` directory as `mold-$version-$arch-linux.tar.gz` (e.g.
# `mold-2.42.0-x86_64-linux.tar.gz`).
#
# This script aims to produce reproducible outputs. The container images,
# Rust toolchain, Cargo dependencies and file timestamps are pinned so that
# the same git commit can produce a bit-for-bit identical binary file. This
# property serves as a strong safeguard against supply chain attacks. With
# a reproducible build, anyone can independently verify that the binary
# files published on our GitHub release page were built from the git commit
# tagged for release by rebuilding the binaries themselves.
#
# Debian provides snapshot.debian.org to host all historical binary
# packages. Distro package repositories must be pinned as well as the base
# image before a build is fully reproducible. The loongarch64 build still
# uses the live Debian sid repository, so it does not yet have that property.
#
# The mold executable created by this script is dynamically linked to the
# system C runtime and other standard system libraries. We can't statically
# link glibc because doing so would disable dlopen(), which is required to
# load the LTO linker plugin.
#
# We use a reasonably old Debian version for the build environment because
# a binary dynamically linked against a newer version of glibc won't work
# on a system with an older version of glibc.
#
# The Rust toolchain is downloaded from the official Rust distribution
# site. Its version and SHA-256 hash are recorded below, so the toolchain
# is another pinned build input.
#
# This script can be used to create non-native binaries (e.g., building
# aarch64 binary on x86-64) because Podman automatically runs everything
# under QEMU if the container image is not native. To use this script for
# non-native builds, you may need to install the qemu-user-static package.

set -e -x
cd "$(dirname "$0")"

usage() {
  echo "Usage: $0 [ x86_64 | aarch64 | arm | riscv64 | ppc64le | s390x | loongarch64 ]"
  exit 1
}

case $# in
0)
  arch=$(uname -m)
  if [ "$arch" = arm64 ]; then
    arch=aarch64
  elif [[ "$arch" = arm* ]]; then
    arch=arm
  fi
  ;;
1)
  arch=$1
  ;;
*)
  usage
  ;;
esac

rust_version=1.97.1

# Switch to the pinned snapshot.debian.org sources that the Debian images
# ship commented out; the live mirrors no longer carry every architecture.
apt_setup="sed -i -e '/^deb/d' -e 's/^# deb /deb /g' /etc/apt/sources.list"

case $arch in
x86_64)
  # Debian 9 (Stretch) released in June 2017.
  #
  # We use a Google-provided mirror (mirror.gcr.io) instead of the official
  # Docker Hub (docker.io) because docker.io has a strict rate limit policy.
  base_image=mirror.gcr.io/library/debian:stretch@sha256:c5c5200ff1e9c73ffbf188b4a67eb1c91531b644856b4aefe86a58d2f0cb05be
  rust_target=x86_64-unknown-linux-gnu
  rust_sha256=b4cdbc7cc6b0ee0a2666b1872769fdb2ad8393b28b63952f6493b4b400e4832b
  ;;
aarch64)
  # Debian 11 (Bullseye) released in August 2021.
  base_image=mirror.gcr.io/library/debian:bullseye-20240904@sha256:8ccc486c29a3ad02ad5af7f1156e2152dff3ba5634eec9be375269ef123457d8
  rust_target=aarch64-unknown-linux-gnu
  rust_sha256=2f2496c70bd336a66a4c8baf2d303ba161f3552f192444c3639ba903c7c1e2c5
  ;;
arm)
  # Debian 11 (Bullseye) released in August 2021.
  base_image=mirror.gcr.io/library/debian:bullseye-20240904@sha256:8ccc486c29a3ad02ad5af7f1156e2152dff3ba5634eec9be375269ef123457d8
  rust_target=armv7-unknown-linux-gnueabihf
  rust_sha256=e89c5e33aaddc6ef56857000c9117875c2997e9a1a500bd7b16277c9874b002f
  ;;
riscv64)
  base_image=mirror.gcr.io/riscv64/debian:unstable-20240926@sha256:25654919c2926f38952cdd14b3300d83d13f2d820715f78c9f4b7a1d9399bf48
  apt_setup="sed -i -e '/^URIs/d' -e 's/^# http/URIs: http/' /etc/apt/sources.list.d/debian.sources"
  rust_target=riscv64gc-unknown-linux-gnu
  rust_sha256=59bec35d8febb2ab918fa41cffbaa5b07146a63bdc33f029ff756d70a3151ece
  ;;
ppc64le)
  # Debian 11 (Bullseye) released in August 2021.
  base_image=mirror.gcr.io/library/debian:bullseye-20240904@sha256:8ccc486c29a3ad02ad5af7f1156e2152dff3ba5634eec9be375269ef123457d8
  rust_target=powerpc64le-unknown-linux-gnu
  rust_sha256=ff524eef5a59d801df09ccad5cdaf9ea1f0a07d75cbed2a7e9f013a9eb76a3c1
  ;;
s390x)
  # Debian 11 (Bullseye) released in August 2021.
  base_image=mirror.gcr.io/library/debian:bullseye-20240904@sha256:8ccc486c29a3ad02ad5af7f1156e2152dff3ba5634eec9be375269ef123457d8
  rust_target=s390x-unknown-linux-gnu
  rust_sha256=808268af9e880d41b8cb32b242e38c9bd3ea7aba6409b02fbffa0fbc5370c538
  ;;
loongarch64)
  base_image=mirror.gcr.io/loongarch64/debian:sid@sha256:0356df4e494bbb86bb469377a00789a5b42bbf67d5ff649a3f9721b745cbef77
  apt_setup="echo 'deb http://deb.debian.org/debian sid main' > /etc/apt/sources.list"
  rust_target=loongarch64-unknown-linux-gnu
  rust_sha256=d5a925962854730ae7641420d8337af93988ea4ff47b503a856ec53776c87841
  ;;
*)
  usage
  ;;
esac

# Create a Podman image containing the native C tools and a pinned Rust
# toolchain. The downloaded archive is checked before it is unpacked.
image=mold-rust-builder-$arch
archive=rust-$rust_version-$rust_target.tar.gz

podman build --arch "$arch" -t "$image" - <<EOF
FROM $base_image
ENV DEBIAN_FRONTEND=noninteractive TZ=UTC
RUN $apt_setup && \
  echo 'Acquire::Retries "10"; Acquire::http::timeout "10"; Acquire::Check-Valid-Until "false";' > /etc/apt/apt.conf.d/80-retries && \
  apt-get update && \
  apt-get install -y --no-install-recommends build-essential ca-certificates git wget && \
  rm -rf /var/lib/apt/lists
RUN mkdir /tmp/rust && \
  cd /tmp/rust && \
  wget --progress=dot:mega https://static.rust-lang.org/dist/$archive && \
  echo '$rust_sha256 $archive' | sha256sum -c && \
  tar xf $archive && \
  ./${archive%.tar.gz}/install.sh --prefix=/usr/local --disable-ldconfig && \
  cd / && \
  rm -rf /tmp/rust
EOF

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml)
dest=mold-$version-$arch-linux

# Source tarballs available on GitHub don't contain .git directory.
# Clone the repo if missing.
[ -d .git ] || git clone --branch v$version --depth 1 --bare https://github.com/rui314/mold .git

# We use the timestamp of the last Git commit as the file timestamp
# for build artifacts.
timestamp=$(git log -1 --format=%ct)

# `uname -m` in an ARM32 container running on an ARM64 host reports it
# not as ARM32 but as ARM64. Keep the reported machine consistent with
# the ARM32 userspace for any build tool that inspects it.
setarch=
[ "$arch" = arm ] && setarch='setarch linux32'

mkdir -p dist "target/dist-vendor-$arch"

# Cargo verifies registry packages against the checksums in Cargo.lock and
# checks out Git dependencies at the commit recorded there. Vendor them in a
# separate networked step so that the actual build can run without a network.
# Cargo's own cache is a tmpfs because the libgit2 inside a 32-bit cargo can't
# read a bind-mounted ext4 directory (readdir fails with EOVERFLOW under QEMU).
podman run --arch "$arch" -it --rm --userns=host --pids-limit=-1 \
  --pull=never --env CARGO_HOME=/cargo --tmpfs /cargo -v "$(pwd):/mold:ro" \
  -v "$(pwd)/target/dist-vendor-$arch:/vendor" "$image" $setarch \
  bash -c 'cd /mold && cargo vendor --locked /vendor/sources > /vendor/config.toml'

# Build mold in a container.
#
# SOURCE_DATE_EPOCH is a standardized environment variable that allows
# build artifacts to appear as if they were built at a specific time.
# Fixed source, vendor and target paths keep embedded build paths stable.
podman run --arch "$arch" -it --rm --userns=host --pids-limit=-1 --network=none \
  --pull=never --env SOURCE_DATE_EPOCH="$timestamp" --env DEST="$dest" \
  -v "$(pwd):/mold:ro" -v "$(pwd)/dist:/dist" \
  -v "$(pwd)/target/dist-vendor-$arch:/vendor:ro" "$image" \
  $setarch bash -c '
set -e
export CARGO_TARGET_DIR=/build/target
cd /mold
cargo build --release --frozen --config /vendor/config.toml --package mold-cli
stage=/build/$DEST
DESTDIR=/build PREFIX=/$DEST ./install-mold.sh
strip --strip-unneeded "$stage/bin/mold" "$stage/lib/mold/mold-wrapper.so"
find "$stage" -print | xargs touch --no-dereference --date="@$SOURCE_DATE_EPOCH"
cd /build
find "$DEST" -print | sort | tar -cf - --no-recursion --files-from=- | gzip -9nc > "/dist/$DEST.tar.gz"
sha256sum "/dist/$DEST.tar.gz"
'
