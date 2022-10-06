#!/bin/bash
. $(dirname $0)/common.inc

# IFUNC is not supported on RISC-V yet
[ $MACHINE = riscv64 -o $MACHINE = riscv32 ] && skip

# Skip if libc is musl because musl does not support GNU FUNC
ldd --help 2>&1 | grep -q musl && skip

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
