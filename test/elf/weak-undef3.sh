#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = riscv64 -o $MACHINE = riscv32 ] || skip

cat <<EOF | $CC -c -o $t/a.o -xc -
#include <stdio.h>
#include <stdint.h>

int main() {
  uint64_t a;
  __asm__(
    ".weak foo\n"
    ".Lpcrel_hi_foo:\n"
    "\tauipc %1, %%pcrel_hi(foo)\n"
    "\taddi  %1, %0, %%pcrel_lo(.Lpcrel_hi_foo)"
    : "=r"(a)
    : "r"(a)
    :
  );

  printf("%d\n", a == 0);
}
EOF

$CC -B. -static -o $t/exe $t/a.o
$QEMU $t/exe | grep -q '^1$'
