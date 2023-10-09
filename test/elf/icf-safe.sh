#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $GCC -c -o $t/a.o -ffunction-sections -fdata-sections -xc -
int bar() {
  return 5;
}

int foo1(int x) {
  return bar() + x;
}

int foo2(int x) {
  return bar() + x;
}

int foo3(int x) {
  return bar() + x;
}
EOF

cat <<EOF | $CC -c -o $t/b.o -ffunction-sections -fdata-sections -xc -
#include <stdio.h>

int foo1();
int foo2();
int foo3();

int main() {
  printf("%d %d\n", foo1 == foo2, foo2 == foo3);
}
EOF

$CC -B. -o $t/exe1 -Wl,-icf=safe $t/a.o $t/b.o
$QEMU $t/exe1 | grep -q '^0 0$'

cat <<EOF | $GCC -c -o $t/c.o -ffunction-sections -fdata-sections -xc -
int foo1();
int foo2();
int foo3();

int main() {
  foo1();
  foo2();
  foo3();
}
EOF

$CC -B. -o $t/exe2 -Wl,-icf=safe $t/a.o $t/c.o
$QEMU $t/exe2 > $t/log2
! grep foo2 $t/log2 || false

