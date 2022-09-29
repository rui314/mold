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

echo 'int main() {}' | $CC -o /dev/null -xc - -static >& /dev/null || \
  { echo skipped; exit; }

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

__attribute__((aligned(512)))
char hello[] = "Hello";

__attribute__((aligned(512)))
char world[] = "world";

int main() {
  printf("%s %s\n", hello, world);
}
EOF

$CC -B. -static -Wl,--filler,0xfe -o $t/exe1 $t/a.o
sed -i -e 's/--filler 0xfe/--filler 0x00/' $t/exe1
hexdump -C $t/exe1 > $t/txt1

$CC -B. -static -Wl,--filler,0x00 -o $t/exe2 $t/a.o
hexdump -C $t/exe2 > $t/txt2

diff -q $t/txt1 $t/txt2

echo OK
