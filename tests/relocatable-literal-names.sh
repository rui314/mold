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
