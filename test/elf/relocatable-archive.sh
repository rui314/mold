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

cat <<EOF | $CC -c -o $t/a.o -xc -
void bar();
void foo() {
  bar();
}
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
void bar() {}
EOF

cat <<EOF | $CC -c -o $t/c.o -xc -
void baz() {}
EOF

cat <<EOF | $CC -c -o $t/d.o -xc -
void foo();
int main() {
  foo();
}
EOF

ar crs $t/e.a $t/a.o $t/b.o $t/c.o
./mold -r -o $t/f.o $t/d.o $t/e.a

readelf --symbols $t/f.o > $t/log
grep -q 'foo\b' $t/log
grep -q 'bar\b' $t/log
! grep -q 'baz\b' $t/log || false

echo OK
