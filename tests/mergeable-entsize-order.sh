#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Mergeable sections of the same name but different entry sizes are
# merged into separate output sections, which appear in input order. a.o
# has many other mergeable sections so that b.o's .foo is likely to be
# created first when input files are processed in parallel.
for i in $(seq 1 100); do
  echo ".section .foo$i,\"aM\",%progbits,1"
  echo ".byte $i"
done > $t/a.s

cat <<EOF >> $t/a.s
.section .foo,"aM",%progbits,8
.quad 1
EOF

$CC -c -o $t/a.o -xassembler $t/a.s

cat <<EOF | $CC -c -o $t/b.o -xassembler -
.section .foo,"aM",%progbits,4
.long 2
EOF

for i in 1 2 3; do
  ./mold -shared -o $t/c.so $t/a.o $t/b.o
  readelf -SW $t/c.so | grep -F ' .foo ' | head -1 | grep -w 08
done

./mold -shared -o $t/d.so $t/b.o $t/a.o
readelf -SW $t/d.so | grep -F ' .foo ' | head -1 | grep -w 04
