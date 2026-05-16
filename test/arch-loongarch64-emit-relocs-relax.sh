#!/bin/bash
. $(dirname $0)/common.inc

cat <<'EOF' | $CC -o $t/a.o -c -xassembler -
.text
.globl _start
_start:
  move $s0, $ra

  .reloc ., R_LARCH_RELAX
  pcalau12i $t0, %pc_hi20(data_1)
  .reloc ., R_LARCH_RELAX
  addi.d $t0, $t0, %pc_lo12(data_1)
  ld.d $t0, $t0, 0

  .reloc ., R_LARCH_RELAX
  pcalau12i $t1, %got_pc_hi20(data_2)
  .reloc ., R_LARCH_RELAX
  ld.d $t1, $t1, %got_pc_lo12(data_2)
  ld.d $t1, $t1, 0

  pcaddu18i $t2, %call36(call_target)
  jirl $ra, $t2, 0
  move $t2, $a0

  add.d $a0, $t0, $t1
  add.d $a0, $a0, $t2

  pcaddu18i $t0, %call36(tail_target)
  jirl $zero, $t0, 0

call_target:
  li.d $a0, 0x33
  ret

tail_target:
  li.d $t0, 0x66
  sub.d $a0, $a0, $t0
  # exit(0)
  li.d $a7, 93
  syscall 0

.section .rodata,"a"
.p2align 2
data_1:
  .8byte 0x11

.data
.globl data_2
.hidden data_2
.p2align 3
data_2:
  .8byte 0x22
EOF

$CC -B. -o $t/exe $t/a.o --static -nostdlib -Wl,-e,_start,--emit-relocs,--relax
$QEMU $t/exe

$OBJDUMP -dr $t/exe > $t/exe.objdump
grep -E 'R_LARCH_PCREL20_S2[[:space:]]+data_1' $t/exe.objdump
grep -E 'R_LARCH_PCREL20_S2[[:space:]]+data_2' $t/exe.objdump
grep -E 'R_LARCH_B26[[:space:]]+call_target' $t/exe.objdump
grep -E 'R_LARCH_B26[[:space:]]+tail_target' $t/exe.objdump
grep -Fw R_LARCH_NONE $t/exe.objdump

not grep -Fw R_LARCH_PCALA_HI20 $t/exe.objdump
not grep -Fw R_LARCH_PCALA_LO12 $t/exe.objdump
not grep -Fw R_LARCH_GOT_PC_HI20 $t/exe.objdump
not grep -Fw R_LARCH_GOT_PC_LO12 $t/exe.objdump
not grep -Fw R_LARCH_CALL36 $t/exe.objdump
