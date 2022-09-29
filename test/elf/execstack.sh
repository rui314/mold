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

cat <<EOF | $CC -c -xc -o $t/a.o -
int main() {}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,-z,execstack
readelf --segments -W $t/exe | grep -q 'GNU_STACK.* RWE '

$CC -B. -o $t/exe $t/a.o -Wl,-z,execstack -Wl,-z,noexecstack
readelf --segments -W $t/exe | grep -q 'GNU_STACK.* RW '

$CC -B. -o $t/exe $t/a.o
readelf --segments -W $t/exe | grep -q 'GNU_STACK.* RW '

echo OK
