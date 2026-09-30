#!/bin/bash
source "$(dirname "$0")"/common.inc

# -segment_order lays out the segments of an image no dyld loads (a
# -static one): the listed ones in their order after __TEXT, which
# holds the mach header and stays first (after __PAGEZERO), then the
# unlisted ones in the usual order, with a warning for each, and
# __LINKEDIT last (ld-prime).
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl __start
__start:
  ret
.data
.quad 1
.section __MYSEG,__mysect
.quad 2
.section __OTHER,__o
.quad 3
EOF

segs() { otool -l $1 | awk '$1 == "segname" && !seen[$2]++ { printf "%s ", $2 }'; }

$mold -arch $ARCH -static -e __start $t/a.o -segment_order __OTHER:__MYSEG -o $t/exe 2> $t/log
grep -q -- '-segment_order should list all segments, __DATA is missing' $t/log
[ "$(segs $t/exe)" = '__PAGEZERO __TEXT __OTHER __MYSEG __DATA __LINKEDIT ' ]

$mold -arch $ARCH -static -e __start $t/a.o -segment_order __OTHER:__MYSEG:__DATA:__TEXT \
  -o $t/exe2 2> $t/log2
grep -q -- '-segment_order of __TEXT is ignored, the segment must be ordered second' $t/log2
[ "$(segs $t/exe2)" = '__PAGEZERO __TEXT __OTHER __MYSEG __DATA __LINKEDIT ' ]

$mold -arch $ARCH -static -e __start -pagezero_size 0 $t/a.o \
  -segment_order __OTHER:__MYSEG:__DATA:__TEXT -o $t/exe3 2> $t/log3
grep -q -- '-segment_order of __TEXT is ignored, the segment must be ordered first' $t/log3
[ "$(segs $t/exe3)" = '__TEXT __OTHER __MYSEG __DATA __LINKEDIT ' ]

not $mold -arch $ARCH -static -e __start $t/a.o -segment_order __OTHER -o $t/exe4 2> $t/log4
grep -q -- '-segment_order should specifify at least two segments' $t/log4

echo 'int main() { return 0; }' | $CC -o $t/main.o -c -xc -
not $CC --ld-path=$mold -o $t/exe5 $t/main.o -Wl,-segment_order,__TEXT:__DATA 2> $t/log5
grep -q -- '-segment_order can only be used with -preload, -static' $t/log5
