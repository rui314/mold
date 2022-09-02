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

[[ $MACHINE = aarch64 ]] || { echo skipped; exit; }

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

void fn3();
void fn4();

__attribute__((section(".low"))) void fn1() { printf(" fn1"); fn3(); }
__attribute__((section(".low"))) void fn2() { printf(" fn2"); fn4(); }

int main() {
  printf(" main");
  fn1();
  printf("\n");
}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>

void fn1();
void fn2();

__attribute__((section(".high"))) void fn3() { printf(" fn3"); fn2(); }
__attribute__((section(".high"))) void fn4() { printf(" fn4"); }
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o \
  -Wl,--section-start=.low=0x10000000,--section-start=.high=0x20000000
$QEMU $t/exe | grep -q 'main fn1 fn3 fn2 fn4'

echo OK
