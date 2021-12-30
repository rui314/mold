#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | cc -o "$t"/a.o -c -xc -
void foobar() {}
EOF

rm -f "$t"/b.a
ar rcs "$t"/b.a "$t"/a.o

cat <<EOF | cc -o "$t"/c.o -c -xc -
int main() {}
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/c.o "$t"/b.a
! readelf --symbols "$t"/exe | grep -q foobar || false

clang -fuse-ld="$mold" -o "$t"/exe "$t"/c.o "$t"/b.a -Wl,-require-defined,foobar
readelf --symbols "$t"/exe | grep -q foobar

! clang -fuse-ld="$mold" -o "$t"/exe "$t"/c.o "$t"/b.a -Wl,-require-defined,xyz >& "$t"/log
grep -q 'undefined symbol: xyz' "$t"/log

echo OK
