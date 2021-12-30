#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

mkdir -p "$t"/foo

cat <<EOF | cc -o "$t"/foo/a.o -c -xc -
int main() {}
EOF

cat <<EOF > "$t"/b.script
INPUT(a.o)
EOF

clang -o "$t"/exe -L"$t"/foo "$t"/b.script

echo OK
