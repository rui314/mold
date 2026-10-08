#!/usr/bin/env bash
. $(dirname $0)/common.inc

# GAS emits R_RISCV_ALIGN for an alignment directive in an executable
# section even if the section is NOBITS.

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section .foo, "ax", @nobits
.byte 0
.p2align 4
EOF

./mold -r -o $t/b.o $t/a.o
readelf -r $t/b.o | grep -F R_RISCV_ALIGN
