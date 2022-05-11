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
cd "$(dirname "$0")"/../..
t=out/test/macho/$testname
mkdir -p $t

cat <<EOF | $CC -o $t/a.o -c -xc -
int foo = 3;
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
int bar = 5;
EOF

rm -f $t/c.a
ar rc $t/c.a $t/a.o $t/b.o

cat <<EOF | $CC -o $t/d.o -c -xc -
int main() {}
EOF

clang --ld-path=./ld64 -o $t/exe $t/d.o -Wl,-all_load $t/c.a
nm $t/exe | grep -q 'D _foo$'
nm $t/exe | grep -q 'D _bar$'

echo OK
