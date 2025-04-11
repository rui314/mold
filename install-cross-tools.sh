#!/bin/bash
set -e
. /etc/os-release

set -x

# This script install packages for -DMOLD_ENABLE_QEMU_TESTS=1
# to enable cross-target tests.
#
# Feel free to send me a PR if your OS is not on this list.

case "$ID" in
ubuntu | pop | linuxmint | debian | raspbian)
  apt-get install -y qemu-user {gcc,g++}-{i686,aarch64,riscv64,powerpc,powerpc64,powerpc64le,s390x,sparc64,m68k,sh4,arc}-linux-gnu {gcc,g++}-arm-linux-gnueabihf
  ;;
*)
  echo "Error: don't know anything about build dependencies on $ID-$VERSION_ID"
  exit 1
esac
