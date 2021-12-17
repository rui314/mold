#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<'EOF' | cc -c -o $t/a.o -x assembler -
.globl val1, val2, val3

.section .rodata.str1.1,"aMS",@progbits,1
val1:
.ascii "Hello \0"

.section .rodata.str4.4,"aMS",@progbits,4
.align 4
val2:
.ascii "world  \0\0\0\0"

.section .rodata.cst8,"aM",@progbits,8
.align 8
val3:
.ascii "abcdefgh"
EOF

cat <<'EOF' | cc -c -o $t/b.o -xc -
#include <stdio.h>

extern char val1, val2, val3;

int main() {
  printf("%p %p %p\n", &val1, &val2, &val3);
}
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o $t/b.o

readelf -p .rodata.str $t/exe | grep -q Hello
readelf -p .rodata.str $t/exe | grep -q world
readelf -p .rodata.cst $t/exe | grep -q abcdefgh

echo OK
