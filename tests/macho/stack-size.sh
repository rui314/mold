#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int main() {}
EOF2

$CC --ld-path=$mold -o $t/exe1 $t/a.o
otool -l $t/exe1 | grep -q 'stacksize 0$'

$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-stack_size,200000
otool -l $t/exe2 | grep -q 'stacksize 2097152$'
$t/exe2
