#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | cc -o "$t"/a.o -c -xc -
#include <stdio.h>

__attribute__((aligned(512)))
char hello[] = "Hello";

__attribute__((aligned(512)))
char world[] = "world";

int main() {
  printf("%s %s\n", hello, world);
}
EOF

clang -fuse-ld="$mold" -static -Wl,--filler,0xfe -o "$t"/exe1 "$t"/a.o
sed -i -e 's/--filler 0xfe/--filler 0x00/' "$t"/exe1
hexdump -C "$t"/exe1 > "$t"/txt1

clang -fuse-ld="$mold" -static -Wl,--filler,0x00 -o "$t"/exe2 "$t"/a.o
hexdump -C "$t"/exe2 > "$t"/txt2

diff -q "$t"/txt1 "$t"/txt2

echo OK
