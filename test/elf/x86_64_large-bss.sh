#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = x86_64 ] || skip

cat <<EOF | $CC -o $t/a.o -c -xc - 2> /dev/null || skip
#include <stdio.h>
char arr1[0xc0000000];
extern char arr2[0xc0000000];
int main() {
  printf("%d %lx\n", arr1 < arr2, arr2 - arr1);
}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - 2> /dev/null || skip
char arr2[0xc0000000];
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep -Eq '^1 c0000000$'
