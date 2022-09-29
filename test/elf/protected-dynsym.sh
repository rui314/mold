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

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
extern int foo;
EOF

cat <<EOF | $CC -fPIC -c -o $t/b.o -fcommon -xc -
__attribute__((visibility("protected"))) int foo;
EOF

$CC -B. -shared -o $t/c.so $t/a.o $t/b.o -Wl,-strip-all
readelf --symbols $t/c.so | grep -Eq 'PROTECTED\b.*\bfoo\b'

cat <<EOF | $CC -fPIC -c -o $t/d.o -fno-common -xc -
__attribute__((visibility("protected"))) int foo;
EOF

$CC -B. -shared -o $t/e.so $t/a.o $t/d.o -Wl,-strip-all
readelf --symbols $t/e.so | grep -Eq 'PROTECTED\b.*\bfoo\b'

echo OK
