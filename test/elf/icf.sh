#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -ffunction-sections -fdata-sections -xc -
#include <stdio.h>

int bar() {
  return 5;
}

int foo1(int x) {
  return bar() + x;
}

int foo2(int x) {
  return bar() + x;
}

int foo3() {
  bar();
  return 5;
}

int main() {
  printf("%d %d\n", (long)foo1 == (long)foo2, (long)foo1 == (long)foo3);
  return 0;
}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,-icf=all
$QEMU $t/exe | grep -q '1 0'
