#!/bin/bash
source "$(dirname "$0")"/common.inc

# A final image lists its local symbols - the non-external ones it
# keeps and the private externals it demotes - by address, across all
# objects. Names at one address are aliases of one atom, named by its
# highest-ranked symbol: a strong external, then a private external, a
# local, a weak definition, each rank by descending name. ld-prime
# lists the other names in that order before the atom's own name (a
# strong external's goes with the externals).
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.p2align 2
.private_extern _pa
_pa:
.private_extern _pz
_pz:
bb:
aa:
 ret
cc:
.globl _g
_g:
.private_extern _pb
_pb:
dd:
 ret
.globl _w
.weak_definition _w
_w:
ee:
ff:
 ret
.globl _main
_main: ret
.data
d1: .quad 1
.subsections_via_symbols
EOF
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.p2align 2
t2: ret
.private_extern _p2
_p2: ret
.data
d2: .quad 2
.subsections_via_symbols
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o
[ "$(nm -p $t/exe | awk '$2 ~ /^[a-z]$/ {printf "%s ", $3}')" = \
  '_pa bb aa _pz _pb dd cc ee ff t2 _p2 d1 d2 ' ]
