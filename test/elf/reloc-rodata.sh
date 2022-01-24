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

cat <<EOF | $CC -fno-PIC -c -o $t/a.o -xc -
#include <stdio.h>

int foo;
int * const bar = &foo;

int main() {
  printf("%d\n", *bar);
}
EOF

! $CC -B. -o $t/exe $t/a.o -pie >& $t/log
grep -Eq 'relocation against symbol .+ can not be used; recompile with -fPIC' $t/log

echo OK
