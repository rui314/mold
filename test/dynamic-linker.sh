#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -x assembler -
.globl _start
_start:
EOF

./mold -o $t/exe1 $t/a.o
readelf -WS $t/exe1 | not grep -F .interp

./mold -o $t/exe2 $t/a.o --dynamic-linker=/foo/bar
readelf -WS $t/exe2 | grep -F .interp

./mold -o $t/exe3 $t/a.o --dynamic-linker=/foo/bar -static
readelf -WS $t/exe3 | not grep -F .interp
