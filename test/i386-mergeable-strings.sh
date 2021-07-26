#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<'EOF' | cc -o $t/a.o -c -x assembler -m32 -
  .text
  .globl main
main:
  push $.L.str+3
  call printf
  add $4, %esp
  push $.rodata.str1.1+16
  call printf
  add $4, %esp
  xor %eax, %eax
  ret

  .section .rodata.str1.1, "aMS", @progbits, 1
  .string "bar"
.L.str:
  .string "xyzHello"
  .string "foo world\n"
EOF

clang -m32 -fuse-ld=$mold -static -o $t/exe $t/a.o
$t/exe | grep -q 'Hello world'

echo OK
