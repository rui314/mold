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
#include <stdio.h>

void world() {
  printf("world\n");
}

void real_hello() {
  printf("Hello ");
  world();
}

void hello() {
  real_hello();
}
EOF

$CC -B. -shared -o $t/b.so $t/a.o

cat <<EOF | $CC -c -o $t/c.o -xc -
#include <stdio.h>

void world() {
  printf("WORLD\n");
}

void hello();

int main() {
  hello();
}
EOF

$CC -B. -o $t/exe -Wl,-rpath=$t $t/c.o $t/b.so
$QEMU $t/exe | grep -q 'Hello WORLD'

echo OK
