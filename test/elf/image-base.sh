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
  return 0;
}
EOF

$CC -B. -no-pie -o $t/exe1 $t/a.o -Wl,--image-base=0x8000000
$QEMU $t/exe1 | grep -q 'Hello world'
readelf -W --sections $t/exe1 | grep -Eq '.interp\s+PROGBITS\s+0*8000...\b'

cat <<EOF | $CC -o $t/b.o -c -xc -
void _start() {}
EOF

if [ $MACHINE = x86-64 -o $MACHINE = aarch64 ]; then
  $CC -B. -no-pie -o $t/exe2 $t/b.o -nostdlib -Wl,--image-base=0xffffffff80000000
  readelf -W --sections $t/exe2 | grep -Eq '.interp\s+PROGBITS\s+ffffffff80000...\b'
fi

echo OK
