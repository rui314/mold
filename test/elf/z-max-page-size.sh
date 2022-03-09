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

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,-z,max-page-size=65536
$t/exe | grep -q 'Hello world'
readelf -W --segments $t/exe | grep -q 'LOAD.*R   0x10000$'

$CC -B. -o $t/exe $t/a.o -Wl,-zmax-page-size=$((1024*1024))
$t/exe | grep -q 'Hello world'
readelf -W --segments $t/exe | grep -q 'LOAD.*R   0x100000$'

echo OK
