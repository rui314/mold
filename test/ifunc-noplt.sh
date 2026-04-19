#!/bin/bash
. $(dirname $0)/common.inc

supports_ifunc || skip

cat <<EOF | $CC -fPIC -o $t/a.o -c -xc - -fno-plt
#include <stdio.h>

__attribute__((ifunc("resolve_foo")))
void foo(void);

void hello(void) {
  printf("Hello world\n");
}

typedef void Fn();

Fn *resolve_foo(void) {
  return hello;
}

int main() {
  foo();
}
EOF

$CC -B. -o $t/exe1 $t/a.o -pie
$QEMU $t/exe1 | grep 'Hello world'

$CC -B. -o $t/exe2 $t/a.o -no-pie
$QEMU $t/exe2 | grep 'Hello world'
