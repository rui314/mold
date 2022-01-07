#!/bin/bash
export LANG=
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t=out/test/elf/$testname
mkdir -p $t

echo 'int main() {}' | $CC -m32 -o $t/exe -xc - >& /dev/null \
  || { echo skipped; exit; }

cat <<EOF | $CC -m32 -o $t/a.o -c -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

$CC -B. -m32 -o $t/exe $t/a.o
$t/exe | grep -q 'Hello world'

echo OK
