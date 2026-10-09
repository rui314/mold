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

# An image dyld loads may not order its segments; ld-prime still warns
# first that the list leaves out __DATA_CONST, which such an image has.
echo 'int main() { return 0; }' | $CC -o $t/main.o -c -xc -
not $CC --ld-path=$mold -o $t/exe5 $t/main.o -Wl,-segment_order,__TEXT:__DATA 2> $t/log5
grep -A1 -- '-segment_order lists __DATA, but not __DATA_CONST, assuming standard order' $t/log5 |
  grep -q -- '-segment_order can only be used with -preload, -static'
not $CC --ld-path=$mold -o $t/exe6 $t/main.o -Wl,-segment_order,__TEXT:__DATA_CONST:__DATA \
  2> $t/log6
grep -q -- '-segment_order can only be used with -preload, -static' $t/log6
not grep -q -- 'not __DATA_CONST' $t/log6

# Without the option, an image no dyld loads has __TEXT and __DATA
# first, then its other segments in the order they first appear - its
# __DATA_CONST too (which -data_const fills with __DATA's read-only
# sections), unlike a kext's, which follows __DATA.
cat <<EOF2 | $CC -o $t/b.o -c -xassembler -
.section __DATA,__yy
.quad 1
.section __DATA,__const
.quad 1
.section __FOO,__a
.quad 1
.section __DATA_CONST,__b
.quad 1
.section __BAR,__c
.quad 1
.text
.globl __start
__start: ret
EOF2
cat <<EOF2 | $CC -o $t/c.o -c -xassembler -
.section __BAZ,__d
.quad 1
.data
.quad 1
EOF2
$mold -arch $ARCH -static -e __start $t/b.o $t/c.o -o $t/exe7
[ "$(segs $t/exe7)" = '__PAGEZERO __TEXT __DATA __FOO __DATA_CONST __BAR __BAZ __LINKEDIT ' ]
$mold -arch $ARCH -static -e __start $t/c.o $t/b.o -o $t/exe8
[ "$(segs $t/exe8)" = '__PAGEZERO __TEXT __DATA __BAZ __FOO __DATA_CONST __BAR __LINKEDIT ' ]
$mold -arch $ARCH -static -e __start $t/b.o $t/c.o -data_const -o $t/exe9
[ "$(segs $t/exe9)" = '__PAGEZERO __TEXT __DATA __DATA_CONST __FOO __BAR __BAZ __LINKEDIT ' ]
$mold -arch $ARCH -preload -e __start $t/c.o $t/b.o -o $t/exe10
[ "$(segs $t/exe10)" = '__TEXT __DATA __BAZ __FOO __DATA_CONST __BAR ' ]
$mold -arch $ARCH -kext $t/b.o $t/c.o -o $t/kext
segs $t/kext > $t/segs_kext
grep -q "__DATA __DATA_CONST __FOO __BAR __BAZ __LINKEDIT " $t/segs_kext
