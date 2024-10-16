#!/bin/bash -x

apt-get update
apt-get install -y wget xz-utils

# Install a RV32 toolchain from third party since it's not available
# as an Ubuntu package.
mkdir /rv32
wget -O- -q https://github.com/riscv-collab/riscv-gnu-toolchain/releases/download/2023.07.07/riscv32-glibc-ubuntu-20.04-gcc-nightly-2023.07.07-nightly.tar.gz | tar -C /rv32 --strip-components=1 -xzf -

ln -sf /rv32/sysroot /usr/riscv32-linux-gnu
echo '/rv32/bin/riscv32-unknown-linux-gnu-gcc -L/usr/riscv32-linux-gnu "$@"' > /usr/bin/riscv32-linux-gnu-gcc
echo '/rv32/bin/riscv32-unknown-linux-gnu-g++ -L/usr/riscv32-linux-gnu "$@"' > /usr/bin/riscv32-linux-gnu-g++
chmod 755 /usr/bin/riscv32-linux-gnu-{gcc,g++}

for i in objdump objcopy strip; do
  ln -sf /rv32/bin/riscv32-unknown-linux-gnu-$i /usr/bin/riscv32-linux-gnu-$i
done

# Install a LoongArch toolchain
mkdir /larch
wget -O- -q https://github.com/loongson/build-tools/releases/download/2024.08.08/x86_64-cross-tools-loongarch64-binutils_2.43-gcc_14.2.0-glibc_2.40.tar.xz | tar -C /larch --strip-components=1 --xz -xf -

cp -r /larch/loongarch64-unknown-linux-gnu/lib/* /larch/target/lib64
ln -sf /larch/target /usr/loongarch64-linux-gnu

for i in gcc g++ objdump objcopy strip; do
  ln -sf /larch/bin/loongarch64-unknown-linux-gnu-$i /usr/bin/loongarch64-linux-gnu-$i
done

wget -O /usr/local/bin/qemu-loongarch64 -q https://github.com/loongson/build-tools/releases/download/2023.08.08/qemu-loongarch64
chmod 755 /usr/local/bin/qemu-loongarch64

# Install SH4 big-endian toolchain
mkdir /sh4aeb
wget -O- -q https://toolchains.bootlin.com/downloads/releases/toolchains/sh-sh4aeb/tarballs/sh-sh4aeb--glibc--stable-2024.05-1.tar.xz | tar -C /sh4aeb --strip-components=1 --xz -xf -
ln -sf /sh4aeb/sh4aeb-buildroot-linux-gnu/sysroot /usr/sh4aeb-linux-gnu
echo '/sh4aeb/bin/sh4aeb-linux-gcc -L/usr/sh4aeb-linux-gnu "$@"' > /usr/bin/sh4aeb-linux-gnu-gcc
echo '/sh4aeb/bin/sh4aeb-linux-g++ -L/usr/sh4aeb-linux-gnu "$@"' > /usr/bin/sh4aeb-linux-gnu-g++
chmod 755 /usr/bin/sh4aeb-linux-gnu-{gcc,g++}

for i in objdump objcopy strip; do
  ln -sf /sh4aeb/bin/sh4aeb-linux-$i /usr/bin/sh4aeb-linux-gnu-$i
done

# Install Intel SDE CPU emulator for CET-related tests
mkdir /sde
wget -O- -q https://downloadmirror.intel.com/831748/sde-external-9.44.0-2024-08-22-lin.tar.xz | tar -C /sde --strip-components=1 --xz -xf -
ln -s /sde/sde64 /usr/bin
