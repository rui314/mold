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

cat <<EOF | $CC -o $t/libfoo.dylib -shared -xc -
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

clang --ld-path=./ld64 -o $t/exe $t/a.o -L$t -Wl,-lfoo
otool -l $t/exe | grep -A3 LOAD_DY | grep -q libfoo.dylib

clang --ld-path=./ld64 -o $t/exe $t/a.o -L$t -Wl,-lfoo -Wl,-dead_strip_dylibs
otool -l $t/exe | grep -A3 LOAD_DY > $t/log
! grep -q libfoo.dylib $t/log || false

echo OK
