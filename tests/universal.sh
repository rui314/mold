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
for x in c d; do
  cat <<EOF2 | $CC -o $t/$x.o -c -xassembler -
.data
.globl _${x}0, _${x}1
_${x}0: .byte 0
_${x}1: .quad _main
.subsections_via_symbols
EOF2
done
lipo $t/c.o -create -output $t/fatc.o
rm -f $t/libd.a
ar rcs $t/libd.a $t/d.o
lipo $t/libd.a -create -output $t/libfatd.a
$CC --ld-path=$mold -o $t/exe2 $t/b.o $t/a.o $t/fatc.o $t/libfatd.a -Wl,-u,_d1 \
  -Wl,-no_fixup_chains 2> $t/log2
grep -q "pointer not aligned.*'_c1' (/.*/$t/fatc.o)" $t/log2
grep -q "pointer not aligned.*'_d1' (/.*/$t/libfatd.a\[$ARCH\]\[2\](d.o))" $t/log2
