#!/bin/bash
set -e
echo -n "Testing $(basename -s .sh $0) ..."
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -x assembler -
  .text
  .globl  real_foobar
real_foobar:
  lea     msg(%rip), %rdi
  xor     %rax, %rax
  call    printf
  xor     %rax, %rax
  ret

  .globl  resolve_foobar
resolve_foobar:
  pushq   %rbp
  movq    %rsp, %rbp
  leaq    real_foobar(%rip), %rax
  popq    %rbp
  ret

  .globl  foobar
  .type   foobar, @gnu_indirect_function
  .set    foobar, resolve_foobar

  .globl  main
main:
  pushq   %rbp
  movq    %rsp, %rbp
  call    foobar@PLT
	xor     %rax, %rax
  popq    %rbp
  ret

  .data
msg:
  .string "Hello world\n"
EOF


../mold -static -o $t/exe /usr/lib/x86_64-linux-gnu/crt1.o \
  /usr/lib/x86_64-linux-gnu/crti.o \
  /usr/lib/gcc/x86_64-linux-gnu/9/crtbeginT.o \
  $t/a.o \
  /usr/lib/gcc/x86_64-linux-gnu/9/libgcc.a \
  /usr/lib/gcc/x86_64-linux-gnu/9/libgcc_eh.a \
  /usr/lib/x86_64-linux-gnu/libc.a \
  /usr/lib/gcc/x86_64-linux-gnu/9/crtend.o \
  /usr/lib/x86_64-linux-gnu/crtn.o > /dev/null

$t/exe | grep -q 'Hello world'

echo ' OK'
