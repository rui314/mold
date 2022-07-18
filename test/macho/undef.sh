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
t=out/test/macho/$MACHINE/$testname
mkdir -p $t

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF

rm -f $t/b.a
ar rcs $t/b.a $t/a.o

cat <<EOF | $CC -o $t/c.o -c -xc -
int main() {}
EOF

clang --ld-path=./ld64 -o $t/exe1 $t/b.a $t/c.o
nm $t/exe1 > $t/log1
! grep -q _foo $t/log1 || false

clang --ld-path=./ld64 -o $t/exe2 $t/b.a $t/c.o -Wl,-u,_foo
nm $t/exe2 > $t/log2
grep -q _foo $t/log2

echo OK
