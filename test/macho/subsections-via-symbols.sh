#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
t=out/test/macho/$testname
mkdir -p $t

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.subsections_via_symbols
.globl _fn1, _fn2
.text
.align 4
_fn1:
  nop
_fn2:
  nop
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.globl _fn3, _fn4
.text
.align 16
_fn3:
  .byte 0xcc
_fn4:
  .byte 0xcc
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>

void fn1();
void fn2();
void fn3();
void fn4();

int main() {
  printf("%lu %lu\n", (char *)fn2 - (char *)fn1, (char *)fn4 - (char *)fn3);
}
EOF

clang --ld-path=./ld64 -o $t/exe $t/a.o $t/b.o $t/c.o
$t/exe | grep -q '^16 1$'

echo OK
