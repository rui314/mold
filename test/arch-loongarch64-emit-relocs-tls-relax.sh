#!/bin/bash
. $(dirname $0)/common.inc

# TLS LE

cat <<'EOF' | $CC -o $t/le.o -c -xassembler -
.text
.globl foo
foo:
  lu12i.w $t0, %le_hi20_r(tls_local)
  add.d $t0, $t0, $tp, %le_add_r(tls_local)
  ld.d $a0, $t0, %le_lo12_r(tls_local)

  .reloc ., R_LARCH_RELAX
  pcalau12i $a0, %desc_pc_hi20(tls_local_small)
  .reloc ., R_LARCH_RELAX
  addi.d $a0, $a0, %desc_pc_lo12(tls_local_small)
  .reloc ., R_LARCH_RELAX
  ld.d $ra, $a0, %desc_ld(tls_local_small)
  .reloc ., R_LARCH_RELAX
  jirl $ra, $ra, %desc_call(tls_local_small)

  .reloc ., R_LARCH_RELAX
  pcalau12i $a0, %desc_pc_hi20(tls_local_large)
  .reloc ., R_LARCH_RELAX
  addi.d $a0, $a0, %desc_pc_lo12(tls_local_large)
  .reloc ., R_LARCH_RELAX
  ld.d $ra, $a0, %desc_ld(tls_local_large)
  .reloc ., R_LARCH_RELAX
  jirl $ra, $ra, %desc_call(tls_local_large)

  ret

.section .tbss,"awT",@nobits
.globl tls_local
.p2align 2
tls_local:
  .8byte 0

.globl tls_local_small
tls_local_small:
  .8byte 0

  .zero 8192

.globl tls_local_large
tls_local_large:
  .8byte 0
EOF

cat <<'EOF' | $CC -o $t/le-main.o -c -xc -
extern void foo(void);

int main() {
  foo();
  return 0;
}
EOF

$CC -B. -o $t/le $t/le.o $t/le-main.o -Wl,--emit-relocs,--relax
$QEMU $t/le
$OBJDUMP -dr $t/le > $t/le.objdump
grep -E 'R_LARCH_TLS_LE_LO12_R[[:space:]]+tls_local' $t/le.objdump
grep -E 'R_LARCH_TLS_LE_HI20[[:space:]]+tls_local_large' $t/le.objdump
grep -E 'R_LARCH_TLS_LE_LO12[[:space:]]+tls_local_(small|large)' $t/le.objdump
grep -Fw R_LARCH_NONE $t/le.objdump
not grep -Fw R_LARCH_TLS_LE_HI20_R $t/le.objdump
not grep -Fw R_LARCH_TLS_LE_ADD_R $t/le.objdump
not grep -Fw R_LARCH_TLS_DESC_PC_HI20 $t/le.objdump
not grep -Fw R_LARCH_TLS_DESC_PC_LO12 $t/le.objdump
not grep -Fw R_LARCH_TLS_DESC_LD $t/le.objdump
not grep -Fw R_LARCH_TLS_DESC_CALL $t/le.objdump

# TLS DESC

cat <<'EOF' | $CC -o $t/desc.o -c -xassembler -
.text
.globl bar
bar:
  .reloc ., R_LARCH_RELAX
  pcalau12i $a0, %desc_pc_hi20(tlsdesc_external)
  .reloc ., R_LARCH_RELAX
  addi.d $a0, $a0, %desc_pc_lo12(tlsdesc_external)
  .reloc ., R_LARCH_RELAX
  ld.d $ra, $a0, %desc_ld(tlsdesc_external)
  .reloc ., R_LARCH_RELAX
  jirl $ra, $ra, %desc_call(tlsdesc_external)

  .reloc ., R_LARCH_RELAX
  pcalau12i $a0, %desc_pc_hi20(tlsdesc_ie)
  .reloc ., R_LARCH_RELAX
  addi.d $a0, $a0, %desc_pc_lo12(tlsdesc_ie)
  .reloc ., R_LARCH_RELAX
  ld.d $ra, $a0, %desc_ld(tlsdesc_ie)
  .reloc ., R_LARCH_RELAX
  jirl $ra, $ra, %desc_call(tlsdesc_ie)

  ret
EOF

$CC -B. -shared -o $t/desc.so $t/desc.o -nostdlib -Wl,--emit-relocs,--relax
$OBJDUMP -dr $t/desc.so > $t/desc.objdump
grep -E 'R_LARCH_TLS_DESC_PCREL20_S2[[:space:]]+tlsdesc_external' \
  $t/desc.objdump
grep -E 'R_LARCH_TLS_DESC_LD[[:space:]]+tlsdesc_external' \
  $t/desc.objdump
grep -E 'R_LARCH_TLS_DESC_CALL[[:space:]]+tlsdesc_external' \
  $t/desc.objdump
not grep -Fw R_LARCH_TLS_DESC_PC_HI20 $t/desc.objdump
not grep -Fw R_LARCH_TLS_DESC_PC_LO12 $t/desc.objdump

# TLS IE

cat <<'EOF' | $CC -o $t/ext.o -c -xassembler -
.section .tbss,"awT",@nobits
.globl tlsdesc_ie
.p2align 2
tlsdesc_ie:
  .word 0
EOF

cat <<'EOF' | $CC -o $t/local.o -c -xassembler -
.section .tbss,"awT",@nobits
.globl tlsdesc_external
.p2align 2
tlsdesc_external:
  .word 0
EOF

cat <<'EOF' | $CC -o $t/desc-main.o -c -xc -
extern void bar(void);

int main() {
  bar();
  return 0;
}
EOF

$CC -B. -shared -o $t/def.so $t/ext.o -nostdlib
$CC -B. -o $t/ie $t/desc.o $t/local.o $t/def.so $t/desc-main.o -Wl,--emit-relocs,--relax,-rpath=$t
$QEMU $t/ie
$OBJDUMP -dr $t/ie > $t/ie.objdump
grep -E 'R_LARCH_TLS_IE_PC_HI20[[:space:]]+tlsdesc_ie' $t/ie.objdump
grep -E 'R_LARCH_TLS_IE_PC_LO12[[:space:]]+tlsdesc_ie' $t/ie.objdump
not grep -Fw R_LARCH_TLS_DESC_PC_HI20 $t/ie.objdump
not grep -Fw R_LARCH_TLS_DESC_PC_LO12 $t/ie.objdump
not grep -Fw R_LARCH_TLS_DESC_LD $t/ie.objdump
not grep -Fw R_LARCH_TLS_DESC_CALL $t/ie.objdump
