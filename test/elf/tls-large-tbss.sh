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

cat <<EOF | $CC -c -o $t/a.o -x assembler -
.globl x, y
.section .tbss,"awT",@nobits
x:
.zero 1024
.section .tcommon,"awT",@nobits
y:
.zero 1024
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
#include <stdio.h>

extern _Thread_local char x[1024000];
extern _Thread_local char y[1024000];

int main() {
  x[0] = 3;
  x[1023] = 5;
  printf("%d %d %d %d %d %d\n", x[0], x[1], x[1023], y[0], y[1], y[1023]);
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q '^3 0 5 0 0 0$'

echo OK
