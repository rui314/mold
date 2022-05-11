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

[ $MACHINE = x86_64 ] || { echo skipped; exit; }

cat <<EOF | $CC -o $t/a.o -c -fPIC -xc -
#include <stdio.h>
int main() {
  puts("Hello world");
}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,-emit-relocs
$QEMU $t/exe | grep -q 'Hello world'

readelf -S $t/exe | grep -Pq 'rela?\.text'

echo OK
