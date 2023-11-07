#!/bin/bash
. $(dirname $0)/common.inc

supports_ifunc || skip

cat <<EOF | $CC -c -fPIC -o $t/a.o -c -xc -
typedef int Fn();

int foo() __attribute__((ifunc("resolve_foo")));

int real_foo() { return 3; }

Fn *resolve_foo(void) {
  return real_foo;
}
EOF

cat <<EOF | $CC -c -fPIC -o $t/b.o -xc -
typedef int Fn();
int foo();
Fn *get_foo() { return foo; }
EOF

cat <<EOF | $CC -c -fPIC -o $t/c.o -xc -
#include <stdio.h>

typedef int Fn();
Fn *get_foo();

int main() {
  Fn *f = get_foo();
  printf("%d\n", f());
}
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o $t/c.o -pie
$QEMU $t/exe1 | grep -q '^3$'

$CC -B. -o $t/exe2 $t/a.o $t/b.o $t/c.o -no-pie
$QEMU $t/exe2 | grep -q '^3$'
