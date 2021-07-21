#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -x assembler -
  .text
  .globl main
main:
  lea msg(%rip), %rdi
  xor %rax, %rax
  call printf
  xor %rax, %rax
  ret

  .data
msg:
  .string "Hello world\n"
EOF

clang -fuse-ld=$mold -static -Wl,--filler,0xfe -o $t/exe1 $t/a.o
sed -i -e 's/--filler 0xfe/--filler 0x00/' $t/exe1
hexdump -C $t/exe1 > $t/txt1

clang -fuse-ld=$mold -static -Wl,--filler,0x00 -o $t/exe2 $t/a.o
hexdump -C $t/exe2 > $t/txt2

diff -q $t/txt1 $t/txt2

echo OK
