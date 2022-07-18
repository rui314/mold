#!/bin/bash
export LC_ALL=C
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
GCC="${GCC:-gcc}"
GXX="${GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$MACHINE/$testname
mkdir -p $t

cat <<EOF | $CC -c -o $t/a.o -xc -
#include <stdio.h>

int foo() __attribute__((weak));

int foo() {
  return 3;
}

int main() {
  printf("%d\n", foo());
}
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
int foo() { return 42; }
EOF

clang --ld-path=./ld64 -o $t/exe1 $t/a.o
$t/exe1 | grep -q '^3$'

clang --ld-path=./ld64 -o $t/exe1 $t/a.o $t/b.o
$t/exe1 | grep -q '^42$'

echo OK
