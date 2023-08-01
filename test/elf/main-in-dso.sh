#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -shared -o $t/b.so $t/a.o

$CC -o $t/c.o -c -xc /dev/null -fPIC
$CC -B. -o $t/exe1 $t/c.o $t/b.so -pie
$QEMU $t/exe1 | grep -q 'Hello world'

$CC -o $t/c.o -c -xc /dev/null -fno-PIC
$CC -B. -o $t/exe2 $t/c.o $t/b.so -no-pie
$QEMU $t/exe2 | grep -q 'Hello world'
