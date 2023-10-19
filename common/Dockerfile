# This dockerfile creates a reproducible build environment for mold.
#
# $ docker buildx create --use
# $ docker buildx build --platform x86_64,aarch64,arm,ppc64le,s390x -t rui314/mold-builder:latest --push - < common/Dockerfile

FROM ubuntu:18.04
RUN apt-get update && \
  apt-get install -y --no-install-recommends software-properties-common && \
  add-apt-repository -y ppa:ubuntu-toolchain-r/test && \
  apt-get update && \
  DEBIAN_FRONTEND=noninteractive TZ=UTC apt-get install -y --no-install-recommends build-essential wget libstdc++-11-dev gcc-10 g++-10 python3 libssl-dev && \
  \
  mkdir /cmake && cd /cmake && \
  wget -O- -q https://github.com/Kitware/CMake/releases/download/v3.24.2/cmake-3.24.2.tar.gz | tar --strip-components=1 -xzf - && \
  ./bootstrap --parallel=$(nproc) && make -j$(nproc) && make -j$(nproc) install && \
  rm -rf /var/lib/apt/lists /cmake
