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
#include <stdio.h>
int main() { printf("Hello world\n"); }
EOF

$CC -B. -o $t/exe1 $t/a.o
readelf -W --segments $t/exe1 > $t/log1
! grep -q '\.interp .* \.text' $t/log1 || false

$CC -B. -o $t/exe2 $t/a.o -Wl,--rosegment
readelf -W --segments $t/exe2 > $t/log2
! grep -q '\.interp .* \.text' $t/log2 || false

$CC -B. -o $t/exe3 $t/a.o -Wl,--no-rosegment
readelf -W --segments $t/exe3 > $t/log3
grep -q '\.interp .* \.text' $t/log3

echo OK
