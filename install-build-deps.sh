#!/bin/bash -x
set -e
source /etc/os-release

# The first line for each distro installs a build dependency.
# The second line installs extra packages for `make test`.
test_id_version() {
  case "$1$2" in
  ubuntu20.*)
    apt-get install -y git cmake libssl-dev zlib1g-dev gcc g++ g++-10
    apt-get install -y file bsdmainutils
    ;;
  ubuntu22.* | debian11)
    apt-get install -y git cmake libssl-dev zlib1g-dev gcc g++
    apt-get install -y file bsdmainutils
    ;;
  fedora*)
    dnf install -y git gcc-g++ cmake openssl-devel zlib-devel
    dnf install -y glibc-static file libstdc++-static diffutils
    ;;
  opensuse-leap*)
    zypper install -y git make cmake zlib-devel libopenssl-devel gcc-c++ gcc11-c++
    zypper install -y glibc-devel-static tar diffutils util-linux
    ;;
  opensuse-tumbleweed*)
    zypper install -y git make cmake zlib-devel libopenssl-devel gcc-c++
    zypper install -y glibc-devel-static tar diffutils util-linux
    ;;
  gentoo*)
    emerge dev-vcs/git dev-util/cmake sys-libs/zlib
    ;;
  arch*)
    pacman -S --needed --noconfirm base-devel zlib openssl cmake util-linux git
    ;;
  *)
    echo "Error: don't know anything about build dependencies on $1 $2"
    return 1
    ;;
  esac
}

if ! test_id_version "$ID" "$VERSION_ID" ; then
  echo "Testing related OS"
  if ! test_id_version "${ID_LIKE%% *}" "$VERSION_ID" ; then
    echo "Error: Failed to find matching dependencies for the system"
    exit 1
  fi
fi
