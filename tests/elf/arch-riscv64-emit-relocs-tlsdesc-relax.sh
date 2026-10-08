#!/usr/bin/env bash
. $(dirname $0)/common.inc

# mold relaxes a TLSDESC sequence into a local-exec (LE) or initial-exec (IE)
# sequence if possible. With --emit-relocs, the relocations of the relaxed
# sequence are rewritten to R_RISCV_NONE, which must not prevent the TLSDESC
# LO12 relocations from finding their paired TLSDESC_HI20.

# GNU as may not support TLSDESC for RISC-V, so we use clang instead.
echo 'auipc a0, %tlsdesc_hi(foo)' |
  clang ${TRIPLE:+--target=$TRIPLE} -c -o /dev/null -xassembler - || skip

cat <<'EOF' | $CC -o $t/a.o -c -xc - -fPIC
_Thread_local char foo[4] = "foo";
_Thread_local char padding[100000] = "pad";
_Thread_local char bar[4] = "bar";
EOF

cat <<'EOF' | clang ${TRIPLE:+--target=$TRIPLE} -o $t/b.o -c -xassembler -
.globl get_foo, get_bar
get_foo:
.Ltlsdesc_hi0:
  auipc a0, %tlsdesc_hi(foo)
  .reloc .-4, R_RISCV_RELAX, 0
  ld t0, %tlsdesc_load_lo(.Ltlsdesc_hi0)(a0)
  .reloc .-4, R_RISCV_RELAX, 0
  addi a0, a0, %tlsdesc_add_lo(.Ltlsdesc_hi0)
  .reloc .-4, R_RISCV_RELAX, 0
  jalr t0, 0(t0), %tlsdesc_call(.Ltlsdesc_hi0)
  .reloc .-4, R_RISCV_RELAX, 0
  add a0, a0, tp
  ret

get_bar:
.Ltlsdesc_hi1:
  auipc a0, %tlsdesc_hi(bar)
  .reloc .-4, R_RISCV_RELAX, 0
  ld t0, %tlsdesc_load_lo(.Ltlsdesc_hi1)(a0)
  .reloc .-4, R_RISCV_RELAX, 0
  addi a0, a0, %tlsdesc_add_lo(.Ltlsdesc_hi1)
  .reloc .-4, R_RISCV_RELAX, 0
  jalr t0, 0(t0), %tlsdesc_call(.Ltlsdesc_hi1)
  .reloc .-4, R_RISCV_RELAX, 0
  add a0, a0, tp
  ret
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>
char *get_foo();
char *get_bar();
int main() { printf("%s %s\n", get_foo(), get_bar()); }
EOF

# Local TLS in an executable: TLSDESC => LE
$CC -B. -o $t/exe1 $t/a.o $t/b.o $t/c.o -Wl,--emit-relocs
$QEMU $t/exe1 | grep -F 'foo bar'

# TLSDESC_HI20s, the only relocations referring to foo and bar, are emitted
# as R_RISCV_NONE.
$OBJDUMP -dr $t/exe1 > $t/exe1.objdump
grep -E $'R_RISCV_NONE[ \t]+foo' $t/exe1.objdump
grep -E $'R_RISCV_NONE[ \t]+bar' $t/exe1.objdump

# TLS defined in a shared library: TLSDESC => IE
$CC -B. -shared -o $t/d.so $t/a.o
$CC -B. -o $t/exe2 $t/b.o $t/c.o $t/d.so -Wl,--emit-relocs -Wl,-rpath=$t
$QEMU $t/exe2 | grep -F 'foo bar'

$OBJDUMP -dr $t/exe2 > $t/exe2.objdump
grep -E $'R_RISCV_NONE[ \t]+foo' $t/exe2.objdump
grep -E $'R_RISCV_NONE[ \t]+bar' $t/exe2.objdump
