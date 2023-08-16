#!/bin/sh
set -e
. /etc/os-release

set -x

# The first line for each distro installs a build dependency.
# The second line installs extra packages for unittests.
#
# Feel free to send me a PR if your OS is not on this list.

case "$ID-$VERSION_ID" in
ubuntu-20.* | pop-20.*)
  apt-get update
  apt-get install -y cmake libssl-dev zlib1g-dev gcc g++ g++-10
  apt-get install -y file
  ;;
ubuntu-* | pop-* | linuxmint-* | debian-* | raspbian-*)
  apt-get update
  apt-get install -y cmake libssl-dev zlib1g-dev gcc g++
  apt-get install -y file
  ;;
fedora-*)
  dnf install -y gcc-g++ cmake zlib-devel
  dnf install -y glibc-static file libstdc++-static diffutils util-linux
  ;;
opensuse-leap-*)
  zypper install -y make cmake zlib-devel gcc-c++ gcc11-c++
  zypper install -y glibc-devel-static tar diffutils util-linux
  ;;
opensuse-tumbleweed-*)
  zypper install -y make cmake zlib-devel gcc-c++
  zypper install -y glibc-devel-static tar diffutils util-linux
  ;;
gentoo-*)
  emerge-webrsync
  emerge dev-util/cmake sys-libs/zlib
  ;;
arch-*)
  pacman -Sy
  pacman -S --needed --noconfirm base-devel zlib cmake util-linux
  ;;
void-*)
  xbps-install -Sy xbps
  xbps-install -Sy bash make cmake zlib-devel gcc
  xbps-install -Sy tar diffutils util-linux
  ;;
alpine-*)
  apk update
  apk add bash make linux-headers cmake zlib-dev gcc g++
  ;;
*)
  echo "Error: don't know anything about build dependencies on $ID-$VERSION_ID"
  exit 1
esac
