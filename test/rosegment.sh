#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() { printf("Hello world\n"); }
EOF

$CC -B. -o $t/exe1 $t/a.o
readelf -W --segments $t/exe1 | not grep '\.interp .* \.text'

$CC -B. -o $t/exe2 $t/a.o -Wl,--rosegment
readelf -W --segments $t/exe2 | not grep '\.interp .* \.text'

$CC -B. -o $t/exe3 $t/a.o -Wl,--no-rosegment
readelf -W --segments $t/exe3 | grep '\.interp .* \.text'
