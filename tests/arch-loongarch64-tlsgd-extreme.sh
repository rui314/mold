#!/usr/bin/env bash
. $(dirname $0)/common.inc

# In the extreme code model, the pcalau12i with R_LARCH_TLS_{GD,LD}_PC_HI20
# is the first instruction of a 64-bit sequence, so the GOT may be more than
# 2 GiB away from it. .high is placed 4 GiB above the GOT, which follows .text.
# In a static executable, the first word of a TLSGD GOT entry is module ID 1.

cat <<'EOF' | $CC -o $t/a.o -c -xassembler -
.globl _start
_start:
  pcaddu18i $ra, %call36(get_modid)
  jirl      $ra, $ra, 0
  addi.d    $a0, $a0, -2
  li.w      $a7, 93            # __NR_exit
  syscall   0

.section .high,"ax"
get_modid:
  pcalau12i $t0, %gd_pc_hi20(foo)
  addi.d    $t1, $zero, %got_pc_lo12(foo)
  lu32i.d   $t1, %got64_pc_lo20(foo)
  lu52i.d   $t1, $t1, %got64_pc_hi12(foo)
  ldx.d     $a0, $t0, $t1

  pcalau12i $t0, %ld_pc_hi20(bar)
  addi.d    $t1, $zero, %got_pc_lo12(bar)
  lu32i.d   $t1, %got64_pc_lo20(bar)
  lu52i.d   $t1, $t1, %got64_pc_hi12(bar)
  ldx.d     $t0, $t0, $t1
  add.d     $a0, $a0, $t0
  ret

.section .tbss,"awT",@nobits
foo:
  .zero 4
bar:
  .zero 4
EOF

./mold -static -o $t/exe $t/a.o \
  --section-start=.text=0x300000 --section-start=.high=0x100000000
$QEMU $t/exe
