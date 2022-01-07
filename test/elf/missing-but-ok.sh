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

cat <<EOF | $CC -o "$t"/a.o -c -x assembler -
  .text
  .globl _start, foo
_start:
  nop
EOF

"$mold" -o "$t"/exe "$t"/a.o

echo OK
