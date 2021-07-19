#!/bin/bash
set -e
mold=$1
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

# Skip if libc is musl
echo 'int main() {}' | cc -o $t/exe -xc -
ldd $t/exe | grep -q ld-musl && { echo OK; exit; }

cat <<'EOF' | clang -c -o $t/a.o -x assembler -
.L0:
  mov $0, %edi
  call print
  ret
.L1:
  mov $1, %edi
  call print
  ret
.L2:
  mov $2, %edi
  call print
  ret
.L3:
  mov $3, %edi
  call print
  ret
.L4:
  mov $4, %edi
  call print
  ret
.L5:
  mov $5, %edi
  call print
  ret
.L6:
  mov $6, %edi
  call print
  ret
.L7:
  mov $7, %edi
  call print
  ret

.section .init_array,"aw",@init_array
.quad .L0
.quad .L1

.section .ctors,"aw",@progbits
.quad .L2
.quad .L3

.section .fini_array,"aw",@fini_array
.quad .L4
.quad .L5

.section .dtors,"aw",@progbits
.quad .L6
.quad .L7
EOF

cat <<'EOF' | clang -c -o $t/b.o -x assembler -
.L8:
  mov $8, %edi
  call print
  ret
.L9:
  mov $9, %edi
  call print
  ret
.La:
  mov $10, %edi
  call print
  ret
.Lb:
  mov $11, %edi
  call print
  ret
.Lc:
  mov $12, %edi
  call print
  ret
.Ld:
  mov $13, %edi
  call print
  ret
.Le:
  mov $14, %edi
  call print
  ret
.Lf:
  mov $15, %edi
  call print
  ret

.section .init_array,"aw",@init_array
.quad .L8
.quad .L9

.section .ctors,"aw",@progbits
.quad .La
.quad .Lb

.section .fini_array,"aw",@fini_array
.quad .Lc
.quad .Ld

.section .dtors,"aw",@progbits
.quad .Le
.quad .Lf
EOF

cat <<EOF | clang -c -o $t/c.o -xc -
#include <stdio.h>

void print(int n) {
  printf("%x", n);
}

int main() {}
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o $t/b.o $t/c.o
$t/exe | grep -q '^013289baefdc6754$'

echo OK
