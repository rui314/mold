#!/bin/bash
. $(dirname $0)/common.inc

supports_ifunc || skip
[ "$(uname)" = FreeBSD ] && skip

cat <<EOF | $GXX -o $t/a.o -c -xc++ - -fPIC
#include <iostream>

class Hello {
public:
  __attribute__((target("default"))) void say() { std::cout << "Hello\n"; }
  __attribute__((target("popcnt")))  void say() { std::cout << "Howdy\n"; }
};

void hello() {
  Hello().say();
}
EOF

$CXX -B. -shared -o $t/b.so $t/a.o

cat <<EOF | $CXX -o $t/c.o -c -xc++ - -fPIC
void hello();
int main() { hello(); }
EOF

$CXX -B. -o $t/exe $t/b.so $t/c.o
$QEMU $t/exe | grep '^H'
