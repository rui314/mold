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

cat <<EOF | $CC -o $t/a.o -c -x assembler -
  .text
  .globl _start
_start:
  nop
EOF

./mold -o $t/b.so $t/a.o -auxiliary foo -f bar -shared

readelf --dynamic $t/b.so > $t/log
grep -Fq 'Auxiliary library: [foo]' $t/log
grep -Fq 'Auxiliary library: [bar]' $t/log

echo OK
