#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = m68k ] && skip
[ $MACHINE = sh4 ] && skip

cat <<EOF | $CXX -o $t/a.o -c -fPIC -xc++ -g -ffunction-sections -
#include <iostream>
struct Foo {
  Foo() { std::cout << "Hello world\n"; }
};

Foo x;
EOF

cat <<EOF | $CXX -o $t/b.o -c -fPIC -xc++ -g -ffunction-sections -
#include <iostream>
struct Foo {
  Foo() { std::cout << "Hello world\n"; }
};

Foo y;
int main() {}
EOF

$CXX -B. -o $t/exe $t/a.o $t/b.o -Wl,-emit-relocs
$QEMU $t/exe | grep -q 'Hello world'
