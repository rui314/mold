#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | clang -c -fPIC -o "$t"/a.o -xc -
#include <stdio.h>

void foo() {
  printf("Hello world\n");
}
EOF

clang -fuse-ld="$mold" -shared -o "$t"/b.so "$t"/a.o -Wl,-z,interpose
readelf --dynamic "$t"/b.so | grep -q 'Flags: INTERPOSE'

echo OK
