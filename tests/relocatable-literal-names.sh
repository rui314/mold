#!/bin/bash
source "$(dirname "$0")"/common.inc
[ $ARCH = arm64 ] || skip

# arm64 relocations must name what they refer to. A -r output keeps the
# literal records and their labels as they are, for the final link to
# merge, and the relocations keep naming the labels. (ld-prime merges
# the literals and names each record itself, l<nnn> or LC<n>.)
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _f
.p2align 2
_f:
 adrp x0, lCPI0_1@PAGE
 ldr d0, [x0, lCPI0_1@PAGEOFF]
 ret
.section __TEXT,__literal4,4byte_literals
.p2align 2
lA: .long 1
lB: .long 1
lC: .long 2
.section __TEXT,__literal8,8byte_literals
.p2align 3
lCPI0_0: .double 5.5
lCPI0_1: .double 6.5
.section __TEXT,__literal16,16byte_literals
.p2align 4
lD: .quad 1, 2
.section __TEXT,__cstring,cstring_literals
lstr: .asciz "hello"
.subsections_via_symbols
EOF
$mold -arch $ARCH -r $t/a.o -o $t/r.o
nm -pm $t/r.o > $t/nm
for s in lA lB lC lCPI0_0 lCPI0_1 lD lstr; do
  grep -q " non-external $s\$" $t/nm
done
otool -rv $t/r.o > $t/relocs
grep -q 'PAGE21  False     lCPI0_1$' $t/relocs
grep -q 'PAGOF12 False     lCPI0_1$' $t/relocs

cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
double f(void);
float g(void);
int main() { printf("%g %g\n", f(), g()); }
EOF

# Two labels of one record: the relocations keep naming each.
cat <<EOF2 | $CC -o $t/b.o -c -xassembler -
.text
.globl _g
.p2align 2
_g:
 adrp x0, la@PAGE
 ldr s0, [x0, la@PAGEOFF]
 adrp x0, lb@PAGE
 ldr s1, [x0, lb@PAGEOFF]
 fadd s0, s0, s1
 ret
.section __TEXT,__literal4,4byte_literals
.p2align 2
la:
lb: .float 7.5
.float 8.5
.section __TEXT,__literal8,8byte_literals
.p2align 3
.quad 9
EOF2
$mold -arch $ARCH -r $t/b.o -o $t/r2.o
otool -rv $t/r2.o > $t/relocs2
[ "$(grep -c 'False     la$' $t/relocs2)" = 2 ]
[ "$(grep -c 'False     lb$' $t/relocs2)" = 2 ]

# Programs linked from the outputs, with either linker, read the same
# values as from the objects.
$CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o $t/b.o
$t/exe | grep -q '^6.5 15$'
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/r.o $t/r2.o
$t/exe2 | grep -q '^6.5 15$'
$CC -o $t/exe3 $t/main.o $t/r.o $t/r2.o
$t/exe3 | grep -q '^6.5 15$'
