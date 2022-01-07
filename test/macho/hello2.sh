#!/bin/bash
export LANG=
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/ld64.mold"
t="$(pwd)/out/test/macho/$testname"
mkdir -p "$t"

cat <<EOF | $CC -o "$t"/a.o -c -xc -
#include <stdio.h>

int main() {
  printf("Hello");
  puts(" world");
}
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o
"$t"/exe | grep -q 'Hello world'

echo OK
