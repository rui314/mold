#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | cc -fno-PIC -c -o "$t"/a.o -xc -
#include <stdio.h>

int foo;
int * const bar = &foo;

int main() {
  printf("%d\n", *bar);
}
EOF

! clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o -pie >& "$t"/log
grep -Pq 'relocation against symbol .+ can not be used; recompile with -fPIC' "$t"/log

echo OK
