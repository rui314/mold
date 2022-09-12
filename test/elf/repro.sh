#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/elf/$MACHINE/$testname
mkdir -p $t

cat <<EOF | $CC -c -o $t/a.o -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

rm -rf $t/exe.repro $t/exe.repro.tar

$CC -B. -o $t/exe $t/a.o
! [ -f $t/exe.repro.tar ] || false

$CC -B. -o $t/exe $t/a.o -Wl,-repro

tar -C $t -xf $t/exe.repro.tar
grep -Fq /a.o  $t/exe.repro/response.txt
grep -Fq mold $t/exe.repro/version.txt

rm -rf $t/exe.repro $t/exe.repro.tar

MOLD_REPRO=1 $CC -B. -o $t/exe $t/a.o
tar -C $t -xf $t/exe.repro.tar
grep -Fq /a.o  $t/exe.repro/response.txt
grep -Fq mold $t/exe.repro/version.txt

echo OK
