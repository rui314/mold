#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

# Skip if libc is musl because musl does not support GNU FUNC
echo 'int main() {}' | cc -o $t/exe -xc -
ldd $t/exe | grep -q ld-musl && { echo OK; exit; }

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

clang -fuse-ld=$mold -shared -o $t/b.so $t/a.o
readelf --dyn-syms $t/b.so | grep -Pq '0 (IFUNC|<OS specific>: 10)\s+GLOBAL DEFAULT   \d+ foobar'

echo OK
