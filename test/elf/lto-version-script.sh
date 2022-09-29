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

cat <<EOF | $CC -flto -c -fPIC -o $t/a.o -xc -
void foo() {}
void bar() {}
EOF

cat <<EOF > $t/b.script
{
  global: foo;
  local: *;
};
EOF

$CC -B. -shared -o $t/c.so -flto $t/a.o -Wl,-version-script=$t/b.script
nm -D $t/c.so | grep -q 'T foo'
! nm -D $t/c.so | grep -q 'T bar' || false

echo OK
