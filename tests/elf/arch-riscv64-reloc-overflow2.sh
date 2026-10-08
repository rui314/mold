#!/usr/bin/env bash
. $(dirname $0)/common.inc

# GNU as doesn't support R_RISCV_PLT32 or R_RISCV_GOT32_PCREL.
cat <<EOF | clang ${TRIPLE:+--target=$TRIPLE} -o $t/a.o -c -x assembler - || skip
.section .foo, "aw"
.reloc ., R_RISCV_32_PCREL, bar
.reloc .+4, R_RISCV_PLT32, bar
.reloc .+8, R_RISCV_GOT32_PCREL, bar
.zero 12
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
void bar() {}
void _start() {}
EOF

$CC -B. -nostdlib -o $t/exe1 $t/a.o $t/b.o -Wl,--section-start=.foo=0x10000000

not $CC -B. -nostdlib -o $t/exe2 $t/a.o $t/b.o \
  -Wl,--section-start=.foo=0x100000000 2> $t/log
grep -F 'relocation R_RISCV_32_PCREL against bar out of range' $t/log
grep -F 'relocation R_RISCV_PLT32 against bar out of range' $t/log
grep -F 'relocation R_RISCV_GOT32_PCREL against bar out of range' $t/log
