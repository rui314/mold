#!/bin/bash
source "$(dirname "$0")"/common.inc

# -section_order lays out the sections of a segment of an image no dyld
# loads (-static or -preload): the listed ones lead the segment in the
# list's order, after __text unless the list places it too, and the
# rest follow in their usual order. A name nothing defines is skipped.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __TEXT,__foo
.quad 1
.section __TEXT,__code2,regular,pure_instructions
ret
.globl _start
.text
_start: ret
.cstring
.asciz "hi"
.section __TEXT,__const
.quad 2
.section __DATA,__zz
.quad 3
.data
.quad 4
.section __DATA,__const
.quad 5
.zerofill __DATA,__bss,_b,64,3
.zerofill __DATA,__zf,_c,64,3
EOF

sects() {
  otool -l $1 | awk -v seg=$2 '$1 == "sectname" { s = $2 } $1 == "segname" && s != "" { if ($2 == seg) printf "%s ", s; s = "" }'
}

$mold -arch $ARCH -preload -e _start $t/a.o -o $t/exe1
[ "$(sects $t/exe1 __TEXT)" = '__text __code2 __foo __cstring __const ' ]
[ "$(sects $t/exe1 __DATA)" = '__zz __data __const __bss __zf ' ]

$mold -arch $ARCH -preload -e _start $t/a.o -o $t/exe2 \
  -section_order __TEXT __const:__nonexistent:__cstring -section_order __DATA __const:__data
[ "$(sects $t/exe2 __TEXT)" = '__text __const __cstring __code2 __foo ' ]
[ "$(sects $t/exe2 __DATA)" = '__const __data __zz __bss __zf ' ]

$mold -arch $ARCH -static -e _start $t/a.o -o $t/exe3 -section_order __TEXT __foo:__text \
  -section_order __DATA __zz:__data:__const:__zf
[ "$(sects $t/exe3 __TEXT)" = '__foo __text __code2 __cstring __const ' ]
[ "$(sects $t/exe3 __DATA)" = '__zz __data __const __zf __bss ' ]

# A zero-fill section has no file bytes, so it may come only after
# every section with contents.
not $mold -arch $ARCH -preload -e _start $t/a.o -o $t/exe4 \
  -section_order __DATA __data:__zf 2> $t/log4
grep -q '__zf is zero-fill, it should be ordered at the end of the segment __DATA, or alongside other zero-fill sections' $t/log4

not $mold -arch $ARCH -preload -e _start $t/a.o -o $t/exe5 \
  -section_order __DATA __data -section_order __DATA __const 2> $t/log5
grep -q -- '-section_order __DATA used more than once' $t/log5

not $mold -arch $ARCH -preload -e _start $t/a.o -o $t/exe6 -section_order __DATA 2> $t/log6
grep -q -- '-section_order needs <segname> <section-list>' $t/log6

not $mold -arch $ARCH -preload -e _start $t/a.o -o $t/exe7 -section_order __DATA : 2> $t/log7
grep -q -- '-section_order should specifify at least one section' $t/log7

# An image dyld loads keeps the usual order.
not $mold -arch $ARCH -e _start $t/a.o -o $t/exe8 -section_order __TEXT __const 2> $t/log8
grep -q -- '-section_order can only be used with -preload, -dylinker, -static, or with -platform_version "firmware"/"sepOS"' $t/log8
