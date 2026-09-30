#!/usr/bin/env bash
. $(dirname $0)/common.inc

# .gcc_except_table.f2 is folded into .gcc_except_table.f1 by ICF. .Llsda2
# is not in the output symbol table, so emitted relocations referring to it
# must refer to .Llsda1's location relative to .gcc_except_table instead.

cat <<'EOF' | $CC -o $t/a.o -c -xassembler -
.section .text.f1,"ax",@progbits
.globl f1, lsda1
f1:
  .cfi_startproc
  .cfi_lsda 0x1b, .Llsda1
  li a0, 1
  ret
  .cfi_endproc
lsda1:
  lla a0, .Llsda1
  ret

.section .text.f2,"ax",@progbits
.globl f2, lsda2
f2:
  .cfi_startproc
  .cfi_lsda 0x1b, .Llsda2
  li a0, 2
  ret
  .cfi_endproc
lsda2:
  lla a0, .Llsda2
  ret

.section .gcc_except_table.f1,"a",@progbits
.Llsda1:
  .byte 0xff, 0xff, 0x01, 0x00

.section .gcc_except_table.f2,"a",@progbits
.Llsda2:
  .byte 0xff, 0xff, 0x01, 0x00
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>
int f1(), f2();
void *lsda1(), *lsda2();
int main() { printf("%d %d %d\n", f1(), f2(), lsda1() == lsda2()); }
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,--icf=all,--emit-relocs
$QEMU $t/exe | grep '^1 2 1$'

readelf -rW $t/exe > $t/log
grep -E 'R_RISCV_32_PCREL +[0-9a-f]+ \.Llsda1 \+ 0$' $t/log
grep -E 'R_RISCV_32_PCREL +[0-9a-f]+ \.gcc_except_table \+ 0$' $t/log
grep -E 'R_RISCV_PCREL_HI20 +[0-9a-f]+ \.Llsda1 \+ 0$' $t/log
grep -E 'R_RISCV_PCREL_HI20 +[0-9a-f]+ \.gcc_except_table \+ 0$' $t/log
