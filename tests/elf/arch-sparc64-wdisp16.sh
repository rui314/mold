#!/usr/bin/env bash
. $(dirname $0)/common.inc

# R_SPARC_WDISP16 is used by branch-on-register instructions, whose 16-bit
# word displacement is split into two fields.

cat <<EOF | $CC -c -o $t/a.o -xassembler -
  .section .text.a, "ax", @progbits
  .globl is_zero
is_zero:
  brz %o0, ret1
  nop
  retl
  mov 0, %o0

  .section .text.b, "ax", @progbits
  .space 0x10000
ret1:
  retl
  mov 1, %o0

  .section .text.c, "ax", @progbits
  .globl is_nonzero
is_nonzero:
  brnz %o0, ret1
  nop
  retl
  mov 0, %o0
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
#include <stdio.h>
int is_zero(int);
int is_nonzero(int);
int main() {
  printf("%d %d %d %d\n", is_zero(0), is_zero(3), is_nonzero(0), is_nonzero(3));
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep '^1 0 0 1$'
