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
t=out/test/macho/$MACHINE/$testname
mkdir -p $t

cat <<EOF | $CC -c -o $t/a.o -xc -
#include <stdio.h>
char world[] = "world";

char *hello() {
  return "Hello";
}
EOF

clang --ld-path=./ld64 -o $t/b.dylib -shared $t/a.o

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>

char *hello();
extern char world[];

int main() {
  printf("%s %s\n", hello(), world);
}
EOF

clang --ld-path=./ld64 -o $t/exe $t/c.o $t/b.dylib
$t/exe | grep -q 'Hello world'

echo OK
