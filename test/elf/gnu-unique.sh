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

command -v $GXX >& /dev/null || { echo skipped; exit; }

cat <<EOF | $GXX -o $t/a.o -c -std=c++17 -fno-gnu-unique -xc++ -
inline int foo = 5;
int bar() { return foo; }
EOF

cat <<EOF | $GXX -o $t/b.o -c -std=c++17 -fgnu-unique -xc++ -
#include <stdio.h>

inline int foo = 5;

int main() {
  printf("foo=%d\n", foo);
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep -q 'foo=5'

echo OK
