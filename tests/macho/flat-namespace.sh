#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() { printf("hi\n"); }
EOF2

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-flat_namespace
otool -hv $t/exe > $t/hdr
not grep -q TWOLEVEL $t/hdr
dyld_info -fixups $t/exe | grep -q 'flat-namespace.*_printf'
$t/exe | grep hi
