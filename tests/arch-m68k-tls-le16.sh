#!/usr/bin/env bash
. $(dirname $0)/common.inc

# The thread pointer points 0x7000 bytes past the start of the TLS block,
# so a 16-bit local-exec offset is usually negative.

cat <<EOF | $CC -c -o $t/a.o -xassembler -
  .globl get_x
get_x:
  move.l 4(%sp), %a0
  move.l (x@TLSLE:w, %a0), %d0
  rts

  .section .tdata, "awT", @progbits
  .globl x
x:
  .long 42
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
#include <stdio.h>
void *__m68k_read_tp(void);
int get_x(void *tp);
int main() { printf("%d\n", get_x(__m68k_read_tp())); }
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep '^42$'
