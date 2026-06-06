#!/bin/bash
. $(dirname $0)/common.inc

# When mold relaxes an instruction sequence, the relocations attached to the
# original instructions no longer match the relaxed code. With --emit-relocs,
# mold must rewrite them so that the emitted relocations describe the relaxed
# instructions, both in type and in offset.

cat <<'EOF' | $CC -o $t/a.o -c -xassembler -
.text
.globl _start
_start:
  # pcaddu18i + jirl => bl  (R_LARCH_CALL36 => R_LARCH_B26)
  .reloc ., R_LARCH_RELAX
  pcaddu18i $ra, %call36(get_zero)
  jirl $ra, $ra, 0

  # pcalau12i + addi.d => pcaddi  (R_LARCH_PCALA_* => R_LARCH_PCREL20_S2)
  .reloc ., R_LARCH_RELAX
  pcalau12i $t0, %pc_hi20(v)
  .reloc ., R_LARCH_RELAX
  addi.d $t0, $t0, %pc_lo12(v)
  ld.w $t0, $t0, 0
  or $a0, $a0, $t0

  # pcalau12i + ld.d => pcaddi  (R_LARCH_GOT_PC_* => R_LARCH_PCREL20_S2)
  .reloc ., R_LARCH_RELAX
  pcalau12i $t1, %got_pc_hi20(v)
  .reloc ., R_LARCH_RELAX
  ld.d $t1, $t1, %got_pc_lo12(v)
  ld.w $t1, $t1, 0
  or $a0, $a0, $t1

  li.w $a7, 93           # __NR_exit
  syscall 0

get_zero:
  li.w $a0, 0
  ret

.section .rodata,"a"
.p2align 2
.hidden v
v:
  .word 0
EOF

$CC -B. -o $t/exe $t/a.o --static -nostdlib -Wl,-e,_start,--emit-relocs,--relax
$QEMU $t/exe

$OBJDUMP -dr $t/exe > $t/exe.objdump

# Relaxation took place.
grep -Fw pcaddi $t/exe.objdump
grep -Fw bl $t/exe.objdump

# The relocations were rewritten to match the relaxed instructions. The dead
# high-part relocations are turned into R_LARCH_NONE.
grep -Fw R_LARCH_PCREL20_S2 $t/exe.objdump
grep -Fw R_LARCH_B26 $t/exe.objdump
grep -Fw R_LARCH_NONE $t/exe.objdump

# The stale pre-relaxation relocations are gone.
not grep -Fw R_LARCH_PCALA_HI20 $t/exe.objdump
not grep -Fw R_LARCH_PCALA_LO12 $t/exe.objdump
not grep -Fw R_LARCH_GOT_PC_HI20 $t/exe.objdump
not grep -Fw R_LARCH_GOT_PC_LO12 $t/exe.objdump
not grep -Fw R_LARCH_CALL36 $t/exe.objdump

# Each rewritten relocation must sit at the address of the relaxed instruction
# it describes, not at its original pre-relaxation offset. This checks that the
# relocation offsets were adjusted: R_LARCH_PCREL20_S2 must land on a pcaddi
# and R_LARCH_B26 on a b/bl.
awk '
  $3 == "pcaddi"             { insn[$1] = $3 }
  $3 == "b" || $3 == "bl"    { insn[$1] = $3 }
  $2 == "R_LARCH_PCREL20_S2" { if (insn[$1] != "pcaddi")       { bad=1; print "bad offset: " $0 } }
  $2 == "R_LARCH_B26"        { if (insn[$1] !~ /^bl?$/)        { bad=1; print "bad offset: " $0 } }
  END { exit bad+0 }
' $t/exe.objdump
