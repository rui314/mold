# This dockerfile creates a reproducible build environment for mold.
#
# $ docker buildx create --use
# $ docker buildx build --platform x86_64,aarch64,arm,ppc64le,s390x -t rui314/mold-builder:latest --push .

FROM ubuntu:18.04
ENV TZ=Europe/London
RUN apt-get update && \
  apt-get install -y --no-install-recommends software-properties-common && \
  add-apt-repository -y ppa:ubuntu-toolchain-r/test && \
  apt-get update && \
  apt-get install -y --no-install-recommends build-essential wget libstdc++-11-dev zlib1g-dev gcc-10 g++-10 python3 && \
  \
  mkdir /openssl && cd /openssl && \
  wget -O- -q https://www.openssl.org/source/openssl-3.0.7.tar.gz | tar --strip-components=1 -xzf - && \
  ./Configure --prefix=/usr/local --libdir=lib && \
  make -j$(nproc) && \
  make -j$(nproc) install && \
  ldconfig && \
  mkdir /cmake && cd /cmake && \
  wget -O- -q https://github.com/Kitware/CMake/releases/download/v3.24.2/cmake-3.24.2.tar.gz | tar --strip-components=1 -xzf - && \
  ./bootstrap --parallel=$(nproc) && make -j$(nproc) && make -j$(nproc) install && \
  rm -rf /var/lib/apt/lists/* /cmake /openssl
