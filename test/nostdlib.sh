#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fno-PIE
void _start() {}
EOF

./mold -o $t/exe $t/a.o

readelf -W --sections $t/exe > $t/log
not grep -Fq ' .dynsym ' $t/log
not grep -Fq ' .dynstr ' $t/log
