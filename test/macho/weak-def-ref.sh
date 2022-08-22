#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | c++ -o $t/a.o -c -xc++ -
#include <iostream>

struct Foo {
  Foo() { std::cout << "foo\n"; }
};

Foo x;

int main() {}
EOF

c++ --ld-path=./ld64 -o $t/exe $t/a.o
objdump --macho --exports-trie $t/exe > $t/log
! grep -q __ZN3FooC1Ev $t/log || false

echo OK
