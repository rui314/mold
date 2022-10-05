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

cat <<EOF | $GCC -fPIC -c -o $t/a.o -xc -
#include <stdio.h>

__attribute__((tls_model("local-dynamic"))) extern _Thread_local int foo;
__attribute__((tls_model("local-dynamic"))) static _Thread_local int bar;

int *get_foo_addr() { return &foo; }
int *get_bar_addr() { return &bar; }

int main() {
  bar = 5;

  printf("%d %d %d %d\n", *get_foo_addr(), *get_bar_addr(), foo, bar);
  return 0;
}
EOF

cat <<EOF | $GCC -fPIC -c -o $t/b.o -xc -
__attribute__((tls_model("local-dynamic"))) _Thread_local int foo = 3;
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep -q '3 5 3 5'

$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,-no-relax
$QEMU $t/exe | grep -q '3 5 3 5'

echo OK
