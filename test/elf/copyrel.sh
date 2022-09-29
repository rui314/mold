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

cat <<EOF | $CC -fno-PIC -o $t/a.o -c -xc -
#include <stdio.h>

extern int foo;
extern int *get_bar();

int main() {
  printf("%d %d %d\n", foo, *get_bar(), &foo == get_bar());
  return 0;
}
EOF

cat <<EOF | $CC -fno-PIC -o $t/b.o -c -xc -
extern int bar;
int *get_bar() { return &bar; }
EOF

cat <<EOF | $CC -fPIC -o $t/c.o -c -xc -
int foo = 42;
extern int bar __attribute__((alias("foo")));
extern int baz __attribute__((alias("foo")));
EOF

$CC -B. -shared -o $t/c.so $t/c.o
$CC -B. -no-pie -o $t/exe $t/a.o $t/b.o $t/c.so
$QEMU $t/exe | grep -q '42 42 1'

echo OK
