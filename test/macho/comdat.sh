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

cat <<EOF | $CC -o $t/a.o -c -xc++ -
#include <iostream>
struct T {
  T() { std::cout << "foo\n"; }
};
T x;
EOF

cat <<EOF | $CC -o $t/b.o -c -xc++ -
#include <iostream>
struct T {
  T() { std::cout << "foo\n"; }
};
T y;
EOF

cat <<EOF | $CC -o $t/c.o -c -xc++ -
int main() {}
EOF

clang++ --ld-path=./ld64 -o $t/exe $t/a.o $t/b.o $t/c.o

echo OK
