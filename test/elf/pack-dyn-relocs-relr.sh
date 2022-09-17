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

command -v llvm-readelf >& /dev/null || { echo skipped; exit; }

cat <<EOF | $CC -o $t/a.o -fPIC -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe1 $t/a.o -pie
llvm-readelf -r $t/exe1 | grep RELATIVE | wc -l > $t/log1

$CC -B. -o $t/exe2 $t/a.o -pie -Wl,-pack-dyn-relocs=relr
llvm-readelf -r $t/exe2 | grep RELATIVE | wc -l > $t/log2

diff $t/log1 $t/log2

llvm-readelf --dynamic $t/exe2 > $t/log3
grep -wq RELR $t/log3
grep -wq RELRSZ $t/log3
grep -wq RELRENT $t/log3

echo OK
