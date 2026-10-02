#!/bin/bash
source "$(dirname "$0")"/common.inc

# A final image lists the non-external symbols each object keeps and
# the private externals it demotes, with N_PEXT, each at its address;
# names at one address are aliases of one subsection. (ld-prime lists
# them by address across all objects, the names at one address by
# rank, a subsection's own name last.)
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
t2: nop
 ret
.private_extern _p2
_p2: nop
 nop
 ret
.data
d2: .quad 2
.subsections_via_symbols
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o
nm -p $t/exe | awk '$2 ~ /^[a-z]$/' > $t/locals
[ "$(awk '{ print $3 }' $t/locals | sort | tr '\n' ' ')" = \
  '_p2 _pa _pb _pz aa bb cc d1 d2 dd ee ff t2 ' ]
nm -m $t/exe > $t/nm
for s in _pa _pz _pb _p2; do
  grep -q "non-external (was a private external) $s\$" $t/nm
done

# Each name at its address: those of one subsection at one, the
# subsections at the addresses their globals have.
addr() { awk -v s=$1 '$NF == s { print $1 }' $t/nm; }
[ "$(addr _pa)" = "$(addr aa)" ]
[ "$(addr _pz)" = "$(addr bb)" ]
[ "$(addr _g)" = "$(addr cc)" ]
[ "$(addr _g)" = "$(addr _pb)" ]
[ "$(addr _g)" = "$(addr dd)" ]
[ "$(addr _w)" = "$(addr ee)" ]
[ "$(addr _w)" = "$(addr ff)" ]
