#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -c -o $t/a.o -xc -
#include <stdio.h>
int foo() __attribute__((weak));
int foo() { return 3; }
int main() { printf("%d\n", foo()); }
EOF2

cat <<EOF2 | $CC -c -o $t/b.o -xc -
int foo() { return 42; }
EOF2

$CC --ld-path=$mold -o $t/exe1 $t/a.o
$t/exe1 | grep '^3$'

$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/b.o
$t/exe2 | grep '^42$'
