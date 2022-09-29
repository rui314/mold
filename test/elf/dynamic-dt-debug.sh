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
int main() {}
EOF

$CC -B. -o $t/exe $t/a.o
readelf --dynamic $t/exe > $t/log
grep -Fq '(DEBUG)' $t/log

cat <<EOF | $CC -o $t/b.o -c -xc -
void foo() {}
EOF

$CC -B. -o $t/c.so $t/b.o -shared
readelf --dynamic $t/c.so > $t/log
! grep -Fq '(DEBUG)' $t/log || false

echo OK
