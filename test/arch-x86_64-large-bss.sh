#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -mcmodel=large
#include <stdio.h>
char arr1[0xc0000000];
extern char arr2[0xc0000000];
int main() {
  printf("%d %lx\n", (void *)arr1 < (void *)arr2, arr2 - arr1);
}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -mcmodel=large
char arr2[0xc0000000];
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep -E '^1 c0000000$'
