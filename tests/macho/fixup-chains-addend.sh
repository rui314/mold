#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int arr[5] = { 1, 2, 3, 4, 5 };
EOF

$CC --ld-path=$mold -shared -o $t/b.dylib $t/a.o

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>

extern int arr[5];

int *p1 = arr + (1 << 10);

int main() {
  printf("%d %d\n", arr[0], *(p1 - (1 << 10)));
}
EOF

$CC --ld-path=$mold -o $t/exe1 $t/c.o $t/b.dylib -Wl,-fixup_chains
$t/exe1
$t/exe1 | grep -q '^1 1$'

$CC --ld-path=$mold -o $t/exe2 $t/c.o $t/b.dylib -Wl,-no_fixup_chains
$t/exe2
$t/exe2 | grep -q '^1 1$'
