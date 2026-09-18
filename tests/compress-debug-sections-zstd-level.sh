#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -g -o $t/a.o -xc -
#include <stdio.h>
int main() { printf("Hello world\n"); }
EOF

# Test zstd:1 (lowest level)
$CC -B. -o $t/exe1 $t/a.o -Wl,--compress-debug-sections=zstd:1
readelf -WS $t/exe1 | grep '\.debug_info .* [Cx] '

# Test zstd:22 (highest level)
$CC -B. -o $t/exe22 $t/a.o -Wl,--compress-debug-sections=zstd:22
readelf -WS $t/exe22 | grep '\.debug_info .* [Cx] '

# Out-of-range level should fail with a descriptive error
not $CC -B. -o $t/exe0 $t/a.o -Wl,--compress-debug-sections=zstd:0 |&
  grep 'zstd level must be between 1 and 22'
not $CC -B. -o $t/exe23 $t/a.o -Wl,--compress-debug-sections=zstd:23 |&
  grep 'zstd level must be between 1 and 22'
