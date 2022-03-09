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

cat <<EOF | $CC -o $t/a.o -c -flto -xc -
#include <stdio.h>
void hello() {
  printf("Hello world\n");
}
EOF

cat <<EOF | $CC -o $t/b.o -c -flto -xc -
#include <stdio.h>
void howdy() {
  printf("Hello world\n");
}
EOF

rm -f $t/c.a
ar rc $t/c.a $t/a.o $t/b.o

cat <<EOF | $CC -o $t/d.o -c -flto -xc -
void hello();
int main() {
  hello();
}
EOF

$CC -B. -o $t/exe -flto $t/d.o $t/c.a
$t/exe | grep -q 'Hello world'

nm $t/exe > $t/log
grep -q hello $t/log
! grep -q howdy $t/log || false

echo OK
