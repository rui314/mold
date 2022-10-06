#!/bin/bash
. $(dirname $0)/common.inc

command -v dwarfdump >& /dev/null || skip

cat <<EOF | $CXX -c -o $t/a.o -g -xc++ -
#include <iostream>
struct Foo {
  Foo() { std::cout << "Hello world\n"; }
};
Foo x;
EOF

cat <<EOF | $CXX -c -o $t/b.o -g -xc++ -
#include <iostream>
struct Foo {
  Foo() { std::cout << "Hello world\n"; }
};
Foo y;
EOF

cat <<EOF | $CXX -o $t/c.o -c -xc++ -g -
int main() {}
EOF

$CXX -B. -o $t/exe $t/a.o $t/b.o $t/c.o -g
$QEMU $t/exe | grep -q 'Hello world'

dwarfdump $t/exe > /dev/null
