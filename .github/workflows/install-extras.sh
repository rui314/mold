#!/bin/bash -x

apt-get update
apt-get install -y wget xz-utils

# Install a 32-bit RISC-V toolchain
mkdir /rv32
wget -O- --progress=dot:mega https://github.com/riscv-collab/riscv-gnu-toolchain/releases/download/2025.01.20/riscv32-glibc-ubuntu-22.04-gcc-nightly-2025.01.20-nightly.tar.xz | tar -C /rv32 --strip-components=1 --xz -xf -

ln -sf /rv32/sysroot /usr/riscv32-linux-gnu
echo '/rv32/bin/riscv32-unknown-linux-gnu-gcc -L/usr/riscv32-linux-gnu "$@"' > /usr/bin/riscv32-linux-gnu-gcc
echo '/rv32/bin/riscv32-unknown-linux-gnu-g++ -L/usr/riscv32-linux-gnu "$@"' > /usr/bin/riscv32-linux-gnu-g++
chmod 755 /usr/bin/riscv32-linux-gnu-{gcc,g++}

for i in objdump objcopy strip; do
  ln -sf /rv32/bin/riscv32-unknown-linux-gnu-$i /usr/bin/riscv32-linux-gnu-$i
done

# Install a LoongArch toolchain
mkdir /larch
wget -O- --progress=dot:mega https://github.com/loongson/build-tools/releases/download/2025.02.21/x86_64-cross-tools-loongarch64-binutils_2.44-gcc_14.2.0-glibc_2.41.tar.xz | tar -C /larch --strip-components=1 --xz -xf -

cp -r /larch/loongarch64-unknown-linux-gnu/lib/* /larch/target/lib64
ln -sf /larch/target /usr/loongarch64-linux-gnu

for i in gcc g++ objdump objcopy strip; do
  ln -sf /larch/bin/loongarch64-unknown-linux-gnu-$i /usr/bin/loongarch64-linux-gnu-$i
done

wget -O /usr/local/bin/qemu-loongarch64 --progress=dot:mega https://github.com/loongson/build-tools/releases/download/2024.11.01/qemu-loongarch64
chmod 755 /usr/local/bin/qemu-loongarch64

# Install ARM32 big-endian toolchain
mkdir /armeb
wget -O- --progress=dot:mega https://toolchains.bootlin.com/downloads/releases/toolchains/armebv7-eabihf/tarballs/armebv7-eabihf--glibc--stable-2024.05-1.tar.xz | tar -C /armeb --strip-components=1 --xz -xf -

ln -sf /armeb/armeb-buildroot-linux-gnueabihf/sysroot /usr/armeb-linux-gnueabihf
echo '/armeb/bin/armeb-linux-gcc -L/usr/armeb-linux-gnu "$@"' > /usr/bin/armeb-linux-gnueabihf-gcc
echo '/armeb/bin/armeb-linux-g++ -L/usr/armeb-linux-gnu "$@"' > /usr/bin/armeb-linux-gnueabihf-g++
chmod 755 /usr/bin/armeb-linux-gnueabihf-{gcc,g++}

for i in objdump objcopy strip; do
  ln -sf /armeb/bin/armeb-linux-$i /usr/bin/armeb-linux-gnueabihf-$i
done

# Install ARM64 big-endian toolchain
mkdir /aarch64be
wget -O- --progress=dot:mega https://toolchains.bootlin.com/downloads/releases/toolchains/aarch64be/tarballs/aarch64be--glibc--stable-2024.05-1.tar.xz | tar -C /aarch64be --strip-components=1 --xz -xf -

ln -sf /aarch64be/aarch64_be-buildroot-linux-gnu/sysroot /usr/aarch64_be-linux-gnu
echo '/aarch64be/bin/aarch64_be-linux-gcc -L/usr/aarch64_be-linux-gnu "$@"' > /usr/bin/aarch64_be-linux-gnu-gcc
echo '/aarch64be/bin/aarch64_be-linux-g++ -L/usr/aarch64_be-linux-gnu "$@"' > /usr/bin/aarch64_be-linux-gnu-g++
chmod 755 /usr/bin/aarch64_be-linux-gnu-{gcc,g++}

for i in objdump objcopy strip; do
  ln -sf /aarch64be/bin/aarch64_be-linux-$i /usr/bin/aarch64_be-linux-gnu-$i
done

# Install SH4 big-endian toolchain
mkdir /sh4aeb
wget -O- --progress=dot:mega https://toolchains.bootlin.com/downloads/releases/toolchains/sh-sh4aeb/tarballs/sh-sh4aeb--glibc--stable-2024.05-1.tar.xz | tar -C /sh4aeb --strip-components=1 --xz -xf -

ln -sf /sh4aeb/sh4aeb-buildroot-linux-gnu/sysroot /usr/sh4aeb-linux-gnu
echo '/sh4aeb/bin/sh4aeb-linux-gcc -L/usr/sh4aeb-linux-gnu "$@"' > /usr/bin/sh4aeb-linux-gnu-gcc
echo '/sh4aeb/bin/sh4aeb-linux-g++ -L/usr/sh4aeb-linux-gnu "$@"' > /usr/bin/sh4aeb-linux-gnu-g++
chmod 755 /usr/bin/sh4aeb-linux-gnu-{gcc,g++}

for i in objdump objcopy strip; do
  ln -sf /sh4aeb/bin/sh4aeb-linux-$i /usr/bin/sh4aeb-linux-gnu-$i
done

# Install Intel SDE CPU emulator for CET-related tests
mkdir /sde
wget -O- --progress=dot:mega https://downloadmirror.intel.com/843185/sde-external-9.48.0-2024-11-25-lin.tar.xz | tar -C /sde --strip-components=1 --xz -xf -
ln -s /sde/sde64 /usr/bin
