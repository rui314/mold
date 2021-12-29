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

int hello() {
  printf("Hello world\n");
  return 0;
}
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o -Wl,-e,_hello
"$t"/exe | grep -q 'Hello world'

! clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o -Wl,-e,no_such_symbol 2> "$t"/log || false
grep -q 'undefined entry point symbol: no_such_symbol' "$t"/log

echo OK
