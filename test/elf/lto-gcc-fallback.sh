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

which ld.bfd >& /dev/null || { echo skipped; exit 0; }

cat <<EOF | gcc -flto -c -o $t/a.o -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

gcc -B"$(pwd)" -o $t/exe $t/a.o >& $t/log
grep -q 'falling back' $t/log
$t/exe | grep -q 'Hello world'

echo OK
