#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

int main();

void print() {
  printf("%d\n", (char *)print < (char *)main);
}

int main() {
  print();
}
EOF

cat <<EOF > $t/order1
_print
_main
EOF

cat <<EOF > $t/order2
_main
_print
EOF

$CC --ld-path=$mold -o $t/exe1 $t/a.o -Wl,-order_file,$t/order1
$t/exe1 | grep '^1$'

$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-order_file,$t/order2
$t/exe2 | grep '^0$'
# Arch and object-file qualifiers: lines for other arches are ignored,
# and a file qualifier restricts the match to that object's symbols.
OTHER=x86_64; [ $ARCH = x86_64 ] && OTHER=arm64
cat <<EOF > $t/order3
$OTHER:_main
$ARCH:_print
_main
EOF
$CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-order_file,$t/order3
$t/exe3 | grep '^1$'

cat <<EOF > $t/order4
nosuch.o:_main
a.o:_print
EOF
$CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-order_file,$t/order4
$t/exe4 | grep '^1$'

# The qualifier is a leaf name: one with a directory names no file.
cat <<EOF > $t/order5
$t/a.o:_main
_print
EOF
$CC --ld-path=$mold -o $t/exe5 $t/a.o -Wl,-order_file,$t/order5
$t/exe5 | grep '^1$'

# A line names a subsection by any symbol of it, a C string's or a
# literal's label too. (ld-prime orders only by the names its -map
# gives subsections: a C string is known by its contents there, so its
# label orders nothing.)
cat <<EOF2 | $CC -o $t/s.o -c -xassembler -
.globl _main
.p2align 2
_main: ret
.section __TEXT,__cstring,cstring_literals
l_.str: .asciz "first"
l_.str.1: .asciz "second"
.section __TEXT,__const
lc1: .quad 1
lc2: .quad 2
.subsections_via_symbols
EOF2
printf 'l_.str.1\nlc2\n' > $t/order6
$CC --ld-path=$mold -o $t/exe6 $t/s.o -Wl,-order_file,$t/order6
otool -X -v -s __TEXT __cstring $t/exe6 | head -1 | grep -q 'second$'
[ "$(otool -X -s __TEXT __const $t/exe6 | head -1 | awk '{ print $2 + 0 }')" = 2 ]

# With no -order_file, an Apple-internal SDK's file for the
# -final_output name orders the image: the first -syslibroot's
# AppleInternal/OrderFiles/<name>.order.
sdk=$(xcrun --show-sdk-path)
rm -rf $t/sdk
mkdir -p $t/sdk/AppleInternal/OrderFiles
ln -s $sdk/usr $t/sdk/usr
ln -s $sdk/System $t/sdk/System
cp $t/order2 $t/sdk/AppleInternal/OrderFiles/final7.order
$CC --ld-path=$mold -o $t/exe7 $t/a.o -isysroot $t/sdk -Wl,-final_output,final7
$t/exe7 | grep '^0$'
$CC --ld-path=$mold -o $t/exe8 $t/a.o -isysroot $t/sdk -Wl,-final_output,final7 \
  -Wl,-order_file,$t/order1
$t/exe8 | grep '^1$'
