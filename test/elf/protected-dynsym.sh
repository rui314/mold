#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | cc -fPIC -c -o "$t"/a.o -xc -
extern int foo;
EOF

cat <<EOF | cc -fPIC -c -o "$t"/b.o -xc -
__attribute__((visibility("protected"))) int foo;
EOF

clang -fuse-ld="$mold" -shared -o "$t"/c.so "$t"/a.o "$t"/b.o -Wl,-strip-all
readelf --symbols "$t"/c.so | grep -Pq 'PROTECTED\b.*\bfoo\b'

echo OK
