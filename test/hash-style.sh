#!/bin/bash
. $(dirname $0)/common.inc

# MIPS ABIs are not compatible with .gnu.hash

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF

./mold -shared -o $t/b.so $t/a.o

readelf -WS $t/b.so | grep -F ' .hash'
readelf -WS $t/b.so | grep -F ' .gnu.hash'

./mold -shared -o $t/c.so $t/a.o --hash-style=both --hash-style=none

readelf -WS $t/c.so > $t/log
not grep -F ' .hash' $t/log
not grep -F ' .gnu.hash' $t/log
