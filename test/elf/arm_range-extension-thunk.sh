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

[[ $MACHINE = arm* ]] || { echo skipped; exit; }

echo 'int main() {}' | $CC -c -o /dev/null -xc - -O0 -mthumb >& /dev/null \
  || { echo skipped; exit; }

cat <<EOF > $t/a.c
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

cat <<EOF > $t/b.c
#include <stdio.h>

void fn1();
void fn2();

__attribute__((section(".high"))) void fn3() { printf(" fn3"); fn2(); }
__attribute__((section(".high"))) void fn4() { printf(" fn4"); }
EOF

$CC -c -o $t/c.o $t/a.c -O0 -mthumb
$CC -c -o $t/d.o $t/b.c -O0 -marm

$CC -B. -o $t/exe $t/c.o $t/d.o \
  -Wl,--section-start=.low=0x10000000,--section-start=.high=0x20000000
$QEMU $t/exe | grep -q 'main fn1 fn3 fn2 fn4'

$CC -c -o $t/e.o $t/a.c -O2 -mthumb
$CC -c -o $t/f.o $t/b.c -O2 -marm

$CC -B. -o $t/exe $t/e.o $t/f.o \
  -Wl,--section-start=.low=0x10000000,--section-start=.high=0x20000000
$QEMU $t/exe | grep -q 'main fn1 fn3 fn2 fn4'

echo OK
