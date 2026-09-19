#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() { printf("Hello world\n"); }
EOF2

$CC --ld-path=$mold -o $t/exe $t/a.o
cp $t/exe $t/exe1
$CC --ld-path=$mold -o $t/exe $t/a.o
cp $t/exe $t/exe2
diff $t/exe1 $t/exe2
