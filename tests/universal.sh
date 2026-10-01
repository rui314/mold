#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

lipo $t/a.o -create -output $t/fat.o

cat <<EOF | $CC -o $t/b.o -c -xc -
void hello();
int main() {
  hello();
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o
$t/exe | grep 'Hello world'

# ld-prime names an object of a fat file in a diagnostic by the fat
# file's real path, and a member of a fat archive by the archive's,
# the slice's architecture and the member's position.
cat <<EOF2 | $CC -o $t/c.o -c -xassembler -
.data
.globl _c0, _c1
_c0: .byte 0
_c1: .quad _main
.subsections_via_symbols
EOF2
lipo $t/c.o -create -output $t/fatc.o
rm -f $t/libc.a
ar rcs $t/libc.a $t/c.o
lipo $t/libc.a -create -output $t/libfatc.a
$CC --ld-path=$mold -o $t/exe2 $t/b.o $t/a.o $t/fatc.o $t/libfatc.a \
  -Wl,-no_fixup_chains 2> $t/log2
grep -q "atom '_c1' (/.*/$t/fatc.o) is too small" $t/log2
grep -q "atom '_c1' (/.*/$t/libfatc.a\[$ARCH\]\[2\](c.o)) is too small" $t/log2
