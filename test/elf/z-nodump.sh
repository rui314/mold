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

cat <<EOF | $CC -c -o "$t"/a.o -xc -
void foo() {}
EOF

$CC -B. -shared -o "$t"/b.so "$t"/a.o
! readelf --dynamic "$t"/b.so | grep -Pq 'Flags: NODUMP' || false

$CC -B. -shared -o "$t"/b.so "$t"/a.o -Wl,-z,nodump
readelf --dynamic "$t"/b.so | grep -Pq 'Flags: NODUMP'

echo OK
