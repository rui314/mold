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

cat <<EOF | $CC -fcommon -xc -c -o $t/a.o -
int foo;
int bar;
int baz = 42;
EOF

cat <<EOF | $CC -fcommon -xc -c -o $t/b.o -
#include <stdio.h>

int foo;
int bar = 5;
int baz;

int main() {
  printf("%d %d %d\n", foo, bar, baz);
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep -q '0 5 42'

readelf --sections $t/exe > $t/log
grep -q '.common .*NOBITS' $t/log

echo OK
