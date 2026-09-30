#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() { printf("hi\n"); }
EOF2

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-flat_namespace
otool -hv $t/exe > $t/hdr
not grep -q TWOLEVEL $t/hdr
# Flat lookups are resolved by dyld, so the image does not claim
# MH_NOUNDEFS either: DYLDLINK|PIE only, as ld-prime writes it.
[ "$(otool -h $t/exe | tail -1 | awk '{print $NF}')" = 0x00200004 ]
dyld_info -fixups $t/exe | grep 'flat-namespace.*_printf'
$t/exe | grep hi
