#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/elf/$MACHINE/$testname
mkdir -p $t

cat <<'EOF' | $CC -c -o $t/a.o -x assembler -
.globl val1, val2, val3, val4, val5

.section .rodata.str1.1,"aMS",@progbits,1
val1:
.ascii "Hello \0"

.section .rodata.str4.4,"aMS",@progbits,4
.align 4
val2:
.ascii "world   \0\0\0\0"

.section .rodata.foo,"aMS",@progbits,1
val3:
.ascii "foobar\0"

.section .rodata.cst8,"aM",@progbits,8
.align 8
val4:
.ascii "abcdefgh"

.section .rodatabar,"aMS",@progbits,1
val5:
.ascii "bar\0"
EOF

cat <<'EOF' | $CC -c -o $t/b.o -xc -
#include <stdio.h>

extern char val1, val2, val3, val4, val5;

int main() {
  printf("%p %p %p %p %p\n", &val1, &val2, &val3, &val4, &val5);
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o

readelf -p .rodata.str $t/exe | grep -q Hello
readelf -p .rodata.str $t/exe | grep -q world
readelf -p .rodata.str $t/exe | grep -q foobar
readelf -p .rodata.cst $t/exe | grep -q abcdefgh
readelf -p .rodatabar $t/exe | grep -q bar

echo OK
