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

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC
#include <stdio.h>

void ignore(void *foo) {}

void hello() {
  printf("Hello world\n");
}
EOF

$CC -B. -shared -o $t/b.so $t/a.o

cat <<EOF | $CC -o $t/c.o -c -xc - -fPIC
void ignore(void *);
int hello();

void foo() { ignore(hello); }

int main() { hello(); }
EOF

$CC -B. -o $t/exe $t/c.o $t/b.so
$QEMU $t/exe | grep -q 'Hello world'

echo OK
