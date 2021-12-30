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
int main() {}
EOF

mkdir -p "$t"/foo/bar
rm -f "$t"/foo/bar/libfoo.a
ar rcs "$t"/foo/bar/libfoo.a "$t"/a.o

cat <<EOF > "$t"/b.script
INPUT(-lfoo)
EOF

clang -o "$t"/exe -L"$t"/foo/bar "$t"/b.script

echo OK
