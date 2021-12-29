#!/bin/bash
export LANG=
set -e
cd "$(dirname "$0")"
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh "$0") ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh "$0")
mkdir -p "$t"

cat <<EOF | clang -fcommon -xc -c -o "$t"/a.o -
int foo;
int bar;
int baz = 42;
EOF

cat <<EOF | clang -fcommon -xc -c -o "$t"/b.o -
#include <stdio.h>

int foo;
int bar = 5;
int baz;

int main() {
  printf("%d %d %d\n", foo, bar, baz);
}
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o "$t"/b.o
"$t"/exe | grep -q '0 5 42'

readelf --sections "$t"/exe > "$t"/log
grep -q '.common .*NOBITS' "$t"/log

echo OK
