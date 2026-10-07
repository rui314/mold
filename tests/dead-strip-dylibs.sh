#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() { printf("hi\n"); }
EOF2

$CC --ld-path=$mold -o $t/exe1 $t/a.o -framework CoreFoundation
otool -L $t/exe1 | grep CoreFoundation

$CC --ld-path=$mold -o $t/exe2 $t/a.o -framework CoreFoundation \
  -Wl,-dead_strip_dylibs
otool -L $t/exe2 > $t/libs
not grep -q CoreFoundation $t/libs
$RUN $t/exe2 | grep hi
