#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <stdio.h>
void hello() { printf("hi\n"); }
int main() { hello(); }
EOF2

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-map,$t/map
$t/exe | grep hi
grep -q '# Object files:' $t/map
grep -q 'a.o' $t/map
grep -q '__TEXT.*__text' $t/map
grep -q '_hello' $t/map
grep -q '_main' $t/map
