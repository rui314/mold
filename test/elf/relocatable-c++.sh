#!/bin/bash
. $(dirname $0)/common.inc

# OneTBB isn't tsan-clean
nm mold | grep -q '__tsan_init' && skip

# Ubuntu 22.04 SH4 GCC is broken
[ $MACHINE = sh4 ] && skip

cat <<EOF | $CXX -c -o $t/a.o -xc++ -
void hello();
void world();

template <typename T>
struct Foo {
  Foo() { hello(); }
};

template <typename T>
struct Bar {
  Bar() { world(); }
};

void baz() {
  Foo<int> foo;
  Bar<int> bar;
}
EOF

cat <<EOF | $CXX -c -o $t/b.o -xc++ -
#include <iostream>

void hello() { std::cout << "Hello "; }
void world() { std::cout << "world\n"; }
void baz();

int main() {
  baz();
}
EOF

./mold --relocatable -o $t/c.o $t/a.o
./mold --relocatable -o $t/d.o $t/b.o

$CXX -B. -o $t/exe $t/c.o $t/d.o
$QEMU $t/exe | grep -q 'Hello world'
