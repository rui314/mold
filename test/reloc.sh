#!/bin/bash
set -e
echo -n "Testing $(basename -s .sh $0) ..."
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -x assembler -
  .set abs, 0xcafe

  .globl _start
_start:
  call _start
  call extfn@PLT
  call abs
  mov data(%rip), %rax
  mov data@GOT, %rax
  mov data@GOTPCREL(%rip), %rax

  .data
data:
  .string "foo"
  .quad extdata

  .section .rodata
rodata:
  .string "foo"
  .zero 4
  .quad extdata
EOF

cat <<EOF | cc -shared -o $t/b.so -x assembler -
  .type extfn, @function
  .globl extfn
extfn:
  nop

  .data
  .type extdata, @object
  .size extdata, 4
  .globl extdata
extdata:
  .long 3
EOF

clang -o $t/exe $t/a.o $t/b.so -nostdlib -pie

echo ' OK'
