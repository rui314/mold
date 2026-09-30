#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<'EOF' | $CC -o $t/a.o -c -xassembler -
.globl f1, f2, f1_mid, f2_mid

.section .text.f1,"ax",@progbits
.p2align 4
.type f1, @function
f1:
  move $t1, $ra
  .reloc ., R_LARCH_CALL36, g
  .reloc ., R_LARCH_RELAX
  pcaddu18i $ra, 0
  jirl $ra, $ra, 0
  move $ra, $t1
f1_mid:
  addi.w $a0, $a0, 1
  ret
.size f1, .-f1

.section .text.f2,"ax",@progbits
.p2align 4
.type f2, @function
f2:
  move $t1, $ra
  .reloc ., R_LARCH_CALL36, g
  .reloc ., R_LARCH_RELAX
  pcaddu18i $ra, 0
  jirl $ra, $ra, 0
  move $ra, $t1
f2_mid:
  addi.w $a0, $a0, 1
  ret
.size f2, .-f2

.section .text.g,"ax",@progbits
g:
  li.w $a0, 3
  ret
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>
int f1(), f2(), f1_mid(int), f2_mid(int);
int main() {
  printf("%d %d %d %d\n", (long)f1 == (long)f2, (long)f1_mid == (long)f2_mid,
         f2(), f2_mid(5));
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,--icf=all -Wl,--export-dynamic

readelf --dyn-syms $t/exe | grep -E ' 20 FUNC .* f1$'
readelf --dyn-syms $t/exe | grep -E ' 20 FUNC .* f2$'

$QEMU $t/exe | grep '^1 1 4 6$'
