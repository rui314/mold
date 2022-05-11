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
cd "$(dirname "$0")"/../..
t=out/test/elf/$testname
mkdir -p $t

cat <<EOF | $CC -o $t/a.o -c -xc -
int two() { return 2; }
EOF

head -c 1 /dev/zero >> $t/a.o

cat <<EOF | $CC -o $t/b.o -c -xc -
int three() { return 3; }
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>

int two();
int three();

int main() {
  printf("%d\n", two() + three());
}
EOF

rm -f $t/d.a
ar rcs $t/d.a $t/a.o $t/b.o

$CC -B. -o $t/exe $t/c.o $t/d.a

echo OK
