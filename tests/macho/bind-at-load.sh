#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() { printf("hi\n"); }
EOF2

# ld-prime honors -bind_at_load by calling imported functions through
# GOT slots bound at load instead of lazy pointers; it does not set
# MH_BINDATLOAD (dyld binds everything at load regardless).
if [ $ARCH = arm64 ]; then classic=11.0; else classic=12.0; fi
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-bind_at_load -mmacosx-version-min=$classic
otool -hv $t/exe > $t/hdr
not grep -q BINDATLOAD $t/hdr
otool -l $t/exe > $t/lc
not grep -q '__la_symbol_ptr' $t/lc
dyld_info -fixups $t/exe | grep -q '__got .* bind .*_printf'
$t/exe | grep hi
