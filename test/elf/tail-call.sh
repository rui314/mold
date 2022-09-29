#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/elf/$MACHINE/$testname
mkdir -p $t

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
  $OBJDUMP -d $t/exe | grep -q bfed # c.j pc - 6
fi

echo OK
