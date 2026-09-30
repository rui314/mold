#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.option norvc
.globl foo
foo:
  nop
  .balign 16
  ret
EOF

readelf -rW $t/a.o | grep -E 'R_RISCV_ALIGN +c$'

./mold -r -o $t/b.o $t/a.o
readelf -rW $t/b.o | grep -E 'R_RISCV_ALIGN +c$'
