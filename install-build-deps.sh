#!/bin/bash -x
set -e
source /etc/os-release

# The first line for each distro installs a build dependency.
# The second line installs extra packages for `make test`.
#
# Feel free to send me a PR if you find a missing OS.

case "$ID-$VERSION_ID" in
ubuntu-20.* | pop-20.*)
  apt-get install -y cmake libssl-dev zlib1g-dev gcc g++ g++-10
  apt-get install -y file bsdmainutils
  ;;
ubuntu-* | pop-* | debian-* | raspbian-*)
  apt-get install -y cmake libssl-dev zlib1g-dev gcc g++
  apt-get install -y file bsdmainutils
  ;;
fedora-*)
  dnf install -y gcc-g++ cmake openssl-devel zlib-devel
  dnf install -y glibc-static file libstdc++-static diffutils
  ;;
opensuse-leap-*)
  zypper install -y make cmake zlib-devel libopenssl-devel gcc-c++ gcc11-c++
  zypper install -y glibc-devel-static tar diffutils util-linux
  ;;
opensuse-tumbleweed-*)
  zypper install -y make cmake zlib-devel libopenssl-devel gcc-c++
  zypper install -y glibc-devel-static tar diffutils util-linux
  ;;
gentoo-*)
  emerge dev-util/cmake sys-libs/zlib
  ;;
arch-*)
  pacman -S --needed --noconfirm base-devel zlib openssl cmake util-linux
  ;;
*)
  echo "Error: don't know anything about build dependencies on $ID-$VERSION_ID"
  exit 1
esac
