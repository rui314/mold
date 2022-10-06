#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc++ -
#include <iostream>
static struct Foo1 {
  Foo1() { std::cout << "foo1 "; }
} x;

static struct Foo2 {
  Foo2() { std::cout << "foo2 "; }
} y;
EOF

cat <<EOF | $CC -o $t/b.o -c -xc++ -
#include <iostream>
static struct Foo3 {
  Foo3() { std::cout << "foo3 "; }
} x;

static struct Foo4 {
  Foo4() { std::cout << "foo4 "; }
} y;
EOF

cat <<EOF | $CC -o $t/c.o -c -xc++ -
#include <iostream>
static struct Foo5 {
  Foo5() { std::cout << "foo5 "; }
} x;

static struct Foo6 {
  Foo6() { std::cout << "foo6 "; }
} y;

int main() { std::cout << "\n"; }
EOF

$CXX -B. -o $t/exe1 $t/a.o $t/b.o $t/c.o
$QEMU $t/exe1 | grep -q 'foo1 foo2 foo3 foo4 foo5 foo6'

$CXX -B. -o $t/exe2 $t/a.o $t/b.o $t/c.o -Wl,--reverse-sections
$QEMU $t/exe2 | grep -q 'foo5 foo6 foo3 foo4 foo1 foo2'
