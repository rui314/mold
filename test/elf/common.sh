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

cat <<EOF | $CC -fcommon -xc -c -o $t/a.o -
int foo;
int bar;
int baz = 42;
EOF

cat <<EOF | $CC -fcommon -xc -c -o $t/b.o -
#include <stdio.h>

int foo;
int bar = 5;
int baz;

int main() {
  printf("%d %d %d\n", foo, bar, baz);
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q '0 5 42'

readelf --sections $t/exe > $t/log
grep -q '.common .*NOBITS' $t/log

echo OK
