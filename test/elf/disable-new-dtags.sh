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

cat <<EOF | $CC -o $t/a.o -c -xc -fPIC -
void foo() {}
EOF

$CC -B. -shared -o $t/b.so $t/a.o -Wl,-rpath=/foo
readelf --dynamic $t/b.so | grep -q 'RUNPATH.*/foo'

$CC -B. -shared -o $t/b.so $t/a.o -Wl,-rpath=/foo -Wl,-enable-new-dtags
readelf --dynamic $t/b.so | grep -q 'RUNPATH.*/foo'

$CC -B. -shared -o $t/b.so $t/a.o -Wl,-rpath=/foo -Wl,-disable-new-dtags
readelf --dynamic $t/b.so | grep -q 'RPATH.*/foo'

echo OK
