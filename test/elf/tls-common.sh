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

cat <<EOF | $GCC -o $t/a.o -c -xassembler -
.globl foo
.tls_common foo,4,4
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -std=c11 -
#include <stdio.h>

extern _Thread_local int foo;

int main() {
  printf("foo=%d\n", foo);
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep -q '^foo=0$'

echo OK
