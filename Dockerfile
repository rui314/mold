# This dockerfile creates a reproducible build environment for mold.
#
# $ docker buildx create --use
# $ docker buildx build --platform linux/x86_64,linux/aarch64 -t rui314/mold-builder:latest --push .

FROM ubuntu:18.04
ENV TZ=Europe/London
RUN apt-get update && \
  apt-get install -y --no-install-recommends software-properties-common && \
  add-apt-repository -y ppa:ubuntu-toolchain-r/test && \
  apt-get update && \
  apt-get install -y --no-install-recommends build-essential wget \
    libstdc++-11-dev zlib1g-dev gcc-10 g++-10 python3 && \
  cd / && \
  wget -O- -q https://www.openssl.org/source/openssl-3.0.5.tar.gz | tar xzf - && \
  mv openssl-3.0.5 openssl && \
  cd openssl && \
  ./Configure --prefix=/usr/local && \
  make -j$(nproc) && \
  make -j$(nproc) install && \
  wget -O- -q https://github.com/Kitware/CMake/releases/download/v3.24.0/cmake-3.24.0-linux-$(uname -m).tar.gz | tar -C /usr/local --strip-components=1 -xzf -
