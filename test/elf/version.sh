#!/bin/bash
export LANG=
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
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

cat <<EOF | $CC -c -xc -o "$t"/a.o -
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF

rm -f "$t"/exe
$CC -B. -Wl,--version -o "$t"/exe "$t"/a.o 2>&1 | grep -q mold
! [ -f "$t"/exe ] || false

$CC -B. -Wl,-v -o "$t"/exe "$t"/a.o 2>&1 | grep -q mold
"$t"/exe | grep -q 'Hello world'

echo OK
