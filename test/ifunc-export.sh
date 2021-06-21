#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -c -fPIC -o $t/a.o -x assembler -
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

clang -fuse-ld=`pwd`/../mold -shared -o $t/b.so $t/a.o
readelf --dyn-syms $t/b.so | grep -Pq '0 FUNC    GLOBAL DEFAULT   \d+ foobar'

echo OK
