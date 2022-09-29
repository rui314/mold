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

cat <<EOF | $CC -o $t/a.so -shared -fPIC -xc -
void foo() {}
EOF

cat <<EOF | $CC -o $t/b.o -fPIC -c -xc -
__attribute__((visibility("hidden"))) void foo();
int main() { foo(); }
EOF

! $CC -B. -o $t/exe $t/a.so $t/b.o >& $t/log
grep -q 'undefined symbol: foo' $t/log

echo OK
