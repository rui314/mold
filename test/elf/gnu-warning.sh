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

cat <<EOF | $GCC -c -o $t/a.o -xc -
#include <stdio.h>

__attribute__((section(".gnu.warning.foo")))
static const char foo[] = "foo is deprecated";

__attribute__((section(".gnu.warning.bar")))
const char bar[] = "bar is deprecated";

int main() {
  printf("Hello world\n");
}
EOF

# Make sure that we do not copy .gnu.warning.* sections.
$CC -B. -o $t/exe $t/a.o
$QEMU $t/exe | grep -q 'Hello world'

echo OK
