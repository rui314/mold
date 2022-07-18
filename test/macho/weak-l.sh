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
#include <stdio.h>
void hello() __attribute__((weak_import));

int main() {
  if (hello)
    hello();
  else
    printf("hello is missing\n");
}
EOF

clang --ld-path=./ld64 -o $t/exe $t/a.o -L$t -Wl,-weak-lfoo
$t/exe | grep -q 'Hello world'

rm $t/libfoo.dylib
$t/exe | grep -q 'hello is missing'

echo OK
