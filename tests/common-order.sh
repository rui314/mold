#!/bin/bash
source "$(dirname "$0")"/common.inc

# Common symbols are laid out by the objects' symbol tables, each where
# the tentative definition that wins - the first of the largest - is.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.globl _main
_main: ret
.comm _c_d, 8
.comm _c_b, 8
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.comm _c_c, 8
.comm _c_a, 8
.comm _c_b, 16
.comm _c_e, 4
EOF

$CC --ld-path=$mold -o $t/exe1 $t/a.o $t/b.o
nm -n $t/exe1 | awk '/ _c_/ { print $3 }' | tr '\n' ' ' > $t/order1
grep -q '^_c_d _c_a _c_b _c_c _c_e $' $t/order1

$CC --ld-path=$mold -o $t/exe2 $t/b.o $t/a.o
nm -n $t/exe2 | awk '/ _c_/ { print $3 }' | tr '\n' ' ' > $t/order2
grep -q '^_c_a _c_b _c_c _c_e _c_d $' $t/order2
