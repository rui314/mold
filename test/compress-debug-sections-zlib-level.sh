#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -g -o $t/a.o -xc -
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
int main() { printf("Hello world\n"); }
EOF

# Level 0 stores data uncompressed; level 9 compresses hardest.
# The level-0 executable must be larger than the level-9 one.
$CC -B. -o $t/exe0 $t/a.o -Wl,--compress-debug-sections=zlib:0
$CC -B. -o $t/exe9 $t/a.o -Wl,--compress-debug-sections=zlib:9
readelf -WS $t/exe0 | grep '\.debug_info .* [Cx] '
readelf -WS $t/exe9 | grep '\.debug_info .* [Cx] '
[ $(wc -c < $t/exe0) -gt $(wc -c < $t/exe9) ]

# Out-of-range level should fail with a descriptive error
not $CC -B. -o $t/exe-1 $t/a.o -Wl,--compress-debug-sections=zlib:-1 |&
  grep 'zlib level must be between 0 and 9'
not $CC -B. -o $t/exe10 $t/a.o -Wl,--compress-debug-sections=zlib:10 |&
  grep 'zlib level must be between 0 and 9'
