#!/bin/bash
export LANG=
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

$CC -B. -o $t/exe1 $t/a.o -Wl,-z,separate-loadable-segments
$t/exe1 | grep -q 'Hello world'

$CC -B. -o $t/exe2 $t/a.o -Wl,-z,separate-code -Wl,-z,norelro
$t/exe2 | grep -q 'Hello world'

$CC -B. -o $t/exe3 $t/a.o -Wl,-z,noseparate-code -Wl,-z,norelro
$t/exe3 | grep -q 'Hello world'

readelf --segments $t/exe3 > $t/log
! grep 'LOAD .* RW ' $t/log || false

echo OK
