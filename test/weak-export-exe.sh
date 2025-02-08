#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
#include <stdio.h>

__attribute__((weak)) int foo();

int main() {
  printf("%d\n", foo ? foo() : 3);
}
EOF

$CC -B. -o $t/exe $t/a.o
readelf --dyn-syms $t/exe | not grep -q 'WEAK   DEFAULT  UND foo'
$QEMU $t/exe | grep -q '^3$'
