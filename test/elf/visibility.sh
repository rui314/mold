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
t=out/test/elf/$MACHINE/$testname
mkdir -p $t

cat <<EOF | $CC -xc -c -o $t/a.o -
__attribute__((visibility("hidden"))) int foo = 3;
EOF

cat <<EOF | $CC -xc -c -o $t/b.o -
__attribute__((visibility("hidden"))) int foo = 3;
int bar = 5;
EOF

rm -f $t/c.a
ar crs $t/c.a $t/a.o $t/b.o

cat <<EOF | $CC -xc -fPIC -c -o $t/d.o -
extern int bar;
int main() { return bar; }
EOF

$CC -B. -shared -o $t/e.so $t/c.a $t/d.o
readelf --dyn-syms $t/e.so > $t/log
! grep -Fq foo $t/log || false

echo OK
