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

cat <<EOF | $CC -c -fPIC -xc -o $t/a.o -
extern int foo;
int x() { return foo; }
EOF

cat <<EOF | $CC -c -fPIC -xc -o $t/b.o -
extern int foo;
int y() { return foo; }
int main() {}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,--warn-unresolved-symbols,--warn-once >& $t/log

[ "$(grep 'undefined symbol:.* foo$' $t/log | wc -l)" = 1 ]

echo OK
