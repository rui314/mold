#!/bin/bash -x
set -e
apt-get install -y git bzip2 ninja-build pkg-config libglib2.0-dev python3-distutils-extra libpixman-1-dev

cd /
git clone --depth=1 https://github.com/foss-for-synopsys-dwc-arc-processors/qemu.git
mkdir /qemu/build
cd /qemu/build
../configure --static --extra-cflags=-Wno-error
ninja
ln -s /qemu/build/qemu-arc /qemu/build/qemu-arc64 /usr/local/bin


apt-get install -y libgmp-dev libmpfr-dev texinfo bison yacc flex
cd /
git clone --depth=1 https://github.com/foss-for-synopsys-dwc-arc-processors/binutils-gdb.git
cd /binutils-gdb
mkdir build
cd build
../configure --target=arc-linux
make -j$(nproc)


apt-get install -y mold
mkdir /build
cd /build
cmake -GNinja -DCMAKE_BUILD_TYPE=Debug -DMOLD_USE_MOLD=1 /mold
ninja
TRIPLE=arc-linux-gnu bash -x /mold/test/hello-dynamic.sh
