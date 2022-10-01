#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/elf/$MACHINE/$testname
mkdir -p $t

[[ $MACHINE = x86_64 ]] || [[ $MACHINE = riscv* ]] || { echo skipped; exit; }

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
$QEMU $t/exe | grep -q '^3 0 5 0 0 0$'

echo OK
