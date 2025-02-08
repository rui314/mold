#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -x assembler -
.globl _start
_start:
EOF

./mold -o $t/exe $t/a.o
readelf --sections $t/exe | not grep -Fq .interp

./mold -o $t/exe $t/a.o --dynamic-linker=/foo/bar
readelf --sections $t/exe | grep -Fq .interp
