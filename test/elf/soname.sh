#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | clang -fPIC -c -o "$t"/a.o -xc -
void foo() {}
EOF

clang -o "$t"/b.so -shared "$t"/a.o
readelf --dynamic "$t"/b.so > "$t"/log
! fgrep -q 'Library soname' "$t"/log || false

clang -fuse-ld="$mold" -o "$t"/b.so -shared "$t"/a.o -Wl,-soname,foo
readelf --dynamic "$t"/b.so | fgrep -q 'Library soname: [foo]'

echo OK
