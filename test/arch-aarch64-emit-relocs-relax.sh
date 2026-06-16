#!/bin/bash
. $(dirname $0)/common.inc

# When mold relaxes an instruction sequence, the relocations attached to the
# original instructions no longer match the relaxed code. With --emit-relocs,
# mold must rewrite the emitted relocations so that their types describe the
# relaxed instructions.

cat <<'EOF' | $CC -o $t/a.o -c -xassembler -
.text
.globl _start
_start:
  # GOT-load relaxation: ADRP+LDR => ADRP+ADD
  #   R_AARCH64_ADR_GOT_PAGE     => R_AARCH64_ADR_PREL_PG_HI21
  #   R_AARCH64_LD64_GOT_LO12_NC => R_AARCH64_ADD_ABS_LO12_NC
  adrp x0, :got:gvar
  ldr  x0, [x0, #:got_lo12:gvar]

  # ADRP+ADD => NOP+ADR (gvar is within +-1 MiB)
  #   R_AARCH64_ADR_PREL_PG_HI21 => R_AARCH64_NONE
  #   R_AARCH64_ADD_ABS_LO12_NC  => R_AARCH64_ADR_PREL_LO21
  adrp x1, gvar
  add  x1, x1, #:lo12:gvar

  # Both sequences must materialize the same address.
  cmp  x0, x1
  bne  1f

  # ... and it must point at gvar, whose value is 42.
  ldr  w2, [x0]
  cmp  w2, #42
  bne  1f

  mov  x0, #0
  b    2f
1:
  mov  x0, #1
2:
  mov  x8, #93           // __NR_exit
  svc  #0

.data
.hidden gvar
gvar:
  .word 42
EOF

$CC -B. -o $t/exe $t/a.o --static -nostdlib -Wl,-e,_start,--emit-relocs,--relax
$QEMU $t/exe

$OBJDUMP -dr $t/exe > $t/exe.objdump

# Relaxation took place.
grep -Fw nop $t/exe.objdump
grep -Fw adr $t/exe.objdump
grep -Fw add $t/exe.objdump

# The relocations were rewritten to match the relaxed instructions.
grep -Fw R_AARCH64_ADR_PREL_PG_HI21 $t/exe.objdump
grep -Fw R_AARCH64_ADD_ABS_LO12_NC $t/exe.objdump
grep -Fw R_AARCH64_ADR_PREL_LO21 $t/exe.objdump

# The stale pre-relaxation relocations are gone.
not grep -Fw R_AARCH64_ADR_GOT_PAGE $t/exe.objdump
not grep -Fw R_AARCH64_LD64_GOT_LO12_NC $t/exe.objdump

# Each rewritten relocation must sit on the relaxed instruction it describes.
awk '
  $3 == "adrp" || $3 == "add" || $3 == "adr" || $3 == "nop" { insn[$1] = $3 }
  $2 == "R_AARCH64_ADR_PREL_PG_HI21" { if (insn[$1] != "adrp") { bad=1; print "bad offset: " $0 } }
  $2 == "R_AARCH64_ADD_ABS_LO12_NC"  { if (insn[$1] != "add")  { bad=1; print "bad offset: " $0 } }
  $2 == "R_AARCH64_ADR_PREL_LO21"    { if (insn[$1] != "adr")  { bad=1; print "bad offset: " $0 } }
  END { exit bad+0 }
' $t/exe.objdump
