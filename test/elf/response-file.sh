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

cat <<EOF | $CC -c -o $t/a.o -xc -
void foo();
void bar();
int main() { foo(); bar(); }
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
void foo() {}
EOF

cat <<EOF | $CC -c -o $t/c.o -xc -
void bar() {}
EOF

echo "'$t/b.o' '$t/c.o'" > $t/rsp

$CC -o $t/exe $t/a.o -Wl,@$t/rsp

echo OK
