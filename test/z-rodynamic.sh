#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

$CC -B. -o $t/exe1 $t/a.o
readelf -WS $t/exe1 | grep '\.dynamic.* WA '

$CC -B. -o $t/exe2 $t/a.o -Wl,-z,rodynamic
readelf -WS $t/exe2 | grep '\.dynamic.* A '
