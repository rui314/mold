#!/bin/bash
source "$(dirname "$0")"/common.inc

# -image_base (or -seg1addr) puts the first segment after __PAGEZERO at
# the given address, for an image that stays where it was linked: a
# -static one, a non-PIE x86-64 executable, a dylib or bundle with
# classic dyld info. dyld slides a PIE executable and a chained-fixups
# dylib anyway, and ld-prime ignores the option for those with a
# warning. A base inside __PAGEZERO is an error.
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
fi

not $mold -arch $ARCH -static -e _main $t/a.o -image_base 0x100000 -o $t/exe4 2> $t/log4
grep -q 'custom segments overlap: __PAGEZERO(0x0-0x100000000) __TEXT(0x100000-' $t/log4
