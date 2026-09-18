#!/bin/sh
# This script installs binary packages needed to test mold.
# Feel free to send me a PR if your OS is not on this list.

set -e
. /etc/os-release

set -x

case "$ID" in
ubuntu | pop | linuxmint | debian | raspbian | neon | zorin)
  apt-get update
  apt-get install -y curl gcc g++ clang gdb
  apt-get install -y qemu-user \
    gcc-i686-linux-gnu g++-i686-linux-gnu \
    gcc-aarch64-linux-gnu g++-aarch64-linux-gnu \
    gcc-riscv64-linux-gnu g++-riscv64-linux-gnu \
    gcc-powerpc-linux-gnu g++-powerpc-linux-gnu \
    gcc-powerpc64-linux-gnu g++-powerpc64-linux-gnu \
    gcc-powerpc64le-linux-gnu g++-powerpc64le-linux-gnu \
    gcc-s390x-linux-gnu g++-s390x-linux-gnu \
    gcc-sparc64-linux-gnu g++-sparc64-linux-gnu \
    gcc-m68k-linux-gnu g++-m68k-linux-gnu \
    gcc-sh4-linux-gnu g++-sh4-linux-gnu \
    gcc-arm-linux-gnueabihf g++-arm-linux-gnueabihf
  ;;
fedora | fedora-* | amzn | rhel | centos)
  dnf install -y curl gcc-c++ glibc-static libstdc++-static diffutils util-linux tar
  ;;
rocky | ol)
  dnf install -y curl gcc-c++ diffutils util-linux
  ;;
opensuse-*)
  zypper install -y bash curl gcc-c++ glibc-devel-static tar diffutils util-linux gawk
  ;;
gentoo)
  emerge-webrsync
  FEATURES='getbinpkg binpkg-request-signature' emerge net-misc/curl
  ;;
arch | archarm | artix | endeavouros | manjaro | cachyos)
  pacman -Sy --needed --noconfirm base-devel curl util-linux
  ;;
void)
  xbps-install -Sy xbps bash curl gcc tar diffutils util-linux
  ;;
alpine)
  apk update
  apk add bash curl linux-headers gcc g++
  ;;
clear-linux-os)
  swupd update
  swupd bundle-add c-basic diffutils
  ;;
almalinux)
  dnf install -y curl gcc-toolset-13-gcc-c++ gcc-toolset-13-libstdc++-devel diffutils
  ;;
altlinux)
  apt-get update
  apt-get install -y curl gcc-c++ diffutils util-linux
  ;;
freebsd)
  pkg update
  pkg install -y bash binutils curl gcc
  ;;
*)
  echo "Error: don't know anything about test dependencies on $ID-$VERSION_ID"
  exit 1
esac
