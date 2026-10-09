#!/bin/bash
source "$(dirname "$0")"/common.inc

# The segments of signed pointers are standard ones to ld-prime:
# __AUTH_CONST then __AUTH go after __DATA_CONST and before __DATA, in
# whatever order the inputs name them, in an image dyld or kmutil
# loads. A -static image orders them as any other segment, as first
# seen after __DATA.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
_main:
  ret
.section __DATA,__const
.p2align 3
.quad _main
.data
.quad 1
EOF
printf '%016d' 0 > $t/blob
sects="-Wl,-sectcreate,__FOO,__x,$t/blob -Wl,-sectcreate,__AUTH,__x,$t/blob"
sects="$sects -Wl,-sectcreate,__AUTH_CONST,__x,$t/blob"
segs() { otool -l $1 | awk '$1 == "segname" { print $2 }' | uniq | tr '\n' ' '; }

$CC --ld-path=$mold -o $t/exe $t/a.o $sects
[ "$(segs $t/exe)" = '__PAGEZERO __TEXT __DATA_CONST __AUTH_CONST __AUTH __DATA __FOO __LINKEDIT ' ]

$CC --ld-path=$mold -o $t/b.dylib -shared $t/a.o $sects
[ "$(segs $t/b.dylib)" = '__TEXT __DATA_CONST __AUTH_CONST __AUTH __DATA __FOO __LINKEDIT ' ]

$mold -arch $ARCH -static -e _main -o $t/exe2 $t/a.o -sectcreate __FOO __x $t/blob \
  -sectcreate __AUTH __x $t/blob -sectcreate __AUTH_CONST __x $t/blob
[ "$(segs $t/exe2)" = '__PAGEZERO __TEXT __DATA __FOO __AUTH __AUTH_CONST __LINKEDIT ' ]
