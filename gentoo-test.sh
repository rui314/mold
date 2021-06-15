#!/bin/bash
#
# This test script takes a Gentoo package name and tries to build it
# using mold in a Docker environment. We chose Gentoo Linux as a test
# target, because its source-based package allows us to build programs
# locally and run their test suites without any hassle.
#
# You can get a complete list of Gentoo packages availalbe for testing
# with the following command:
#
# docker run --rm mold-gentoo emerge --color n -s '' | \
#   perl -ne 'next unless m!^\*\s+(\S+/\S+)!; print "$1\n"'

if [ "$1" = "" ]; then
  echo "Usage: $0 gentoo-package-name"
  exit 1
fi

set -x

# Create a Docker image
if ! docker image ls mold-gentoo | grep -q mold-gentoo; then
  set -e
  cat <<EOF | docker build -t mold-gentoo -
FROM gentoo/stage3
RUN emerge-webrsync
RUN echo 'USE="elogind -systemd corefonts truetype jpeg jpeg2k tiff zstd static-libs"' >> /etc/portage/make.conf && \
    echo 'ACCEPT_LICENSE="* -@EULA"' >> /etc/portage/make.conf && \
    echo 'FEATURES="${FEATURE} noclean nostrip -ipc-sandbox -network-sandbox -pid-sandbox -sandbox"' >> /etc/portage/make.conf
EOF
  set +e
fi

# Build mold as a statically-linked executable
if ! [ -f mold ] || ! ldd mold 2>&1 | grep -q 'not a dynamic executable'; then
  make clean
  ./build-static.sh
fi

git_hash=$(./mold --version | perl -ne '/\((\w+)/; print $1;')

# Build a given package in Docker
build() {
  package="$1"
  cmd="MAKEOPTS=-j8 emerge --onlydeps $package && FEATURES=test MAKEOPTS=-j8 emerge $package"
  filename=`echo "$package" | sed 's!/!_!g'`
  link="ln -sf /mold/mold /usr/x86_64-pc-linux-gnu/bin/ld"
  docker="docker run --rm --cap-add=SYS_PTRACE -v `pwd`:/mold mold-gentoo timeout -v -k 10s 30m"
  dir=gentoo/$git_hash

  mkdir -p $dir/success $dir/failure

  $docker bash -c "$link; $cmd" >& $dir/$filename.mold
  if [ $? = 0 ]; then
    mv $dir/$filename.mold $dir/success
  else
    mv $dir/$filename.mold $dir/failure
  fi

  $docker bash -c "$cmd" >& $dir/$filename.ld
  if [ $? = 0 ]; then
    mv $dir/$filename.ld $dir/success
  else
    mv $dir/$filename.ld $dir/failure
  fi
}

# Build a package
build "$1"
