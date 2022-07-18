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
void hello() {}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
void hello() {}
int main() {}
EOF

! clang --ld-path=./ld64 -o $t/exe $t/a.o $t/b.o 2> $t/log || false
grep -q 'duplicate symbol: .*/b.o: .*/a.o: _hello' $t/log

echo OK
