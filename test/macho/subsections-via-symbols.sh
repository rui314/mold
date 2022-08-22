#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xassembler -
.subsections_via_symbols
.globl _fn1, _fn2
.text
.align 4
_fn1:
  nop
_fn2:
  nop
EOF

cat <<EOF | cc -o $t/b.o -c -xassembler -
.globl _fn3, _fn4
.text
.align 16
_fn3:
  .byte 0xcc
_fn4:
  .byte 0xcc
EOF

cat <<EOF | cc -o $t/c.o -c -xc -
#include <stdio.h>

void fn1();
void fn2();
void fn3();
void fn4();

int main() {
  printf("%lu %lu\n", (char *)fn2 - (char *)fn1, (char *)fn4 - (char *)fn3);
}
EOF

cc --ld-path=./ld64 -o $t/exe $t/a.o $t/b.o $t/c.o
$t/exe | grep -q '^16 1$'

echo OK
