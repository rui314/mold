#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | clang -c -xc -o "$t"/a.o -
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF

clang -fuse-ld="$mold" -Wl,--verbose -o "$t"/exe "$t"/a.o > /dev/null

echo OK
