#!/bin/bash
# This script installs binary packages needed to test mold.
# Feel free to send me a PR if your OS is not on this list.

set -e
. /etc/os-release

set -x

case "$ID" in
ubuntu | pop | linuxmint | debian | raspbian | neon | zorin)
  apt-get update
  apt-get install -y gcc g++ clang gdb
  apt-get install -y qemu-user {gcc,g++}-{i686,aarch64,riscv64,powerpc,powerpc64,powerpc64le,s390x,sparc64,m68k,sh4}-linux-gnu {gcc,g++}-arm-linux-gnueabihf
  ;;
fedora | fedora-* | amzn | rhel | centos)
  dnf install -y gcc-c++ glibc-static libstdc++-static diffutils util-linux tar
  ;;
rocky | ol)
  dnf install -y gcc-c++ diffutils util-linux
  ;;
opensuse-*)
  zypper install -y gcc-c++ glibc-devel-static tar diffutils util-linux gawk
  ;;
gentoo)
  :
  ;;
arch | archarm | artix | endeavouros | manjaro | cachyos)
  pacman -Sy --needed --noconfirm base-devel util-linux
  ;;
void)
  xbps-install -Sy xbps bash gcc tar diffutils util-linux
  ;;
alpine)
  apk update
  apk add bash linux-headers gcc g++
  ;;
clear-linux-os)
  swupd update
  swupd bundle-add c-basic diffutils
  ;;
almalinux)
  dnf install -y gcc-toolset-13-gcc-c++ gcc-toolset-13-libstdc++-devel diffutils
  ;;
altlinux)
  apt-get update
  apt-get install -y gcc-c++ diffutils util-linux
  ;;
freebsd)
  pkg update
  pkg install -y bash binutils gcc
  ;;
*)
  echo "Error: don't know anything about test dependencies on $ID-$VERSION_ID"
  exit 1
esac
