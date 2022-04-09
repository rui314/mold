# This dockerfile creates a reproducible build environment for mold.
#
# $ docker build -t rui314/mold-builder:v1-$(uname -m) .
# $ docker push rui314/mold-builder:v1-$(uname -m)

FROM ubuntu:18.04
RUN apt-get update && \
  TZ=Europe/London apt-get install -y tzdata && \
  apt-get install -y --no-install-recommends software-properties-common && \
  add-apt-repository -y ppa:ubuntu-toolchain-r/test && \
  apt-get install -y --no-install-recommends build-essential git \
    wget pkg-config cmake libstdc++-11-dev zlib1g-dev gpg gpg-agent && \
  bash -c "$(wget -O- https://apt.llvm.org/llvm.sh)" && \
  apt-get install -y --no-install-recommends clang-14 && \
  apt clean && \
  rm -rf /var/lib/apt/lists/* && \
  cd / && \
  wget -O- https://www.openssl.org/source/openssl-3.0.2.tar.gz | tar xzf - && \
  mv openssl-3.0.2 openssl && \
  cd openssl && \
  ./Configure --prefix=/openssl && \
  make -j$(nproc)
