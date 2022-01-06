#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

echo 'int main() {}' | cc -c -o "$t"/a.o -xc -

echo 'VER1 { foo[12; };' > "$t"/b.ver

! clang -fuse-ld="$mold" -shared -o "$t"/c.so -Wl,-version-script,"$t"/b.ver \
  "$t"/a.o >& "$t"/log || false
grep -q 'invalid version pattern' "$t"/log

echo OK
