#!/bin/bash
source "$(dirname "$0")"/common.inc

# Among weak definitions ld64 keeps the copy with the greatest
# alignment, whichever comes first; at equal alignment the first wins.
# Swift metadata records come 8-aligned from one object and 16-aligned
# from another, and ld-prime's layout follows the 16-aligned copy.
cat <<EOF2 | $CC -o $t/a8.o -c -xassembler -
.section __TEXT,__const
.p2align 3
.globl _w
.weak_definition _w
_w: .asciz "copy-A-align8"
.p2align 3
.globl _pad1
_pad1: .quad 1
.subsections_via_symbols
EOF2
cat <<EOF2 | $CC -o $t/b16.o -c -xassembler -
.section __TEXT,__const
.p2align 4
.globl _w
.weak_definition _w
_w: .asciz "copy-B-align16"
.p2align 4
.globl _pad2
_pad2: .quad 2
.subsections_via_symbols
EOF2
cat <<EOF2 | $CC -o $t/main.o -c -xc -
#include <stdio.h>
#include <string.h>
extern const char w[]; extern const long pad1, pad2;
int main() { printf("%s %ld %ld %d\n", w, pad1, pad2, (int)((unsigned long)w % 16)); }
EOF2
$CC --ld-path=$mold -o $t/exe $t/main.o $t/a8.o $t/b16.o
$t/exe | grep '^copy-B-align16 1 2 0$'
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/b16.o $t/a8.o
$t/exe2 | grep '^copy-B-align16 1 2 0$'
$mold -r -arch $ARCH -o $t/r.o $t/a8.o $t/b16.o
nm -n $t/r.o | grep ' S ' | awk '{print $3}' | tr '\n' ' ' > $t/order
grep -q '^_pad1 _w _pad2 $' $t/order

# Without .subsections_via_symbols a section is one subsection; the
# losing copy's section also holds _pad2, whose bytes must survive.
cat <<EOF2 | $CC -o $t/c.o -c -xassembler -
.section __TEXT,__const
.p2align 4
.globl _w
.weak_definition _w
_w: .asciz "copy-C-whole-section"
.p2align 4
.globl _pad2
_pad2: .quad 2
EOF2
$CC --ld-path=$mold -o $t/exe3 $t/main.o $t/a8.o $t/c.o
$t/exe3 | grep '^copy-C-whole-section 1 2 0$'
# That whole-section atom is not weak in ld64's eyes: the symbol at
# its start loses its weak flag, so the image exports a plain _w and
# claims neither WEAK_DEFINES nor BINDS_TO_WEAK.
dyld_info -exports $t/exe3 > $t/exports3
grep ' _w$' $t/exports3
nm -m $t/exe3 > $t/nm3
grep '(__TEXT,__const) external _w$' $t/nm3
otool -h $t/exe3 > $t/hdr3
grep ' 0x00200085$' $t/hdr3
