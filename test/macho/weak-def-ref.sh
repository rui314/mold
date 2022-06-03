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

cat <<EOF | $CXX -o $t/a.o -c -xc++ -
#include <iostream>

struct Foo {
  Foo() { std::cout << "foo\n"; }
};

Foo x;

int main() {}
EOF

clang++ --ld-path=./ld64 -o $t/exe $t/a.o
objdump --macho --exports-trie $t/exe > $t/log
! grep -q __ZN3FooC1Ev $t/log || false

echo OK
