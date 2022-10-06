#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -O2 -o $t/a.o -c -xc -
int add1(int n) { return n + 1; }
EOF

cat <<EOF | $CC -O2 -o $t/b.o -c -xc -
int add1(int n);
int add2(int n) { n += 1; return add1(n); }
EOF

cat <<EOF | $CC -O2 -o $t/c.o -c -xc -
#include <stdio.h>
int add2(int n);

int main() {
  printf("%d\n", add2(40));
  return 0;
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o $t/c.o
$QEMU $t/exe | grep -q '42'

if [ $MACHINE = riscv32 -o $MACHINE = riscv64 ]; then
  ${TEST_TRIPLE}objdump -d $t/exe | grep -q bfed # c.j pc - 6
fi
