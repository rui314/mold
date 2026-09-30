#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Relocations in a relocatable output may refer to any symbol, so -r
# keeps all symbols even with -x, -X or -s.

cat <<EOF | $CC -o $t/a.o -c -Wa,-L -xassembler -
.text
.globl foo
foo:
  nop
bar:
  nop
.Lbaz:
  nop
EOF

for opt in -x -X -s; do
  ./mold -r $opt -o $t/b.o $t/a.o
  readelf -sW $t/b.o > $t/log
  grep -w bar $t/log
  grep -F .Lbaz $t/log
done
