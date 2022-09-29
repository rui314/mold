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

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,-z,max-page-size=65536 -Wl,-z,separate-loadable-segments
$QEMU $t/exe | grep -q 'Hello world'
readelf -W --segments $t/exe | grep -q 'LOAD.*R   0x10000$'

$CC -B. -o $t/exe $t/a.o -Wl,-zmax-page-size=$((1024*1024)) -Wl,-z,separate-loadable-segments
$QEMU $t/exe | grep -q 'Hello world'
readelf -W --segments $t/exe | grep -q 'LOAD.*R   0x100000$'

echo OK
