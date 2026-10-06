#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
void foo() {}
EOF

$CC -B. -shared -o $t/b.so $t/a.o -Wl,-z,global
readelf --dynamic $t/b.so | grep 'Flags:.*GLOBAL'
