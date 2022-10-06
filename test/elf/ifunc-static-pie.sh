#!/bin/bash
. $(dirname $0)/common.inc

test_cflags -static || skip

[ $MACHINE = aarch64 ] && skip

# Skip if libc is musl because musl does not support GNU IFUNC
ldd --help 2>&1 | grep -q musl && skip

# We need to implement R_386_GOT32X relaxation to support PIE on i386
[ $MACHINE = i386 ] && skip

# IFUNC is not supported on RISC-V yet
[ $MACHINE = riscv64 -o $MACHINE = riscv32 ] && skip
[ $MACHINE = aarch64 ] && skip

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC
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

# Skip if the system does not support -static-pie
$CC -o $t/exe1 $t/a.o -static-pie >& /dev/null || skip
$QEMU $t/exe1 >& /dev/null || skip

$CC -B. -o $t/exe2 $t/a.o -static-pie
$QEMU $t/exe2 | grep -q 'Hello world'
