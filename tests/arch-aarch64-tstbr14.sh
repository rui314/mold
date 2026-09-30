#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xassembler -
.globl bit3
bit3:
  tbnz x0, #3, 1f
  mov w0, #0
  ret
.section .text.bit3, "ax"
1:
  mov w0, #1
  ret
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
#include <stdio.h>
int bit3(long);
int main() { printf("%d %d\n", bit3(8), bit3(7)); }
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep '^1 0$'

cat <<EOF | $CC -c -o $t/c.o -xassembler -
.globl bit3
bit3:
  tbnz x0, #3, 1f
  mov w0, #0
  ret
  .space 0x8000
.section .text.bit3, "ax"
1:
  mov w0, #1
  ret
EOF

not $CC -B. -o $t/exe2 $t/c.o $t/b.o |& grep 'R_AARCH64_TSTBR14.*out of range'
