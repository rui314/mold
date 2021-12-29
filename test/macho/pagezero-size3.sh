#!/bin/bash
export LANG=
set -e
cd "$(dirname "$0")"
mold=`pwd`/../../ld64.mold
echo -n "Testing $(basename -s .sh "$0") ... "
t=$(pwd)/../../out/test/macho/$(basename -s .sh "$0")
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
