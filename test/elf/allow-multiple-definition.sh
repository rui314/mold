#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

echo 'int main() { return 0; }' > "$t"/a.c
echo 'int main() { return 0; }' > "$t"/b.c

! clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.c "$t"/b.c 2> /dev/null || false
clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.c "$t"/b.c -Wl,-allow-multiple-definition
clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.c "$t"/b.c -Wl,-z,muldefs

echo OK
