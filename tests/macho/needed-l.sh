#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() { printf("hi\n"); }
EOF2

# -needed_framework survives -dead_strip_dylibs even when unused.
$CC --ld-path=$mold -o $t/exe $t/a.o \
  -Wl,-needed_framework,CoreFoundation -Wl,-dead_strip_dylibs
otool -L $t/exe | grep -q CoreFoundation
$t/exe | grep hi
