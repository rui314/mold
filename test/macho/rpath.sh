#!/bin/bash
export LANG=
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/ld64.mold"
t="$(pwd)/out/test/macho/$testname"
mkdir -p "$t"

cat <<EOF | $CC -o "$t"/a.o -c -xc -
int main() {}
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o -Wl,-rpath,foo -Wl,-rpath,@bar
otool -l "$t"/exe > "$t"/log

grep -A3 'cmd LC_RPATH' "$t"/log | grep -q 'path foo'
grep -A3 'cmd LC_RPATH' "$t"/log | grep -q 'path @bar'

echo OK
