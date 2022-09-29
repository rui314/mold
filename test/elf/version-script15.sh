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

cat <<'EOF' > $t/a.ver
{
local:
  *;
global:
  [abc][^abc][^\]a-zABC];
};
EOF

cat <<EOF | $CXX -fPIC -c -o $t/b.o -xc -
void azZ() {}
void czZ() {}
void azC() {}
void aaZ() {}
EOF

$CC -B. -shared -Wl,--version-script=$t/a.ver -o $t/c.so $t/b.o

readelf --dyn-syms $t/c.so > $t/log
grep -q ' azZ$' $t/log
grep -q ' czZ$' $t/log
! grep -q ' azC$' $t/log || false
! grep -q ' aaZ$' $t/log || false

echo OK
