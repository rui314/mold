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

cat <<EOF | $CC -o $t/a.o -fPIC -c -xc -
__attribute__((weak, visibility("hidden"))) void foo();
void bar() { foo(); }
EOF

$CC -B. -shared -o $t/b.so $t/a.o

readelf -W --dyn-syms $t/b.so > $t/log
! grep -qw foo $t/log || false
grep -qw bar $t/log

echo OK
