#!/bin/sh
# This script installs binary packages needed to build mold.
# Feel free to send me a PR if your OS is not on this list.

set -e
. /etc/os-release

set -x

case "$ID-$VERSION_ID" in
ubuntu-20.* | pop-20.*)
  apt-get update
  apt-get install -y cmake gcc g++ g++-10
  ;;
ubuntu-* | pop-* | linuxmint-* | debian-* | raspbian-*)
  apt-get update
  apt-get install -y cmake gcc g++
  ;;
fedora-* | amzn-* | rhel-*)
  dnf install -y gcc-g++ cmake glibc-static libstdc++-static diffutils util-linux
  ;;
opensuse-leap-*)
  zypper install -y make cmake gcc-c++ gcc11-c++ glibc-devel-static tar diffutils util-linux
  ;;
opensuse-tumbleweed-*)
  zypper install -y make cmake gcc-c++ glibc-devel-static tar diffutils util-linux
  ;;
gentoo-*)
  emerge-webrsync
  emerge dev-build/cmake
  ;;
arch-* | archarm-* | artix-* | endeavouros-*)
  pacman -Sy --needed --noconfirm base-devel cmake util-linux
  ;;
void-*)
  xbps-install -Sy xbps bash make cmake gcc tar diffutils util-linux
  ;;
alpine-*)
  apk update
  apk add bash make linux-headers cmake gcc g++
  ;;
clear-linux-*)
  swupd update
  swupd bundle-add c-basic diffutils
  ;;
*)
  echo "Error: don't know anything about build dependencies on $ID-$VERSION_ID"
  exit 1
esac
