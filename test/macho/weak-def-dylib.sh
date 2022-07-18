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
int foo() __attribute__((weak));

int foo() {
  return 3;
}
EOF

clang --ld-path=./ld64 -shared -o $t/b.dylib $t/a.o

cat <<EOF | $CC -c -o $t/c.o -xc -
#include <stdio.h>

int foo() __attribute((weak));

int main() {
  printf("%d\n", foo ? foo() : 42);
}
EOF

clang --ld-path=./ld64 -o $t/exe $t/b.dylib $t/c.o
$t/exe | grep -q '^3$'

clang -c -o $t/d.o -xc /dev/null
clang --ld-path=./ld64 -shared -o $t/b.dylib $t/d.o
$t/exe | grep -q '^42$'

echo OK
