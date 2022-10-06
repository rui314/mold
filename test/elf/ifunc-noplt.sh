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

$CC -B. -o $t/exe $t/a.o
$QEMU $t/exe | grep -q 'Hello world'
