#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
#include <stdio.h>
int foo() __attribute__((weak));
int main() {
  printf("%d\n", foo ? foo() : 5);
}
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
int foo() { return 42; }
EOF

$CC --ld-path=$mold -o $t/exe1 $t/a.o -Wl,-U,_foo
$t/exe1 | grep -q '^5$'

$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/b.o -Wl,-U,_foo
$t/exe2 | grep -q '^42$'
