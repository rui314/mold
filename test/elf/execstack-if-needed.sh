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

[ $MACHINE = x86_64 ] || { echo skipped; exit; }

cat <<EOF | $CC -c -xassembler -o $t/a.o -
.globl main
main:
  ret
.section .note.GNU-stack, "x", @progbits
EOF

$CC -B. -o $t/exe $t/a.o >& /dev/null
readelf --segments -W $t/exe | grep -q 'GNU_STACK.* RW '

$CC -B. -o $t/exe $t/a.o -Wl,-z,execstack-if-needed
readelf --segments -W $t/exe | grep -q 'GNU_STACK.* RWE '

echo OK
