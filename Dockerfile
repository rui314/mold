# This dockerfile creates a reproducible build environment for mold.
#
# $ docker buildx create --use
# $ docker buildx build --platform x86_64,aarch64 -t rui314/mold-builder:latest --push .

FROM ubuntu:20.04
ENV TZ=Europe/London
RUN apt-get update && \
  DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends software-properties-common gpg-agent wget cmake build-essential g++ && \
  bash -c "$(wget -O - https://apt.llvm.org/llvm.sh) libc++-15-dev" && \
  mkdir /openssl && cd /openssl && \
  wget -O- -q https://www.openssl.org/source/openssl-3.0.7.tar.gz | tar --strip-components=1 -xzf - && \
  ./Configure --prefix=/usr/local && \
  make -j$(nproc) && \
  make -j$(nproc) install && \
  rm -rf /var/lib/apt/lists/* /openssl
