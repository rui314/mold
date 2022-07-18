#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$MACHINE/$testname
mkdir -p $t

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

clang --ld-path=./ld64 -o $t/exe1 $t/a.o
otool -l $t/exe1 | grep -q 'stacksize 0$'

clang --ld-path=./ld64 -o $t/exe2 $t/a.o -Wl,-stack_size,200000
otool -l $t/exe2 | grep -q 'stacksize 2097152$'

echo OK
