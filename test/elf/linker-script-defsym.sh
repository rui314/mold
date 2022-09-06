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

cat <<EOF | $CC -o $t/a.o -c -xc -
int foo() { return 42; }
EOF

cat <<EOF > $t/script
bar = foo;
EOF

$CC -B. -o $t/b.so -shared $t/script $t/a.o
readelf -sW $t/b.so | grep -q 'FUNC .* bar'

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>
int bar();
int main() {
  printf("%d\n", bar());
  return 0;
}
EOF

$CC -B. -o $t/exe $t/c.o $t/b.so
$QEMU $t/exe | grep -q 42

echo OK
