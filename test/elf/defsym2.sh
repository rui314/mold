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

cat <<EOF | $CC -fPIC -o $t/a.o -c -xc -
void foo() {}
EOF

$CC -B. -o $t/b.so -shared -Wl,-defsym=bar=foo $t/a.o
nm -D $t/b.so | grep -q 'bar' || false

echo OK
