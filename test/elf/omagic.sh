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

echo 'int main() {}' | cc -o /dev/null -xc - -static >& /dev/null || \
  { echo skipped; exit; }

cat <<EOF | $CC -c -o $t/a.o -xc - -fno-PIC
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

$CC -B. $t/a.o -o $t/exe -static -Wl,--omagic
readelf -W --segments $t/exe | grep -qw RWE

echo OK
