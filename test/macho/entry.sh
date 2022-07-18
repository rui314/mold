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

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

int hello() {
  printf("Hello world\n");
  return 0;
}
EOF

clang --ld-path=./ld64 -o $t/exe $t/a.o -Wl,-e,_hello
$t/exe | grep -q 'Hello world'

! clang --ld-path=./ld64 -o $t/exe $t/a.o -Wl,-e,no_such_symbol 2> $t/log || false
grep -q 'undefined entry point symbol: no_such_symbol' $t/log

echo OK
