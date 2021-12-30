#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | clang -c -o "$t"/a.o -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o -Wl,-z,origin

readelf --dynamic "$t"/exe | grep -Pq '\(FLAGS\)\s+ORIGIN'
readelf --dynamic "$t"/exe | grep -Pq 'Flags: ORIGIN'

echo OK
