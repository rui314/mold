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

cat <<EOF | $CC -fPIC -shared -o $t/a.so -xc -
extern _Thread_local int foo;
_Thread_local int bar;

int get_foo1() { return foo; }
int get_bar1() { return bar; }
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
#include <stdio.h>

_Thread_local int foo;
extern _Thread_local int bar;

int get_foo1();
int get_bar1();

int get_foo2() { return foo; }
int get_bar2() { return bar; }

int main() {
  foo = 5;
  bar = 3;
  printf("%d %d %d %d %d %d\n",
         foo, bar,
         get_foo1(), get_bar1(),
         get_foo2(), get_bar2());
  return 0;
}
EOF

$CC -B. -o $t/exe $t/a.so $t/b.o
$QEMU $t/exe | grep -q '5 3 5 3 5 3'

echo OK
