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

cat <<EOF | $CC -o $t/a.o -c -xc - -fno-PIC
int main() {}
EOF

$CC -B. -shared -o $t/b.so $t/a.o

! $CC -B. -o $t/b.so $t/a.o -Wl,-require-defined=no-such-sym >& $t/log1 || false
grep -q 'undefined symbol: no-such-sym' $t/log1

$CC -B. -shared -o $t/b.o $t/a.o -Wl,-require-defined=no-such-sym -Wl,-noinhibit-exec >& $t/log2
grep -q 'undefined symbol: no-such-sym' $t/log2

echo OK
