#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
void foo() {}
EOF

$CC -o $t/b.so -shared $t/a.o
readelf --dynamic $t/b.so | not grep -Fq 'Library soname'

$CC -B. -o $t/b.so -shared $t/a.o -Wl,-soname,foo
readelf --dynamic $t/b.so | grep -Fq 'Library soname: [foo]'
