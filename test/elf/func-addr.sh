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

cat <<EOF | $CC -shared -o $t/a.so -xc -
void fn() {}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -fno-PIC -
#include <stdio.h>

typedef void Func();

void fn();
Func *const ptr = fn;

int main() {
  printf("%d\n", fn == ptr);
}
EOF

$CC -B. -o $t/exe -no-pie $t/b.o $t/a.so
$QEMU $t/exe | grep -q 1

echo OK
