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

[ $MACHINE = x86_64 ] || { echo skipped; exit; }

cat <<EOF | $CC -fPIC -o $t/a.o -c -xc -
#include <stdio.h>
void hello() { printf("Hello"); }
void world() { printf("world"); }
EOF

$CC -B. -o $t/b.so -shared $t/a.o -Wl,-z,ibtplt

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>

void hello();
void world();

int main() {
  hello();
  printf(" ");
  world();
  printf("\n");
}
EOF

$CC -B. -o $t/exe $t/c.o $t/b.so -Wl,-z,ibtplt
$QEMU $t/exe | grep -q 'Hello world'

echo OK
