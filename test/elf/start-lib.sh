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

cat <<EOF | $CC -o $t/a.o -c -xc -
int foo() { return 3; }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
int bar() { return 3; }
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
int main() {}
EOF

$CC -B. -o $t/exe -Wl,-start-lib $t/a.o -Wl,-end-lib $t/b.o $t/c.o
nm $t/exe > $t/log
! grep -q ' foo$' $t/log || false
grep -q ' bar$' $t/log

echo OK
