#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void _start() {}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -march=rv64gc_ztso || skip
void foo() {}
EOF

$CC -B. -nostdlib -o $t/exe $t/a.o $t/b.o
readelf -h $t/exe | grep -Fw TSO
