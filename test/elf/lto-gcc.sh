#!/bin/bash
export LC_ALL=C
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t=out/test/elf/$testname
mkdir -p $t

which gcc >& /dev/null || { echo skipped; exit 0; }

cat <<EOF | gcc -flto -c -o $t/a.o -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

gcc -B. -o $t/exe -flto $t/a.o
$t/exe | grep -q 'Hello world'

echo OK
