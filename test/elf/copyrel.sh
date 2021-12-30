#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | cc -fno-PIC -o "$t"/a.o -c -xc -
#include <stdio.h>

extern int foo;
extern int bar;

int main() {
  printf("%d %d %d\n", foo, bar, &foo == &bar);
  return 0;
}
EOF

cat <<EOF | cc -o "$t"/b.o -c -x assembler -
  .globl foo, bar
  .data;
foo:
bar:
  .long 42
EOF

clang -fuse-ld="$mold" -no-pie -o "$t"/exe "$t"/a.o "$t"/b.o
"$t"/exe | grep -q '42 42 1'

echo OK
