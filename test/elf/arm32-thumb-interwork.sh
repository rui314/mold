#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/elf/$MACHINE/$testname
mkdir -p $t

[ $MACHINE == arm* ] || { echo skipped; exit; }

echo 'int foo() { return 0; }' | $CC -o /dev/null -c -xc - -mthumb 2> /dev/null || { echo skipped; exit; }

cat <<EOF | $CC -o $t/a.o -c -xc - -mthumb
#include <stdio.h>
int bar();
int foo() {
  printf(" foo");
  bar();
}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -marm
#include <stdio.h>
int bar() {
  printf(" bar\n");
}

int foo();

int main() {
  printf("main");
  foo();
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep -q 'main foo bar'

echo OK
