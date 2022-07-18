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

cat <<EOF | $CC -c -o $t/a.o -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF

clang --ld-path=./ld64 -B. -o $t/exe1 $t/a.o -Wl,-adhoc_codesign
clang --ld-path=./ld64 -B. -o $t/exe2 $t/a.o -Wl,-adhoc_codesign

[ "$(otool -l $t/exe1 | grep 'uuid ')" != "$(otool -l $t/exe2 | grep 'uuid ')" ]

echo OK
