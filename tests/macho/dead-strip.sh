#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <stdio.h>
void live() { printf("live\n"); }
void dead() { printf("dead\n"); }
int dead_data[1000] = {3};
int main() { live(); }
EOF2

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-dead_strip
$t/exe | grep '^live$'

nm $t/exe > $t/syms
grep -q _live $t/syms
not grep -q ' _dead' $t/syms
