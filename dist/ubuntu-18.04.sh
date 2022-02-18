#!/bin/bash -x
set -e

ver=$(grep '^VERSION =' $(dirname $0)/../Makefile | sed 's/.* = //')

cat <<'EOF' | docker build -t mold-build-ubuntu18 -
FROM ubuntu:18.04
RUN apt-get update && \
  TZ=Europe/London apt-get install -y tzdata && \
  apt-get install -y --no-install-recommends software-properties-common && \
  add-apt-repository -y ppa:ubuntu-toolchain-r/test && \
  apt-get install -y --no-install-recommends build-essential bsdmainutils file \
    gcc-multilib git wget pkg-config cmake libstdc++-10-dev zlib1g-dev \
    libssl-dev gpg gpg-agent && \
  bash -c "$(wget -O - https://apt.llvm.org/llvm.sh)" && \
  apt-get install -y --no-install-recommends clang-14 && \
  apt clean && \
  rm -rf /var/lib/apt/lists/*
EOF

docker run -it --rm -v "$(pwd):/mold:Z" -u "$(id -u):$(id -g)" \
  mold-build-ubuntu18 \
  bash -c "cp -r /mold /tmp/mold &&
cd /tmp/mold &&
make clean &&
make -j$(nproc) CXX=clang++-14 &&
make install DESTDIR=mold-$ver-ubuntu-18.04 &&
tar czf /mold/mold-$ver-ubuntu-18.04.tar.gz mold-$ver-ubuntu-18.04"
