#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -B. -shared -fPIC -o $t/a.so -xc -
__attribute__((weak))
int foo() { return 5; }
EOF

cat <<EOF | $CC -B. -shared -fPIC -o $t/b.so -xc -
int foo();
int bar() { return foo(); }
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
int foo() { return 8; }
EOF

rm -f $t/d.a
ar rcs $t/d.a $t/c.o

cat <<EOF | $CC -o $t/e.o -c -xc -
#include <stdio.h>
int foo();
int main() {
  printf("%d\n", foo());
}
EOF

$CC -B. -o $t/exe $t/a.so $t/b.so $t/d.a $t/e.o
$QEMU $t/exe | grep -q '^5$'
