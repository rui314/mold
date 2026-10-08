#!/bin/bash
source "$(dirname "$0")"/common.inc

# Common symbols become zero-fill definitions in __DATA,__common, each
# as large as its largest tentative definition, whichever object comes
# first. (ld-prime lays them out by the objects' symbol tables, each
# where the first of the largest tentative definitions is; mold where
# the first declaration is.)
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

# _c_b takes 16 bytes: the next symbol is at least that far on.
check() {
  nm -m $1 | grep -c '(__DATA,__common) external _c_' > $t/count
  [ "$(cat $t/count)" = 5 ]
  nm -n $1 | grep ' _c_' | cut -d' ' -f1,3 > $t/syms
  local b=$(grep -n ' _c_b$' $t/syms | cut -d: -f1)
  local next=$(sed -n "$((b + 1))p" $t/syms | cut -d' ' -f1)
  local addr=$(sed -n "${b}p" $t/syms | cut -d' ' -f1)
  [ -z "$next" ] || [ $((0x$next - 0x$addr)) -ge 16 ]
}

$CC --ld-path=$mold -o $t/exe1 $t/a.o $t/b.o
check $t/exe1
$CC --ld-path=$mold -o $t/exe2 $t/b.o $t/a.o
check $t/exe2
