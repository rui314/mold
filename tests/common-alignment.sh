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

# -max_default_common_align lowers that cap: a hexadecimal power of two
# up to 0x8000, any other number rounded down with a warning, 0 taken
# for 1. The default is 0x100 in a -preload image.
cat <<EOF2 | $CC -o $t/e.o -c -xassembler -
.globl _main
_main: ret
.comm _a, 1
.comm _b, 4096
EOF2
gap() {
  nm $1 > $1.nm
  echo $(( 0x$(awk '/ _b$/ { print $1 }' $1.nm) - 0x$(awk '/ _a$/ { print $1 }' $1.nm) ))
}

$CC --ld-path=$mold -o $t/exe2 $t/e.o -Wl,-max_default_common_align,0x10
[ $(gap $t/exe2) = 16 ]
$CC --ld-path=$mold -o $t/exe3 $t/e.o -Wl,-max_default_common_align,0x30 2> $t/log3
grep -q 'alignment for -max_default_common_align is not a power of two, using 0x20' $t/log3
[ $(gap $t/exe3) = 32 ]
$CC --ld-path=$mold -o $t/exe4 $t/e.o -Wl,-max_default_common_align,0 2> $t/log4
grep -q 'zero is not a valid -max_default_common_align' $t/log4
[ $(gap $t/exe4) = 1 ]

not $mold -o $t/exe5 $t/e.o -max_default_common_align 0x8001 2> $t/log5
grep -q 'argument for -max_default_common_align (0x8001) must be less than or equal to 0x8000' $t/log5
not $mold -o $t/exe5 $t/e.o -max_default_common_align 1x 2> $t/log5
grep -q -- '-max_default_common_align must specify an integer size' $t/log5
not $mold -o $t/exe5 $t/e.o -max_default_common_align 2> $t/log5
grep -q -- '-max_default_common_align.*missing' $t/log5

$mold -preload -arch $ARCH -e _main -o $t/exe6 $t/e.o
[ $(gap $t/exe6) = 256 ]
