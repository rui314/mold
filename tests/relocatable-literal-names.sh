#!/bin/bash
source "$(dirname "$0")"/common.inc
[ $ARCH = arm64 ] || skip

# arm64 ld-prime -r names each record of the fixed-size literal
# sections l<nnn>, a private external, on the counter it names
# cstrings LC<n> with, and points relocations at the new names; the
# records' own labels vanish. Identical literals are merged first.
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
lCPI0_0: .quad 5
lCPI0_1: .quad 6
.section __TEXT,__literal16,16byte_literals
.p2align 4
lD: .quad 1, 2
.section __TEXT,__cstring,cstring_literals
lstr: .asciz "hello"
.subsections_via_symbols
EOF
$mold -arch $ARCH -r $t/a.o -o $t/r.o
nm -pm $t/r.o > $t/nm
awk '{printf "%s ", $NF}' $t/nm > $t/syms
grep -qx 'l001 l002 l003 l004 l005 LC6 _f ' $t/syms
grep -q '(__TEXT,__literal8) non-external (was a private external) l004$' $t/nm
otool -rv $t/r.o > $t/relocs
grep -q 'PAGE21  False     l004$' $t/relocs
grep -q 'PAGOF12 False     l004$' $t/relocs

# A record's greatest label is the name its l<nnn> replaces; any other
# label of the record stays, a plain local alias that relocations keep
# naming - the assembler's ltmpN at a section's start too, in an object
# without subsections, unless it is the record's only label.
cat <<EOF2 | $CC -o $t/b.o -c -xassembler -
.text
.globl _g
.p2align 2
_g:
 adrp x0, la@PAGE
 ldr s0, [x0, la@PAGEOFF]
 adrp x0, lb@PAGE
 ldr s0, [x0, lb@PAGEOFF]
 ret
.section __TEXT,__literal4,4byte_literals
.p2align 2
la:
lb: .long 7
.long 8
.section __TEXT,__literal8,8byte_literals
.p2align 3
.quad 9
EOF2
$mold -arch $ARCH -r $t/b.o -o $t/r2.o
nm -pm $t/r2.o > $t/nm2
awk '{printf "%s ", $NF}' $t/nm2 > $t/syms2
grep -qx 'ltmp0 l001 la ltmp1 l002 l003 _g ' $t/syms2
grep -q '(__TEXT,__literal4) non-external la$' $t/nm2
otool -rv $t/r2.o > $t/relocs2
[ "$(grep -c 'False     la$' $t/relocs2)" = 2 ]
[ "$(grep -c 'False     l001$' $t/relocs2)" = 2 ]
