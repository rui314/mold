#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | clang -flto -c -o "$t"/a.o -xc -
int main() {}
EOF

! clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o &> "$t"/log
grep -q '.*/a.o: .*mold does not support LTO' "$t"/log

echo OK
