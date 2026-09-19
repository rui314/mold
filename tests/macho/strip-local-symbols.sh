#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 > $t/a.c
#include <stdio.h>
static void hello() { printf("Hello world\n"); }
int main(){ hello(); }
EOF2

$CC -o $t/a.o -c $t/a.c

$CC --ld-path=$mold -o $t/exe1 $t/a.o
nm $t/exe1 | grep -qw _hello

$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-x
nm $t/exe2 > $t/log2
not grep -qw _hello $t/log2
$t/exe2 | grep 'Hello world'
