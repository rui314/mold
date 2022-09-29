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

# ARM32's strip command crashes on the output of this test for some reason.
[[ $MACHINE = arm* ]] && { echo skipped; exit; }

strip=${TRIPLE}strip
command -v $strip >& /dev/null || { echo skipped; exit; }

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe $t/a.o -pie -Wl,-zmax-page-size=0x200000
$strip $t/exe
$QEMU $t/exe | grep -q 'Hello world'

echo OK
