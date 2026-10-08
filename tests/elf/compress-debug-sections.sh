#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -g -o $t/a.o -xc -
#include <stdio.h>
int main() { printf("Hello world\n"); }
EOF

# mold compresses a section in 1 MiB shards and combines their Adler-32
# checksums, so give it a section that spans two shards.
seq 1 300000 > $t/data
cat <<EOF | $CC -c -o $t/b.o -xassembler -
.section .debug_foo,""
.incbin "$t/data"
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,--compress-debug-sections=zlib

readelf -WS $t/exe > $t/log
grep '\.debug_info .* [Cx] ' $t/log
grep '\.debug_str .* MS[Cx] ' $t/log
grep '\.debug_foo .* [Cx] ' $t/log

$OBJCOPY --decompress-debug-sections --dump-section .debug_foo=$t/dump $t/exe $t/exe2
cmp $t/data $t/dump
