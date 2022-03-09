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

[ "$(uname -m)" = x86_64 ] || { echo skipped; exit; }

cat <<EOF | gcc -ftls-model=local-dynamic -mtls-dialect=gnu -fPIC -c -o $t/a.o -xc - -mcmodel=large
#include <stdio.h>

extern _Thread_local int foo;
static _Thread_local int bar;

int *get_foo_addr() { return &foo; }
int *get_bar_addr() { return &bar; }

int main() {
  bar = 5;

  printf("%d %d %d %d\n", *get_foo_addr(), *get_bar_addr(), foo, bar);
  return 0;
}
EOF

cat <<EOF | gcc -ftls-model=local-dynamic -mtls-dialect=gnu  -fPIC -c -o $t/b.o -xc - -mcmodel=large
_Thread_local int foo = 3;
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o -mcmodel=large
$t/exe | grep -q '3 5 3 5'

$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,-no-relax -mcmodel=large
$t/exe | grep -q '3 5 3 5'

echo OK
