#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
#include <stdio.h>
void hello() { printf("Hello world\n"); }
EOF

$CC --ld-path=$mold -shared -o $t/b.dylib $t/a.o
$CC --ld-path=$mold -shared -o $t/c.dylib $t/a.o -Wl,-mark_dead_strippable_dylib

cat <<EOF | $CC -o $t/d.o -c -xc -
int main() {}
EOF

$CC --ld-path=$mold -o $t/exe1 $t/d.o $t/b.dylib
objdump --macho --dylibs-used $t/exe1 | grep -Fq b.dylib

$CC --ld-path=$mold -o $t/exe2 $t/d.o $t/c.dylib
objdump --macho --dylibs-used $t/exe2 > $t/log2
! grep -Fq c.dylib $t/log2 || false
