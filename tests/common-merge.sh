#!/bin/bash
source "$(dirname "$0")"/common.inc

# Tentative definitions of one symbol merge into the largest, which
# wins whole - its size, its alignment, whatever the others', and
# whether it is a private external - and of those of one size the
# first does (ld-prime).
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.private_extern _p
.comm _p,8,3
.comm _e,8,3
.comm _t,8,2
.text
.globl _f
_f: ret
.subsections_via_symbols
EOF
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.comm _p,4,2
.private_extern _e
.comm _e,4,2
.private_extern _t
.comm _t,8,4
.text
.globl _g
_g: ret
.subsections_via_symbols
EOF

for order in "$t/a.o $t/b.o" "$t/b.o $t/a.o"; do
  $CC --ld-path=$mold -shared -o $t/c.dylib $order
  nm -m $t/c.dylib > $t/log
  grep -q 'non-external (was a private external) _p$' $t/log
  grep -q ' external _e$' $t/log
done

$CC --ld-path=$mold -shared -o $t/c.dylib $t/a.o $t/b.o
nm -m $t/c.dylib | grep -q ' external _t$'
$CC --ld-path=$mold -shared -o $t/c.dylib $t/b.o $t/a.o
nm -m $t/c.dylib | grep -q 'non-external (was a private external) _t$'

# The 16 bytes aligned to 4 win over the 8 aligned to 32.
cat <<EOF | $CC -o $t/c.o -c -xassembler -
.comm _pad,1,0
.comm _v,16,2
.text
.globl _h
_h: ret
.subsections_via_symbols
EOF
echo '.comm _v,8,5' | $CC -o $t/d.o -c -xassembler -
for order in "$t/c.o $t/d.o" "$t/d.o $t/c.o"; do
  $CC --ld-path=$mold -shared -o $t/e.dylib $order
  nm $t/e.dylib > $t/log2
  pad=$(awk '/ _pad$/ { print $1 }' $t/log2)
  v=$(awk '/ _v$/ { print $1 }' $t/log2)
  [ $((0x$v - 0x$pad)) = 4 ]
done
