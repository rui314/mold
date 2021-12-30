#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | clang -c -xc -o "$t"/a.o -
int main() {}
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o
readelf --segments -W "$t"/exe > "$t"/log
grep -q 'GNU_RELRO ' "$t"/log

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o -Wl,-z,norelro
readelf --segments -W "$t"/exe > "$t"/log
! grep -q 'GNU_RELRO ' "$t"/log || false

echo OK
