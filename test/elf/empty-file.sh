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

cat <<EOF | $CC -o "$t"/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

rm -f "$t"/b.script
touch "$t"/b.script

$CC -B. -o "$t"/exe "$t"/a.o -Wl,--version-script,"$t"/b.script
"$t"/exe | grep -q 'Hello world'

echo OK
