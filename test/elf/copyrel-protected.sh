#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | cc -o "$t"/a.o -c -xc -fno-PIE -
extern int foo;

int main() {
  return foo;
}
EOF

cat <<EOF | cc -shared -o "$t"/b.so -xc -
__attribute__((visibility("protected"))) int foo;
EOF

! clang -fuse-ld="$mold" "$t"/a.o "$t"/b.so -o "$t"/exe >& "$t"/log || false
fgrep -q 'cannot make copy relocation for protected symbol' "$t"/log

echo OK
