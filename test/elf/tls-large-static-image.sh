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

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
_Thread_local int x[] = { 1, 2, 3, [10000] = 5 };
EOF

cat <<EOF | $CC -fPIC -c -o $t/b.o -xc -
#include <stdio.h>
extern _Thread_local int x[];

int main() {
  printf("%d %d %d %d %d\n", x[0], x[1], x[2], x[3], x[10000]);
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep -q '^1 2 3 0 5$'

echo OK

