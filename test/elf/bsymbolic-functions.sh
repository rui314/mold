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

cat <<EOF | $CC -c -o $t/a.o -fPIC -xc -
int foo = 4;

int get_foo() { return foo; }
void *bar() { return bar; }
EOF

$CC -B. -shared -o $t/b.so $t/a.o -Wl,-Bsymbolic-functions

cat <<EOF | $CC -c -o $t/c.o -xc - -fno-PIE
#include <stdio.h>

int foo = 3;
int x = 5;
int get_foo();
void *bar() { return &x; }

int main() {
  printf("%d %d %d\n", foo, get_foo(), bar == bar());
}
EOF

$CC -B. -no-pie -o $t/exe $t/c.o $t/b.so
$QEMU $t/exe | grep -q '3 3 0'

echo OK
