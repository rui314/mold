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

cat <<EOF | $CC -c -o $t/a.o -x assembler -
.globl _start
_start:
EOF

./mold -o $t/exe $t/a.o

readelf --sections $t/exe > $t/log
! grep -Fq .interp $t/log || false

readelf --dynamic $t/exe > $t/log

./mold -o $t/exe $t/a.o --dynamic-linker=/foo/bar

readelf --sections $t/exe > $t/log
grep -Fq .interp $t/log

echo OK
