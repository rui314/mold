#!/bin/bash
export LANG=
set -e
cd "$(dirname "$0")"
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh "$0") ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh "$0")
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
