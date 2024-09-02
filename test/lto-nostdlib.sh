#!/bin/bash
. $(dirname $0)/common.inc

test_cflags -flto || skip

cat <<EOF | $CC -flto -c -o $t/a.o -xc -
void _start() {}
EOF

$CC -B. -o $t/exe -flto $t/a.o -nostdlib
readelf -Ws $t/exe | grep -Eq ' _start'
