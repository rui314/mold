#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | cc -o "$t"/a.o -c -xc -fPIE -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

clang -fuse-ld="$mold" -pie -o "$t"/exe "$t"/a.o
readelf --file-header "$t"/exe | grep -q -E '(Shared object file|Position-Independent Executable file)'
"$t"/exe | grep -q 'Hello world'

echo OK
