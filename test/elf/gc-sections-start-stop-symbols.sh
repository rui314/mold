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

cat <<EOF | $CC -o $t/a.o -c -xc -
int x1 __attribute__((section("foo_section"))) = 3;
int x2 __attribute__((section("bar_section"))) = 5;
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>

extern int __start_foo_section[];

int main() {
  printf("foo=%d\n", __start_foo_section[0]);
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,-gc-sections
$t/exe | grep -q 'foo=3'

readelf --sections $t/exe > $t/log
grep -q foo_section $t/log
! grep -q bar_section $t/log || false

echo OK
