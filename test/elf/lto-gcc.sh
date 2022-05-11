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
cd "$(dirname "$0")"/../..
t=out/test/elf/$testname
mkdir -p $t

which $GCC >& /dev/null || { echo skipped; exit; }

cat <<EOF | $GCC -flto -c -o $t/a.o -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$GCC -B. -o $t/exe -flto $t/a.o
$QEMU $t/exe | grep -q 'Hello world'

echo OK
