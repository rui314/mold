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

[ $MACHINE = aarch64 ] || { echo skipped; exit; }

cat <<EOF | $CC -c -o $t/a.o -fPIC -xc -
#include <stdio.h>

void fn1();
void fn2();

__attribute__((section(".low")))  void fn1() { fn2(); }
__attribute__((section(".high"))) void fn2() { fn1(); }

int main() {
  fn1();
}
EOF

$CC -B. -o $t/exe $t/a.o \
  -Wl,--section-start=.low=0x10000000,--section-start=.high=0x20000000

${TEST_TRIPLE}objdump -dr $t/exe | grep -Fq '<fn1$thunk>:'

echo OK
