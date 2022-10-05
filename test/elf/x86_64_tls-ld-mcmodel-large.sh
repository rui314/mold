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

cat <<EOF | $GCC -ftls-model=local-dynamic -fPIC -c -o $t/a.o -xc - -mcmodel=large
#include <stdio.h>

extern _Thread_local int foo;
static _Thread_local int bar;

int *get_foo_addr() { return &foo; }
int *get_bar_addr() { return &bar; }

int main() {
  bar = 5;

  printf("%d %d %d %d\n", *get_foo_addr(), *get_bar_addr(), foo, bar);
  return 0;
}
EOF

cat <<EOF | $GCC -ftls-model=local-dynamic  -fPIC -c -o $t/b.o -xc - -mcmodel=large
_Thread_local int foo = 3;
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o -mcmodel=large
$QEMU $t/exe | grep -q '3 5 3 5'

$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,-no-relax -mcmodel=large
$QEMU $t/exe | grep -q '3 5 3 5'

echo OK
