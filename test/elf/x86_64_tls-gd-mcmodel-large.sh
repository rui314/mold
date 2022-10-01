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

cat <<EOF | $GCC -mtls-dialect=gnu -fPIC -c -o $t/a.o -xc - -mcmodel=large
#include <stdio.h>

static _Thread_local int x1 = 1;
static _Thread_local int x2;
extern _Thread_local int x3;
extern _Thread_local int x4;
int get_x5();
int get_x6();

int main() {
  x2 = 2;

  printf("%d %d %d %d %d %d\n", x1, x2, x3, x4, get_x5(), get_x6());
  return 0;
}
EOF

cat <<EOF | $GCC -mtls-dialect=gnu -fPIC -c -o $t/b.o -xc - -mcmodel=large
_Thread_local int x3 = 3;
static _Thread_local int x5 = 5;
int get_x5() { return x5; }
EOF


cat <<EOF | $GCC -mtls-dialect=gnu -fPIC -c -o $t/c.o -xc - -mcmodel=large
_Thread_local int x4 = 4;
static _Thread_local int x6 = 6;
int get_x6() { return x6; }
EOF

$CC -B. -shared -o $t/d.so $t/b.o -mcmodel=large
$CC -B. -shared -o $t/e.so $t/c.o -Wl,--no-relax -mcmodel=large

$CC -B. -o $t/exe $t/a.o $t/d.so $t/e.so -mcmodel=large
$QEMU $t/exe | grep -q '1 2 3 4 5 6'

$CC -B. -o $t/exe $t/a.o $t/d.so $t/e.so -Wl,-no-relax -mcmodel=large
$QEMU $t/exe | grep -q '1 2 3 4 5 6'

echo OK
