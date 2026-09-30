#!/bin/bash
source "$(dirname "$0")"/common.inc

# -image_base (or -seg1addr) puts the first segment after __PAGEZERO at
# the given address, for an image that stays where it was linked: a
# -static one, or one with classic dyld info (a non-PIE x86-64
# executable has that by default). dyld slides a PIE executable anyway,
# and ld-prime ignores the option with a warning for that and for any
# other image dyld loads with chained fixups. A base inside __PAGEZERO
# is an error.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -

text() { otool -l $1 | awk '$1 == "segname" && $2 == "__TEXT" { getline; print $2; exit }'; }

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-image_base,0x200000000 2> $t/log
grep -q 'Linking with PIE, -image_base will be ignored' $t/log
[ "$(text $t/exe)" = 0x0000000100000000 ]

$CC --ld-path=$mold -o $t/b.dylib -shared $t/a.o -Wl,-image_base,0x200000000 \
  -mmacosx-version-min=14.0 2> $t/log2
grep -q 'prefered load addresses (-seg1addr) are disabled with chained fixups' $t/log2
[ "$(text $t/b.dylib)" = 0x0000000000000000 ]

$CC --ld-path=$mold -o $t/c.dylib -shared $t/a.o -Wl,-seg1addr,0x200000000 \
  -mmacosx-version-min=14.0 -Wl,-no_fixup_chains
[ "$(text $t/c.dylib)" = 0x0000000200000000 ]

$mold -arch $ARCH -static -e _main $t/a.o -image_base 0x200000000 -o $t/exe2
[ "$(text $t/exe2)" = 0x0000000200000000 ]
nm -m $t/exe2 | grep -q '^0000000200000000 .*__mh_execute_header'

if [ $ARCH = x86_64 ]; then
  $CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-no_pie -Wl,-image_base,0x200000000 \
    -mmacosx-version-min=12.0
  [ "$(text $t/exe3)" = 0x0000000200000000 ]
  $t/exe3

  $CC --ld-path=$mold -o $t/exe8 $t/a.o -Wl,-no_pie -Wl,-image_base,0x200000000 \
    -mmacosx-version-min=14.0 2> /dev/null
  [ "$(text $t/exe8)" = 0x0000000200000000 ]
  $t/exe8

  $CC --ld-path=$mold -o $t/exe9 $t/a.o -Wl,-no_pie -Wl,-image_base,0x200000000 \
    -mmacosx-version-min=14.0 -Wl,-fixup_chains 2> $t/log9
  grep -q 'prefered load addresses (-seg1addr) are disabled with chained fixups' $t/log9
  [ "$(text $t/exe9)" = 0x0000000100000000 ]
  $t/exe9
fi

not $mold -arch $ARCH -static -e _main $t/a.o -image_base 0x100000 -o $t/exe4 2> $t/log4
grep -q 'custom segments overlap: __PAGEZERO(0x0-0x100000000) __TEXT(0x100000-' $t/log4

# A zero -image_base means none.
$mold -arch $ARCH -static -e _main $t/a.o -image_base 0x0 -o $t/exe5
[ "$(text $t/exe5)" = 0x0000000100000000 ]

# arm64 -static -pie counts its relocation addresses from -image_base
# plus __PAGEZERO's size in ld-prime, and a pointer that ends up more
# than 2 GiB away from that fails the link: any nonzero base under the
# default 4 GiB __PAGEZERO. (mold keeps counting from __TEXT.)
cat <<EOF2 | $CC -o $t/p.o -c -xassembler -
.text
.globl _main
_main: ret
.data
.p2align 3
.globl _arr
_arr: .quad 0
  .quad _main
EOF2
$mold -arch $ARCH -static -pie -e _main $t/p.o -pagezero_size 0x4000 -image_base 0x200000000 \
  -o $t/exe6
[ "$(text $t/exe6)" = 0x0000000200000000 ]
if [ $ARCH = arm64 ]; then
  not $mold -arch $ARCH -static -pie -e _main $t/p.o -image_base 0x200000000 -o $t/exe7 2> $t/log7
  grep -q "atom address cannot fit in a fixup at '_arr' (.*/p.o)+8" $t/log7
else
  $mold -arch $ARCH -static -pie -e _main $t/p.o -image_base 0x200000000 -o $t/exe7
fi

# An object file is loaded nowhere, and ld-prime takes the base of a -r
# link only to warn when it is not a multiple of 4 KiB.
$mold -r -arch $ARCH -o $t/r.o $t/a.o -image_base 0x200001000 2> $t/log8
[ ! -s $t/log8 ]
$mold -r -arch $ARCH -o $t/r2.o $t/a.o -image_base 0x200000800 2> $t/log9
grep -q 'base address 0x200000800 is not properly aligned. Changing it to 0x200001000' $t/log9
cmp $t/r.o $t/r2.o

# A final image's base is rounded up to a page first, with the warning,
# whatever happens to it next: the rounded base is what must match a
# -segaddr for __TEXT, and a PIE executable's is ignored after that.
$mold -arch $ARCH -static -e _main $t/a.o -image_base 0x200001001 -segaddr __TEXT 0x200004000 \
  -o $t/exe10 2> $t/log10
grep -q 'base address 0x200001001 is not properly aligned. Changing it to 0x20000[24]000' $t/log10
if [ $ARCH = arm64 ]; then
  not grep -q 'must match' $t/log10
  [ "$(text $t/exe10)" = 0x0000000200004000 ]
fi
$CC --ld-path=$mold -o $t/exe11 $t/a.o -Wl,-image_base,0x200001001 2> $t/log11
grep -q 'base address 0x200001001 is not properly aligned. Changing it to 0x20000[24]000' $t/log11
grep -q 'Linking with PIE, -image_base will be ignored' $t/log11
