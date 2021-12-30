#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/ld64.mold"
t="$(pwd)/out/test/macho/$testname"
mkdir -p "$t"

cat <<EOF | cc -o "$t"/a.o -c -xc -
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

clang -fuse-ld="$mold" -shared -o "$t"/b.dylib "$t"/a.o
otool -l "$t"/b.dylib > "$t"/log
! grep -q 'segname: __PAGEZERO' "$t"/log || false

echo OK
