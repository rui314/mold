#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int three() { return 3; }
EOF2

cat <<EOF2 | $CC -o $t/b.o -c -xc -
#include <stdio.h>
int three();
int main() { printf("%d\n", three()); }
EOF2

printf '%s\n%s\n' $t/a.o $t/b.o > $t/list
$CC --ld-path=$mold -o $t/exe -Wl,-filelist,$t/list
$t/exe | grep '^3$'
