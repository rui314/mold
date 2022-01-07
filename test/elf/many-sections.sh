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

seq 1 65500 | sed 's/.*/.section .text.\0, "ax",@progbits/' > "$t"/a.s

$CC -c -o "$t"/a.o "$t"/a.s

cat <<'EOF' | $CC -c -xc -o "$t"/b.o -
#include <stdio.h>

int main() {
  printf("Hello\n");
  return 0;
}
EOF

$CC -B. -o "$t"/exe "$t"/a.o "$t"/b.o
"$t"/exe | grep -q Hello

echo OK
