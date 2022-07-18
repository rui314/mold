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
t=out/test/macho/$MACHINE/$testname
mkdir -p $t

cat <<EOF | $CC -o $t/a.o -c -xc - -flto
#include <stdio.h>

void hello() {
  printf("Hello world\n");
}

int main() {
  hello();
}
EOF

clang --ld-path=./ld64 -o $t/exe1 $t/a.o -flto
$t/exe1 | grep -q 'Hello world'
nm -dyldinfo-only $t/exe1 > $t/log1
! grep -q _hello $t/log1 || false

clang --ld-path=./ld64 -o $t/exe2 $t/a.o -flto -Wl,-export_dynamic
$t/exe2 | grep -q 'Hello world'
nm -dyldinfo-only $t/exe2 > $t/log2
grep -q _hello $t/log2

echo OK
