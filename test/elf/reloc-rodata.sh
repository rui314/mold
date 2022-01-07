#!/bin/bash
export LANG=
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | $CC -fno-PIC -c -o "$t"/a.o -xc -
#include <stdio.h>

int foo;
int * const bar = &foo;

int main() {
  printf("%d\n", *bar);
}
EOF

! $CC -B. -o "$t"/exe "$t"/a.o -pie >& "$t"/log
grep -Pq 'relocation against symbol .+ can not be used; recompile with -fPIC' "$t"/log

echo OK
