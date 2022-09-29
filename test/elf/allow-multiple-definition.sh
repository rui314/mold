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

echo 'int main() { return 0; }' | $CC -c -o $t/a.o -xc -
echo 'int main() { return 1; }' | $CC -c -o $t/b.o -xc -

! $CC -B. -o $t/exe $t/a.o $t/b.o 2> /dev/null || false
$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,-allow-multiple-definition
$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,-z,muldefs

echo OK
