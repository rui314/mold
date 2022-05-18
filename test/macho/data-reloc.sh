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
cd "$(dirname "$0")"/../..
t=out/test/macho/$testname
mkdir -p $t

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC
#include <stdio.h>

int a = 5;
int *b = &a;

void print() {
  printf("%d %d\n", a, *b);
}
EOF

clang --ld-path=./ld64 -shared -o $t/b.dylib $t/a.o

cat <<EOF | $CC -o $t/c.o -c -xc - -fPIC
void print();
int main() { print(); }
EOF

clang --ld-path=./ld64 -o $t/exe $t/b.dylib $t/c.o
$t/exe | grep -q '^5 5$'

echo OK
