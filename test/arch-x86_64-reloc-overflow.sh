#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -x assembler -
  .globl foo
  .data
foo:
  .short foo
EOF

not ./mold -e foo -o $t/exe $t/a.o 2> $t/log
grep -F 'relocation R_X86_64_16 against foo out of range' $t/log
