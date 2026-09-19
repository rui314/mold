#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <stdio.h>
__attribute__((constructor)) static void ctor() { printf("ctor "); }
int main() { printf("main\n"); }
EOF2

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-init_offsets
otool -l $t/exe > $t/lc
grep -q __init_offsets $t/lc
not grep -q __mod_init_func $t/lc
$t/exe | grep '^ctor main$'
