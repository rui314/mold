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
  .globl main
main:
  nop
EOF

! ./mold -o $t/exe $t/a.o $t/a.o 2> $t/log || false
grep -q 'duplicate symbol: .*\.o: .*\.o: main' $t/log

echo OK
