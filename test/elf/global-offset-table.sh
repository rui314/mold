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

[ $MACHINE = x86_64 ] || { echo skipped; exit; }

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
#include <stdio.h>
#include <stdint.h>

extern char _GLOBAL_OFFSET_TABLE_[];

int main() {
  printf("%lx", (unsigned long)_GLOBAL_OFFSET_TABLE_);
}
EOF

$CC -B. -no-pie -o $t/exe $t/a.o

# _GLOBAL_OFFSET_TABLE_ refers the end of .got only on x86.
# We assume .got is followed by .dynamic.
if [ $MACHINE = x86_64 -o $MACHINE = i386 -o $MACHINE = i686 ]; then
  readelf -WS $t/exe | grep "$($QEMU $t/exe) " | fgrep -q .dynamic
else
  readelf -WS $t/exe | grep "$($QEMU $t/exe) " | fgrep -q .got
fi

echo OK
