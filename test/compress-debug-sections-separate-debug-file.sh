#!/bin/bash
. $(dirname $0)/common.inc

nm mold | grep -q '__tsan_init' && skip
on_qemu && skip

cat <<EOF | $CC -c -g -o $t/a.o -xc -
#include <stdio.h>
int main() { printf("Hello world\n"); }
EOF

$CC -B. -o $t/exe $t/a.o -Wl,--compress-debug-sections=zlib -Wl,--separate-debug-file -Wl,--no-detach

readelf -WS $t/exe.dbg > $t/log
grep -q '\.debug_info .* [Cx] ' $t/log
grep -q '\.debug_str .* MS[Cx] ' $t/log
