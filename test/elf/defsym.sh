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

cat <<EOF | $CC -fPIC -o $t/a.o -c -xc -
#include <stdio.h>
extern char foo;
extern char bar;
void baz();

void print() {
  printf("Hello %p %p\n", &foo, &bar);
}

int main() {
  baz();
}
EOF

$CC -B. -o $t/exe $t/a.o -pie -Wl,-defsym=foo=16 \
  -Wl,-defsym=bar=0x2000 -Wl,-defsym=baz=print

$QEMU $t/exe | grep -q '^Hello 0x10 0x2000$'

echo OK
