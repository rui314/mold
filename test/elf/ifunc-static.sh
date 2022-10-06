#!/bin/bash
. $(dirname $0)/common.inc

test_cflags -static || skip

# IFUNC is not supported on RISC-V yet
[ $MACHINE = riscv64 -o $MACHINE = riscv32 ] && skip

# Skip if libc is musl because musl does not support GNU IFUNC
ldd --help 2>&1 | grep -q musl && skip

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

void foo() __attribute__((ifunc("resolve_foo")));

void hello() {
  printf("Hello world\n");
}

void *resolve_foo() {
  return hello;
}

int main() {
  foo();
  return 0;
}
EOF

$CC -B. -o $t/exe $t/a.o -static
$QEMU $t/exe | grep -q 'Hello world'
