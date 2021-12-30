#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | clang -fPIC -c -o "$t"/a.o -xc -
int foo() __attribute__((visibility("protected")));
int bar() __attribute__((visibility("protected")));
void *baz() __attribute__((visibility("protected")));

int foo() {
  return 4;
}

int bar() {
  return foo();
}

void *baz() {
  return baz;
}
EOF

clang -fuse-ld="$mold" -o "$t"/b.so -shared "$t"/a.o

cat <<EOF | cc -c -o "$t"/c.o -xc - -fno-PIE
#include <stdio.h>

int foo() {
  return 3;
}

int bar();
void *baz();

int main() {
  printf("%d %d %d\n", foo(), bar(), baz == baz());
}
EOF

clang -fuse-ld="$mold" -no-pie -o "$t"/exe "$t"/c.o "$t"/b.so
"$t"/exe | grep -q '3 4 0'

echo OK
