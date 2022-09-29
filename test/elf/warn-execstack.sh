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

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section .note.GNU-stack, "x"
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
int main() {}
EOF

$GCC -B. -o $t/exe $t/a.o $t/b.o 2>&1 | grep -q 'may cause a segmentation fault'

echo OK
