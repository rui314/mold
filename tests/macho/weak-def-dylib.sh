#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
int foo() __attribute__((weak));

int foo() {
  return 3;
}
EOF

$CC --ld-path=$mold -shared -o $t/b.dylib $t/a.o

cat <<EOF | $CC -c -o $t/c.o -xc -
#include <stdio.h>

int foo() __attribute((weak));

int main() {
  printf("%d\n", foo ? foo() : 42);
}
EOF

$CC --ld-path=$mold -o $t/exe $t/b.dylib $t/c.o
$t/exe | grep -q '^3$'

$CC -c -o $t/d.o -xc /dev/null
$CC --ld-path=$mold -shared -o $t/b.dylib $t/d.o
$t/exe | grep -q '^42$'
