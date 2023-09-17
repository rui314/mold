#!/bin/bash
. $(dirname $0)/common.inc

[[ $MACHINE = riscv* ]] || skip

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.globl get_sym1, get_sym2, get_sym3, get_sym4, get_sym5
get_sym1:
  la a0, sym1
  ret
get_sym2:
  la a0, sym2
  ret
get_sym3:
  la a0, sym3
  ret
get_sym4:
  la a0, sym4
  ret
get_sym5:
  la a0, sym5
  ret
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.globl sym1, sym2, sym3, sym4, sym5
sym1 = 0x0
sym2 = 0xba
sym3 = 0xbeef
sym4 = 0x11beef
sym5 = 0xdeadbeef
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>

int get_sym1();
int get_sym2();
int get_sym3();
int get_sym4();
int get_sym5();

int main() {
  printf("%x %x %x %x %x\n",
         get_sym1(), get_sym2(), get_sym3(), get_sym4(), get_sym5());
}
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o $t/c.o -Wl,--no-relax
$QEMU $t/exe1 | grep -Eq '^0 ba beef 11beef deadbeef$'

$CC -B. -o $t/exe2 $t/a.o $t/b.o $t/c.o
$QEMU $t/exe2 | grep -Eq '^0 ba beef 11beef deadbeef$'
