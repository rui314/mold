#!/bin/bash
. $(dirname $0)/common.inc

# IFUNC is not supported on RISC-V yet
[ $MACHINE = riscv32 ] && skip
[ $MACHINE = riscv64 ] && skip

# Skip if libc is musl because musl does not support GNU FUNC
ldd --help 2>&1 | grep -q musl && skip

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

__attribute__((ifunc("resolve_foobar")))
static void foobar(void);

static void real_foobar(void) {
  printf("Hello world\n");
}

typedef void Func();

static Func *resolve_foobar(void) {
  return real_foobar;
}

int main() {
  foobar();
}
EOF

$CC -B. -o $t/exe1 $t/a.o -Wl,-z,lazy
$QEMU $t/exe1 | grep -q 'Hello world'

$CC -B. -o $t/exe2 $t/a.o -Wl,-z,now
$QEMU $t/exe2 | grep -q 'Hello world'
