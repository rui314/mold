#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int main() {}
EOF2

$CC --ld-path=$mold -o $t/exe1 $t/a.o
otool -l $t/exe1 | grep -q LC_FUNCTION_STARTS

$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-no_function_starts
otool -l $t/exe2 > $t/lc
not grep -q LC_FUNCTION_STARTS $t/lc
$t/exe2
