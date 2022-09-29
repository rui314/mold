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

[[ $MACHINE = arm* ]] && flags=-marm

cat <<EOF | $CC -o $t/a.o -c -xc -fno-PIC $flags -
#include <stdio.h>

__attribute__((section(".fn1"))) void fn1() { printf(" fn1"); }
__attribute__((section(".fn2"))) void fn2() { printf(" fn2"); }

int main() {
  printf("main");
  fn1();
  fn2();
  printf(" %p %p\n", fn1, fn2);
}
EOF

$CC -B. -o $t/exe1 $t/a.o -no-pie \
  -Wl,--section-start=.fn1=0x10000000,--section-start=.fn2=0x20000000
$QEMU $t/exe1 | grep -q 'main fn1 fn2 0x10000000 0x20000000'

# PT_LOAD must be sorted on p_vaddr
readelf -W --segments $t/exe1 | grep ' LOAD ' | sed 's/0x[0-9a-f]*//' > $t/log1
diff $t/log1 <(sort $t/log1)

$CC -B. -o $t/exe2 $t/a.o -no-pie \
  -Wl,--section-start=.fn1=0x20000000,--section-start=.fn2=0x10000000
$QEMU $t/exe2 | grep -q 'main fn1 fn2 0x20000000 0x10000000'

readelf -W --segments $t/exe2 | grep ' LOAD ' | sed 's/0x[0-9a-f]*//' > $t/log2
diff $t/log2 <(sort $t/log2)

echo OK
