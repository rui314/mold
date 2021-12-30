#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

"$mold" -v | grep -q 'mold .*compatible with GNU ld and GNU gold'
"$mold" --version | grep -q 'mold .*compatible with GNU ld and GNU gold'

"$mold" -V | grep -q 'mold .*compatible with GNU ld and GNU gold'
"$mold" -V | grep -q elf_x86_64
"$mold" -V | grep -q elf_i386

cat <<EOF | clang -c -xc -o "$t"/a.o -
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF

rm -f "$t"/exe
clang -fuse-ld="$mold" -Wl,--version -o "$t"/exe "$t"/a.o | grep -q mold
! [ -f "$t"/exe ] || false

clang -fuse-ld="$mold" -Wl,-v -o "$t"/exe "$t"/a.o | grep -q mold
"$t"/exe | grep -q 'Hello world'

echo OK
