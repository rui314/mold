#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe1 $t/a.o -fno-PIE
readelf -W --dyn-syms $t/exe1 | grep -F '__global_pointer$'

$CC -B. -o $t/exe2 $t/a.o -fPIE
readelf -W --dyn-syms $t/exe2 | grep -F '__global_pointer$'

cat <<EOF | $CC -o $t/b.o -c -xc - -fPIC
#include <stdio.h>
int hello() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/c.so $t/b.o -shared
readelf -W --dyn-syms $t/c.so | not grep -F '__global_pointer$'
