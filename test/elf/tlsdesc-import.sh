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
#include <stdio.h>

extern _Thread_local int foo;
extern _Thread_local int bar;

int main() {
  bar = 7;
  printf("%d %d\n", foo, bar);
}
EOF

cat <<EOF | $GCC -fPIC -mtls-dialect=$dialect -shared -o $t/b.so -xc -
_Thread_local int foo = 5;
_Thread_local int bar;
EOF

$CC -B. -o $t/exe $t/a.o $t/b.so
$QEMU $t/exe | grep -q '5 7'

echo OK
