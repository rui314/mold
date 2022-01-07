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

cat <<EOF | $CC -c -o "$t"/a.o -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

$CC -B. -o "$t"/exe "$t"/a.o
! readelf --sections "$t"/exe | fgrep -q .repro || false


$CC -B. -o "$t"/exe "$t"/a.o -Wl,-repro
objcopy --dump-section .repro="$t"/repro.tar.gz "$t"/exe

tar -C "$t" -xzf "$t"/repro.tar.gz
fgrep -q /a.o  "$t"/repro/response.txt
fgrep -q mold "$t"/repro/version.txt


MOLD_REPRO=1 $CC -B. -o "$t"/exe "$t"/a.o
objcopy --dump-section .repro="$t"/repro.tar.gz "$t"/exe

tar -C "$t" -xzf "$t"/repro.tar.gz
fgrep -q /a.o  "$t"/repro/response.txt
fgrep -q mold "$t"/repro/version.txt

echo OK
