#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -fPIC -o$t/a.o -xc -
int foo = 4;
int get_foo() { return foo; }
EOF

$CC -B. -shared -fPIC -o $t/b.so $t/a.o -Wl,-Bsymbolic

cat <<EOF | $CC -c -o $t/c.o -xc - -fno-PIE
#include <stdio.h>

int foo = 3;
int get_foo();

int main() {
  printf("%d %d\n", foo, get_foo());
}
EOF

$CC -B. -no-pie -o $t/exe $t/c.o $t/b.so
$QEMU $t/exe | grep '3 4'
