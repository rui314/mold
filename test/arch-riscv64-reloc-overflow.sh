#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -x assembler -
.section .foo, "ax"
foo:
  call bar
EOF

cat <<EOF | $CC -o $t/b.o -c -x assembler -
.globl bar
.section .bar, "ax"
bar:
  ret
EOF

cat <<EOF | $CC -o $t/c.o -c -xc -
int main() {}
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o $t/c.o -Wl,--section-start=.foo=0x10000 \
  -Wl,--section-start=.bar=0x8000f000

not $CC -B. -o $t/exe2 $t/a.o $t/b.o $t/c.o -Wl,--section-start=.foo=0x10000 \
  -Wl,--section-start=.bar=0x8000f800 |&
  grep -F 'relocation R_RISCV_CALL_PLT against bar out of range'
