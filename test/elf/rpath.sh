#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -x assembler -
  .text
  .globl main
main:
  nop
EOF

$CC -B. -o $t/exe $t/a.o \
  -Wl,-rpath,/foo -Wl,-rpath,/bar -Wl,-R/no/such/directory -Wl,-R/

readelf --dynamic $t/exe | \
  grep -Fq 'Library runpath: [/foo:/bar:/no/such/directory:/]'
