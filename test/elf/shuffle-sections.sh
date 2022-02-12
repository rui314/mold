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

$CC -B. -o $t/exe1 $t/a.o
$t/exe1 | grep -q 'Hello world'

$CC -B. -o $t/exe2 $t/a.o -Wl,-shuffle-sections
$t/exe2 | grep -q 'Hello world'

$CC -B. -o $t/exe3 $t/a.o -Wl,-shuffle-sections=100
$t/exe3 | grep -q 'Hello world'

$CC -B. -o $t/exe4 $t/a.o -Wl,-shuffle-sections=100
$t/exe4 | grep -q 'Hello world'

diff $t/exe3 $t/exe4

$CC -B. -o $t/exe5 $t/a.o -Wl,-shuffle-sections=101
$t/exe5 | grep -q 'Hello world'

! diff $t/exe3 $t/exe4 >& /dev/null || false

echo OK
