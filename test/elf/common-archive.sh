#!/bin/bash
export LANG=
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | $CC -fcommon -xc -c -o "$t"/a.o -
#include <stdio.h>

int foo;
int bar;
__attribute__((weak)) int two();

int main() {
  printf("%d %d %d\n", foo, bar, two ? two() : -1);
}
EOF

cat <<EOF | $CC -fcommon -xc -c -o "$t"/b.o -
int foo = 5;
EOF

cat <<EOF | $CC -fcommon -xc -c -o "$t"/c.o -
int bar;
int two() { return 2; }
EOF

rm -f "$t"/d.a
ar rcs "$t"/d.a "$t"/b.o "$t"/c.o

$CC -B. -o "$t"/exe "$t"/a.o "$t"/d.a
"$t"/exe | grep -q '5 0 -1'

cat <<EOF | $CC -fcommon -xc -c -o "$t"/e.o -
int bar = 0;
int two() { return 2; }
EOF

rm -f "$t"/e.a
ar rcs "$t"/e.a "$t"/b.o "$t"/e.o

$CC -B. -o "$t"/exe "$t"/a.o "$t"/e.a
"$t"/exe | grep -q '5 0 2'

echo OK
