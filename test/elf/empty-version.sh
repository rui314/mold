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

cat <<EOF | $CC -fPIC -c -o "$t"/a.o -xc -
void foo1() {}
void foo2() {}

__asm__(".symver foo1, bar1@");
__asm__(".symver foo2, bar2@@");
EOF

$CC -B. -shared -o "$t"/b.so "$t"/a.o

readelf --dyn-syms "$t"/b.so | grep -q 'bar1$'
readelf --dyn-syms "$t"/b.so | grep -q 'bar2$'

echo OK
