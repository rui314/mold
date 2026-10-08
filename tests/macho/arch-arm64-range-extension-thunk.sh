#!/usr/bin/env bash
. $(dirname $0)/common.inc

[ "$ARCH" = arm64 ] || skip

# main and far_func are separated by 136 MiB of code, more than the
# +-128 MiB reach of a b/bl instruction: main's call forward to
# far_func, far_func's call back to near_func and main's call to
# printf's stub after them all each go through a thunk.
cat <<EOF2 | $CC -o $t/a.o -c -xassembler -
.subsections_via_symbols
.macro pad
_pad\@:
  .long \@
  .space 0x100000 - 4
.endm
.rept 136
pad
.endr
EOF2

cat <<EOF2 | $CC -o $t/b.o -c -xassembler -
.subsections_via_symbols
.globl _far_func
.p2align 2
_far_func:
  stp x29, x30, [sp, #-16]!
  bl _near_func
  add w0, w0, #1
  ldp x29, x30, [sp], #16
  ret
EOF2

cat <<EOF2 | $CC -o $t/c.o -c -xc -
#include <stdio.h>
int far_func();
int near_func() { return 41; }
int main() {
  printf("%d\n", far_func());
}
EOF2

$CC --ld-path=$mold -o $t/exe $t/c.o $t/a.o $t/b.o
$RUN $t/exe | grep '^42$'
