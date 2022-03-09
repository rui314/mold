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

cat <<EOF | $CC -o $t/a.so -fPIC -shared -xc -
void *foo() {
  return foo;
}

void *bar() {
  return bar;
}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -fPIC
void *bar();

void *baz() {
  return bar;
}
EOF

cat <<EOF | $CC -o $t/c.o -c -xc - -fno-PIC
#include <stdio.h>

void *foo();
void *bar();
void *baz();

int main() {
  printf("%d %d %d\n", foo == foo(), bar == bar(), bar == baz());
}
EOF

$CC -B. -no-pie -o $t/exe $t/a.so $t/b.o $t/c.o
$t/exe | grep -q '^1 1 1$'

echo OK
