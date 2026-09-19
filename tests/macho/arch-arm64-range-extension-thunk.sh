#!/usr/bin/env bash
. $(dirname $0)/common.inc

[ "$ARCH" = arm64 ] || skip

# Two functions separated by more than the +-128 MiB reach of a b/bl
# instruction.
cat <<EOF2 | $CC -o $t/a.o -c -xassembler -
.globl _far_func
.p2align 2
_far_func:
  mov w0, #42
  ret
.space 0x8800000
EOF2

cat <<EOF2 | $CC -o $t/b.o -c -xassembler -
.space 0x8800000
EOF2

cat <<EOF2 | $CC -o $t/c.o -c -xc -
#include <stdio.h>
int far_func();
int main() {
  printf("%d\n", far_func());
}
EOF2

$CC --ld-path=$mold -o $t/exe $t/c.o $t/a.o $t/b.o
$t/exe | grep '^42$'
