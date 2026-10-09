#!/bin/bash
source "$(dirname "$0")"/common.inc

# clang -faddrsig names the symbols whose addresses are significant in
# __DATA,__llvm_addrsig, by relocations that all point into its eight
# placeholder bytes. An image drops the section, as mold drops
# .llvm_addrsig; a -r output keeps it, relocations and all.
cat <<EOF | $CC -o $t/a.o -c -xc - -faddrsig
#include <stdio.h>
static int f(void) { return 1; }
int g(void) { return 2; }
int (*p)(void) = f;
int main() { printf("%d %d\n", p(), g()); }
EOF
otool -r $t/a.o | grep -q __llvm_addrsig

$CC --ld-path=$mold -o $t/exe $t/a.o
$RUN $t/exe | grep '^1 2$'
otool -l $t/exe > $t/load
not grep -q __llvm_addrsig $t/load

$mold -r -arch $ARCH -platform_version ${PLATFORM_VERSION:-macos 15.0 15.0} -o $t/b.o $t/a.o
otool -r $t/b.o > $t/relocs
[ "$(sed -n '/__llvm_addrsig/,/^Relocation/p' $t/relocs | grep -c '^00000000')" = 4 ]
$CC --ld-path=$mold -o $t/exe2 $t/b.o
$RUN $t/exe2 | grep '^1 2$'
