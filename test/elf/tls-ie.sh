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

__attribute__((tls_model("initial-exec"))) static _Thread_local int foo;
__attribute__((tls_model("initial-exec"))) static _Thread_local int bar;

void set() {
  foo = 3;
  bar = 5;
}

void print() {
  printf("%d %d ", foo, bar);
}
EOF

$CC -B. -shared -o $t/b.so $t/a.o

cat <<EOF | $GCC -c -o $t/c.o -xc -
#include <stdio.h>

_Thread_local int baz;

void set();
void print();

int main() {
  baz = 7;
  print();
  set();
  print();
  printf("%d\n", baz);
}
EOF

$CC -B. -o $t/exe $t/b.so $t/c.o
$QEMU $t/exe | grep -q '^0 0 3 5 7$'

$CC -B. -o $t/exe $t/b.so $t/c.o -Wl,-no-relax
$QEMU $t/exe | grep -q '^0 0 3 5 7$'

echo OK
