#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/ld64.mold"
t="$(pwd)/out/test/macho/$testname"
mkdir -p "$t"

cat <<EOF | cc -o "$t"/a.o -c -xc -
void hello() {}
EOF

cat <<EOF | cc -o "$t"/b.o -c -xc -
void hello() {}
int main() {}
EOF

! clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o "$t"/b.o 2> "$t"/log || false
grep -q 'duplicate symbol: .*/b.o: .*/a.o: _hello' "$t"/log

echo OK
