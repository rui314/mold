#!/usr/bin/env bash
. $(dirname $0)/common.inc

# R_SPARC_OLO10 has a second addend in the upper bits of its type field.

cat <<EOF | $CC -fno-PIC -c -o $t/a.o -xassembler -
  .globl get
get:
  sethi %hi(var), %g1
  retl
  ld [%g1 + %lo(var) + 8], %o0

  .data
  .globl var
var:
  .word 1, 2, 3, 4
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
#include <stdio.h>
int get();
int main() { printf("%d\n", get()); }
EOF

./mold -r -o $t/c.o $t/a.o
$CC -B. -no-pie -o $t/exe1 $t/c.o $t/b.o
$QEMU $t/exe1 | grep '^3$'

$CC -B. -no-pie -o $t/exe2 $t/a.o $t/b.o -Wl,--emit-relocs
readelf -r $t/exe2 | grep -E 'R_SPARC_OLO10 +[0-9a-f]+ var \+ 0 \+ 8$'
