#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
#include <stdio.h>

__attribute__((weak)) int foo();

int main() {
  printf("%d\n", foo ? foo() : 3);
}
EOF

$CC -B. -o $t/b.so $t/a.o -shared
$CC -B. -o $t/c.so $t/a.o -shared -Wl,-z,defs

readelf --dyn-syms $t/b.so | grep 'WEAK   DEFAULT  UND foo'
readelf --dyn-syms $t/c.so | grep 'WEAK   DEFAULT  UND foo'
