#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -fcommon -c -xc -
int foo;
__attribute__((aligned(4096))) int bar;
EOF2

cat <<EOF2 | $CC -o $t/b.o -c -xc -
#include <stdio.h>
#include <stdint.h>
extern int foo;
extern int bar;
int main() {
  printf("%lu %lu\n", (uintptr_t)&foo % 4, (uintptr_t)&bar % 4096);
}
EOF2

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o
$t/exe | grep '^0 0$'

# Without a stated alignment, a common symbol is aligned to its size
# rounded up to a power of two, at most 2^15, which the segment's page
# alignment then caps with a warning.
echo '.comm _c240, 240' | $CC -o $t/c.o -c -xassembler -
$CC --ld-path=$mold -shared -o $t/c.dylib $t/c.o
otool -l $t/c.dylib | grep -A5 'sectname __common' | grep -q 'align 2^8'

echo '.comm _c40000, 40000' | $CC -o $t/d.o -c -xassembler -
$CC --ld-path=$mold -shared -o $t/d.dylib $t/d.o 2> $t/log
if [ $ARCH = arm64 ]; then page=0x4000; else page=0x1000; fi
grep -q "reducing alignment of section __DATA,__common from 0x8000 to $page" $t/log
