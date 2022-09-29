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

echo 'void _start() {}' | $CC -c -o $t/a.o -xc -
./mold -o $t/exe $t/a.o -gdb-index
readelf -WS $t/exe > $t/log
! grep -Fq .gdb_index $t/log || false

echo OK
