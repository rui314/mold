#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC
void foo() {}
EOF

$CC -B. -shared -o $t/b.so $t/a.o
./mold -shared -o $t/c.so $t/b.so
readelf -W --dynamic $t/c.so | grep -F /b.so
