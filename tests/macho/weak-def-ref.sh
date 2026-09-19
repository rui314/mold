#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CXX -o $t/a.o -c -xc++ -
#include <iostream>

struct Foo {
  Foo() { std::cout << "foo\n"; }
};

Foo x;

int main() {}
EOF

$CXX --ld-path=$mold -o $t/exe $t/a.o
objdump --macho --exports-trie $t/exe > $t/log
! grep -q __ZN3FooC1Ev $t/log || false
