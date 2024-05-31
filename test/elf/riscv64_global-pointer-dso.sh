#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.globl __global_pointer$
__global_pointer$ = 0x1000
EOF

$CC -B. -shared -o $t/b.so $t/a.o

cat <<EOF | $CC -o $t/c.o -c -xassembler - -O2
.globl get_global_pointer
get_global_pointer:
  la a0, __global_pointer$
  ret
EOF

cat <<EOF | $CC -o $t/d.o -c -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe $t/b.so $t/c.o $t/d.o
$QEMU $t/exe | grep -q 'Hello world'
