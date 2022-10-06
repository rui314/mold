#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -x assembler -
  .text
  .globl foo
  .hidden foo
foo:
  nop
  .globl bar
bar:
  nop
  .globl _start
_start:
  nop
EOF

$CC -shared -fPIC -o $t/b.so -xc /dev/null
./mold -o $t/exe $t/a.o $t/b.so --export-dynamic

readelf --dyn-syms $t/exe > $t/log
grep -Eq 'NOTYPE  GLOBAL DEFAULT    [0-9]+ bar' $t/log
grep -Eq 'NOTYPE  GLOBAL DEFAULT    [0-9]+ _start' $t/log
