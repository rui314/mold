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

cat <<EOF | $CC -fcommon -xc -c -o $t/a.o -
int foo;
EOF

cat <<EOF | $CC -fcommon -xc -c -o $t/b.o -
int foo;

int main() {
  return 0;
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o \
  -Wl,-warn-common 2> /dev/null

! $CC -B. -o $t/exe $t/a.o $t/b.o \
  -Wl,-warn-common -Wl,-fatal-warnings 2> /dev/null || false

echo OK
