#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime knows the Objective-C image info in __DATA alone: an
# __objc_imageinfo of another segment is any other section to it, kept
# as it is (no check of its size), stripped under -dead_strip when
# nothing refers to it, and in a final image the record it makes of the
# __DATA ones joins it when both are __DATA_CONST,__objc_imageinfo.
mk() {
  {
    echo ".section $2,__objc_imageinfo,regular"
    echo '.long 0'
    echo ".long $3"
    [ -z "$4" ] || echo "$4"
    printf '.globl _%s\n.text\n_%s: ret\n.subsections_via_symbols\n' $1 $1
  } | $CC -o $t/$1.o -c -xassembler -
}
mk main __DATA_CONST 0x40 '.long 7'
mk data __DATA 0
mk text __TEXT 0x40

$CC --ld-path=$mold -o $t/exe $t/main.o $t/data.o 2> $t/log
not grep -F 'unexpectedly large' $t/log
otool -l $t/exe > $t/load
[ "$(grep -c 'sectname __objc_imageinfo' $t/load)" = 1 ]
otool -s __DATA_CONST __objc_imageinfo $t/exe > $t/info
grep -Eq '00000000 00000040 00000007 00000000|00 00 00 00 40 00 00 00 07 00 00 00 00 00 00 00' $t/info
grep -Eq '10\s+(00000000|00 00 00 00) *$' $t/info

$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/text.o
otool -l $t/exe2 | grep -A1 'sectname __objc_imageinfo' > $t/load2
grep -q __TEXT $t/load2
grep -q __DATA_CONST $t/load2

$CC --ld-path=$mold -o $t/exe3 $t/main.o $t/text.o -Wl,-dead_strip
otool -l $t/exe3 > $t/load3
not grep -F __objc_imageinfo $t/load3

# A -r output keeps the section where it was, and makes __DATA's.
$mold -r -arch $ARCH -o $t/r.o $t/main.o $t/data.o
otool -l $t/r.o | grep -A1 'sectname __objc_imageinfo' > $t/load4
grep -q __DATA_CONST $t/load4
grep -q '__DATA$' $t/load4
