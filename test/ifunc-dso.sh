#!/bin/bash
set -e
mold=$1
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

# Skip if libc is musl because musl does not support GNU FUNC
echo 'int main() {}' | cc -o $t/exe -xc -
ldd $t/exe | grep -q ld-musl && { echo OK; exit; }

cat <<EOF | cc -o $t/a.o -c -x assembler -
  .text
  .globl  main
main:
  pushq   %rbp
  movq    %rsp, %rbp
  call    foobar@PLT
  xor     %rax, %rax
  popq    %rbp
  ret
EOF

cat <<EOF | cc -shared -fPIC -o $t/b.so -x assembler -
  .text
real_foobar:
  lea     .Lmsg(%rip), %rdi
  xor     %rax, %rax
  call    printf
  xor     %rax, %rax
  ret

  .globl  resolve_foobar
resolve_foobar:
  pushq   %rbp
  movq    %rsp, %rbp
  movq    real_foobar@GOTPCREL(%rip), %rax
  popq    %rbp
  ret

  .globl  foobar
  .type   foobar, @gnu_indirect_function
  .set    foobar, resolve_foobar

  .data
.Lmsg:
  .string "Hello world\n"
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o $t/b.so
$t/exe | grep -q 'Hello world'

echo OK
