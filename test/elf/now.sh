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

clang -fuse-ld="$mold" -shared -o "$t"/b.so "$t"/a.o -Wl,-z,now
readelf --dynamic "$t"/b.so | grep -q 'Flags: NOW'

clang -fuse-ld="$mold" -shared -o "$t"/b.so "$t"/a.o -Wl,-z,now,-z,lazy
readelf --dynamic "$t"/b.so > "$t"/log
! grep -q 'Flags: NOW' "$t"/log || false

echo OK
