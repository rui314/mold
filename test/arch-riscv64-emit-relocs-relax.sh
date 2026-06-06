#!/bin/bash
. $(dirname $0)/common.inc

# When mold relaxes instructions, the relocations attached to the relaxed code
# must be rewritten for --emit-relocs. A shortened call still references its
# symbol, so R_RISCV_CALL_PLT becomes R_RISCV_JAL or R_RISCV_RVC_JUMP. A
# relaxation that folds an address into the instruction as a link-time
# constant (lui/addi or an AUIPC+LD GOT load) leaves the instruction without a
# relocation, so we emit R_RISCV_NONE.

cat <<'EOF' | $CC -o $t/a.o -c -xassembler -
.text
.globl _start
_start:
  # CALL_PLT => jal (near target)
  .reloc ., R_RISCV_RELAX
  call func

  # lui + addi of a small absolute symbol => lui is deleted (HI20 => NONE),
  # while the low-part relocation stays.
  lui t0, %hi(small)
  .reloc ., R_RISCV_RELAX
  addi t0, t0, %lo(small)

  # GOT load of an absolute symbol => the value is materialized directly
  # (GOT_HI20 and its paired PCREL_LO12_I both => NONE).
got_anchor:
  .reloc ., R_RISCV_GOT_HI20, small
  .reloc ., R_RISCV_RELAX
  auipc t1, 0
  .reloc ., R_RISCV_PCREL_LO12_I, got_anchor
  .reloc ., R_RISCV_RELAX
  ld t1, 0(t1)

  # CALL with rd=zero (tail call) to a near target => c.j (=> RVC_JUMP)
  .reloc ., R_RISCV_RELAX
  tail do_exit

func:
  ret

do_exit:
  li a0, 0
  li a7, 93           # __NR_exit
  ecall
EOF

cat <<'EOF' | $CC -o $t/b.o -c -xassembler -
.globl small
small = 0xba
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o --static -nostdlib -Wl,-e,_start,--emit-relocs,--relax
$QEMU $t/exe

$OBJDUMP -dr $t/exe > $t/exe.objdump

# A shortened call keeps a relocation that names the symbol.
grep -Fw R_RISCV_JAL $t/exe.objdump
grep -Fw R_RISCV_RVC_JUMP $t/exe.objdump

# Address-materializing relaxations drop their relocations.
grep -Fw R_RISCV_NONE $t/exe.objdump

# The stale pre-relaxation relocations are gone.
not grep -Fw R_RISCV_CALL_PLT $t/exe.objdump
not grep -Fw R_RISCV_GOT_HI20 $t/exe.objdump
not grep -Fw R_RISCV_HI20 $t/exe.objdump

# The low-part relocation of the lui/addi pair survives (the addi is kept).
grep -Fw R_RISCV_LO12_I $t/exe.objdump

# Each rewritten call relocation must sit on the relaxed instruction it
# describes: R_RISCV_JAL on a jal and R_RISCV_RVC_JUMP on a c.j. This checks
# that the relocation offsets were adjusted for the deleted bytes.
awk '
  $3 == "jal"              { insn[$1] = "jal" }
  $3 == "j" || $3 == "c.j" { insn[$1] = "cj" }
  $2 == "R_RISCV_JAL"      { if (insn[$1] != "jal") { bad=1; print "bad offset: " $0 } }
  $2 == "R_RISCV_RVC_JUMP" { if (insn[$1] != "cj")  { bad=1; print "bad offset: " $0 } }
  END { exit bad+0 }
' $t/exe.objdump

# TLS local-exec: `lui + add` that materializes TP + %tprel_hi are deleted for
# a variable within 2 KiB of TP (TPREL_HI20 and TPREL_ADD => NONE), while the
# access relative to TP keeps its low-part relocation.
cat <<'EOF' | $CC -o $t/tls.o -c -xc - -O2 -fno-section-anchors
__thread int tv;
int main(void) { return tv; }     // tv is zero-initialized, so exits 0
EOF

$CC -B. -o $t/tls $t/tls.o -static -Wl,--emit-relocs,--relax
$QEMU $t/tls

$OBJDUMP -dr $t/tls > $t/tls.objdump
grep -E $'R_RISCV_TPREL_LO12_[IS][ \t]+tv' $t/tls.objdump
not grep -E $'R_RISCV_TPREL_HI20[ \t]+tv' $t/tls.objdump
not grep -E $'R_RISCV_TPREL_ADD[ \t]+tv' $t/tls.objdump
