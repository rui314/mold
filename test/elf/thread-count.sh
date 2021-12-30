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
int main() {
  printf("Hello world\n");
}
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o -Wl,-no-threads
clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o -Wl,-thread-count=1
clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o -Wl,-threads
clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o -Wl,-threads=1
clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o -Wl,--threads=1

echo OK
