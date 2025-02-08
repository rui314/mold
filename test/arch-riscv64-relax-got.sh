#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.globl get_sym1, get_sym2, get_sym3, get_sym4, get_sym5
.option norvc
get_sym1:
  .reloc ., R_RISCV_GOT_HI20, sym1
  .reloc ., R_RISCV_RELAX
  auipc a0, 0
  .reloc ., R_RISCV_PCREL_LO12_I, get_sym1
  .reloc ., R_RISCV_RELAX
  ld a0, 0(a0)
  ret
get_sym2:
  .reloc ., R_RISCV_GOT_HI20, sym2
  .reloc ., R_RISCV_RELAX
  auipc a0, 0
  .reloc ., R_RISCV_PCREL_LO12_I, get_sym2
  .reloc ., R_RISCV_RELAX
  ld a0, 0(a0)
  ret
get_sym3:
  .reloc ., R_RISCV_GOT_HI20, sym3
  .reloc ., R_RISCV_RELAX
  auipc a0, 0
  .reloc ., R_RISCV_PCREL_LO12_I, get_sym3
  .reloc ., R_RISCV_RELAX
  ld a0, 0(a0)
  ret
get_sym4:
  .reloc ., R_RISCV_GOT_HI20, sym4
  .reloc ., R_RISCV_RELAX
  auipc a0, 0
  .reloc ., R_RISCV_PCREL_LO12_I, get_sym4
  .reloc ., R_RISCV_RELAX
  ld a0, 0(a0)
  ret
get_sym5:
  .reloc ., R_RISCV_GOT_HI20, sym5
  .reloc ., R_RISCV_RELAX
  auipc a0, 0
  .reloc ., R_RISCV_PCREL_LO12_I, get_sym5
  .reloc ., R_RISCV_RELAX
  ld a0, 0(a0)
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
$QEMU $t/exe1 | grep -E '^0 ba beef 11beef deadbeef$'

$CC -B. -o $t/exe2 $t/a.o $t/b.o $t/c.o
$QEMU $t/exe2 | grep -E '^0 ba beef 11beef deadbeef$'

$OBJDUMP -d $t/exe2 | grep -A2 '<get_sym2>:' | grep -E $'li[ \t]+a0,186$'
