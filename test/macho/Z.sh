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
int main() {}
EOF

! clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o -Wl,-Z > "$t"/log 2>&1
grep -q 'library not found: -lSystem' "$t"/log

echo OK
