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

cat <<EOF | $CC -o $t/a.o -c -xc -
char msg[] = "Hello world\n";
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>

extern char msg[];

int main() {
  printf("%s\n", msg);
}
EOF

clang --ld-path=./ld64 -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q 'Hello world'

echo OK
