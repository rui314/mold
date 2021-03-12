#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -x assembler -
  .text
  .globl main
main:
  lea msg(%rip), %rdi
  xor %rax, %rax
  call printf@PLT
  xor %rax, %rax
  ret

  .data
msg:
  .string "Hello world\n"
EOF

rm -f $t/exe

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o -Wl,-preload
! test -e $t/exe
clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o
$t/exe | grep -q 'Hello world'

echo OK
