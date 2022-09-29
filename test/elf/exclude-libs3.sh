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

cat <<EOF | $CC -fPIC -xc -c -o $t/a.o -
void foo();
void bar() { foo(); }
EOF

rm -f $t/b.a
ar crs $t/b.a $t/a.o

cat <<EOF | $CC -fPIC -xc -c -o $t/c.o -
void bar();
void foo() { bar(); }
EOF

$CC -B. -shared -o $t/d.so $t/c.o $t/b.a -Wl,-exclude-libs=ALL
readelf --dyn-syms $t/d.so > $t/log
grep -Fq foo $t/log

echo OK
