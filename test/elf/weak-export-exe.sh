#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | cc -fPIC -c -o "$t"/a.o -xc -
#include <stdio.h>

__attribute__((weak)) int foo();

int main() {
  printf("%d\n", foo ? foo() : 3);
}
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o
! readelf --dyn-syms "$t"/exe | grep -q 'WEAK   DEFAULT  UND foo' || false
"$t"/exe | grep -q '^3$'

echo OK
