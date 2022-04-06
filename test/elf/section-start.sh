#!/bin/bash
export LC_ALL=C
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
GCC="${GCC:-gcc}"
GXX="${GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t=out/test/elf/$testname
mkdir -p $t

cat <<EOF | $CC -o $t/a.o -c -xc -fno-PIC -
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF

$CC -B. -o $t/exe $t/a.o -no-pie -Wl,-section-start=.text=0x606000
$QEMU $t/exe | grep -q 'Hello world'
readelf -W --sections $t/exe | grep -q '\.text .*00606000'

$CC -B. -o $t/exe $t/a.o -no-pie -Wl,-Ttext=804000
$QEMU $t/exe | grep -q 'Hello world'
readelf -W --sections $t/exe | grep -q '\.text .*00804000'

echo OK
