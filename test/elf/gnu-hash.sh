#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | cc -c -o "$t"/a.o -xc -
void foo() {}
void bar() {}
static void baz() {}
EOF

clang -fuse-ld="$mold" -o "$t"/b.so "$t"/a.o -Wl,-hash-style=gnu -shared

echo OK
