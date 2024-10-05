#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -g -o $t/a.o -xc -
#include <stdio.h>
int main() { printf("Hello world\n"); }
EOF

$CC -B. -o $t/exe $t/a.o -Wl,--compress-debug-sections=zlib

readelf -WS $t/exe > $t/log
grep -q '\.debug_info .* [Cx] ' $t/log
grep -q '\.debug_str .* MS[Cx] ' $t/log
