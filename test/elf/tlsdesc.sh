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

if [ $MACHINE = x86_64 ]; then
  dialect=gnu2
elif [ $MACHINE = aarch64 ]; then
  dialect=desc
else
  echo skipped
  exit
fi

cat <<EOF | $GCC -fPIC -mtls-dialect=$dialect -c -o $t/a.o -xc -
extern _Thread_local int foo;

int get_foo() {
  return foo;
}

static _Thread_local int bar = 5;

int get_bar() {
  return bar;
}
EOF

cat <<EOF | $GCC -fPIC -mtls-dialect=$dialect -c -o $t/b.o -xc -
#include <stdio.h>

_Thread_local int foo;

int get_foo();
int get_bar();

int main() {
  foo = 42;
  printf("%d %d\n", get_foo(), get_bar());
  return 0;
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep -q '42 5'

$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,-no-relax
$QEMU $t/exe | grep -q '42 5'

$CC -B. -shared -o $t/c.so $t/a.o
$CC -B. -o $t/exe $t/b.o $t/c.so
$QEMU $t/exe | grep -q '42 5'

$CC -B. -shared -o $t/c.so $t/a.o -Wl,-no-relax
$CC -B. -o $t/exe $t/b.o $t/c.so -Wl,-no-relax
$QEMU $t/exe | grep -q '42 5'

echo OK
