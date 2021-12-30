#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | cc -shared -o "$t"/a.so -xc -
void fn() {}
EOF

cat <<EOF | cc -o "$t"/b.o -c -xc -fno-PIC -
#include <stdio.h>

typedef void Func();

void fn();
Func *const ptr = fn;

int main() {
  printf("%d\n", fn == ptr);
}
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/b.o "$t"/a.so
"$t"/exe | grep -q 1

echo OK
