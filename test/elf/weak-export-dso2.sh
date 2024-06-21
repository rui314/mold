#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
int foo() { return 0; }
EOF

$CC -B. -o $t/b.so $t/a.o -shared

cat <<EOF | $CC -fPIC -c -o $t/c.o -xc -
#include <stdio.h>

__attribute__((weak)) int foo();

int main() {
  printf("%d\n", foo ? foo() : 3);
}
EOF

$CC -B. -o $t/d.so $t/c.o $t/b.so -shared
readelf -W --dyn-syms $t/d.so | grep -q 'WEAK   DEFAULT .* UND foo'
