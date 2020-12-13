#!/bin/bash
set -e
echo -n "Testing $(basename -s .sh $0) ..."
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

../mold -o $t/exe \
  /usr/lib/x86_64-linux-gnu/crt1.o \
  /usr/lib/x86_64-linux-gnu/crti.o \
  /usr/lib/gcc/x86_64-linux-gnu/9/crtbegin.o \
  -L/usr/lib/gcc/x86_64-linux-gnu/9 \
  -L/usr/lib/x86_64-linux-gnu \
  -L/usr/lib64 \
  -L/lib/x86_64-linux-gnu \
  -L/lib64 \
  -L/usr/lib/x86_64-linux-gnu \
  -L/usr/lib64 \
  -L/usr/lib64 \
  -L/usr/lib \
  -L/usr/lib/llvm-10/lib \
  -L/lib \
  -L/usr/lib \
   $t/a.o \
  -lgcc \
  --as-needed \
  -lgcc_s \
  --no-as-needed \
  -lc \
  -lgcc \
  --as-needed \
  -lgcc_s \
  --no-as-needed \
  /usr/lib/gcc/x86_64-linux-gnu/9/crtend.o \
  /usr/lib/x86_64-linux-gnu/crtn.o

$t/exe | grep -q 'Hello world'

echo ' OK'
