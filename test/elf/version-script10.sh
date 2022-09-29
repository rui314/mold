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

echo 'VER1 { foo[12]; }; VER2 {};' > $t/a.ver

cat <<EOF > $t/b.s
.globl foo1, foo2, foo3
foo1:
  nop
foo2:
  nop
foo3:
  nop
EOF

$CC -B. -shared -o $t/c.so -Wl,-version-script,$t/a.ver $t/b.s
readelf --dyn-syms $t/c.so > $t/log
grep -q ' foo1@@VER1$' $t/log
grep -q ' foo2@@VER1$' $t/log
! grep -q ' foo3@@VER1$' $t/log || false

echo OK
