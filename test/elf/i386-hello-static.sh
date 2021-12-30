#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

echo 'int main() {}' | cc -m32 -o "$t"/exe -xc - >& /dev/null \
  || { echo skipped; exit; }

cat <<EOF | cc -m32 -o "$t"/a.o -c -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

clang -m32 -o "$t"/exe "$t"/a.o -static
"$t"/exe | grep -q 'Hello world'

echo OK
