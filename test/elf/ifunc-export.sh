#!/bin/bash
. $(dirname $0)/common.inc

# IFUNC is not supported on RISC-V yet
[ $MACHINE = riscv32 ] && skip
[ $MACHINE = riscv64 ] && skip

# Skip if libc is musl because musl does not support GNU FUNC
ldd --help 2>&1 | grep -q musl && skip

cat <<EOF | $CC -c -fPIC -o $t/a.o -xc -
#include <stdio.h>

__attribute__((ifunc("resolve_foobar")))
void foobar(void);

void real_foobar(void) {
  printf("Hello world\n");
}

typedef void Func();

Func *resolve_foobar(void) {
  return real_foobar;
}
EOF

$CC -B. -shared -o $t/b.so $t/a.o
readelf --dyn-syms $t/b.so | grep -Eq '(IFUNC|<OS specific>: 10)\s+GLOBAL DEFAULT   .* foobar'
