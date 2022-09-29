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

cat <<EOF | $CC -c -o $t/a.o -xc -
#include <stdio.h>

void foo();

void bar() {
  foo();
  printf("Hello world\n");
}
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
void foo() {}
void bar();
void baz();

int main() {
  bar();
  baz();
  return 0;
}
EOF

cat <<EOF | $CC -shared -o $t/c.so -xc -
void baz() {}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o $t/c.so \
  -Wl,-y,foo -Wl,--trace-symbol=baz > $t/log

grep -q 'trace-symbol: .*/a.o: reference to foo' $t/log
grep -q 'trace-symbol: .*/b.o: definition of foo' $t/log
grep -q 'trace-symbol: .*/c.so: definition of baz' $t/log

echo OK
