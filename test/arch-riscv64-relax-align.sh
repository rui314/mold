#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.globl x1
.text
.p2align 5
x1:
  ret
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.globl x2
.text
.p2align 5
x2:
  ret
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>
#include <stdint.h>
extern char x1, x2;
int main() {
  printf("%lu %lu %lu\n",
         (uintptr_t)&x1 % 32,
         (uintptr_t)&x2 % 32,
         &x2 - &x1);
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o $t/c.o
$QEMU $t/exe | grep -q '0 0 32'
