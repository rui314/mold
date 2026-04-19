#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
#include <stdio.h>

__attribute__((weak)) int foo();

int main() {
  printf("%d\n", foo ? foo() : -1);
}
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
int foo();
int bar() { return foo(); }
EOF

not $CC -B. -o $t/exe $t/a.o $t/b.o |& grep 'undefined symbol: foo'
