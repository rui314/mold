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
$RUN $t/exe | grep '^copy-B-align16 1 2 0$'
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/b16.o $t/a8.o
$RUN $t/exe2 | grep '^copy-B-align16 1 2 0$'
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
$RUN $t/exe3 | grep '^copy-C-whole-section 1 2 0$'
# _w stays weak there, as mold keeps STB_WEAK, so the image exports it
# weak (ld-prime makes the symbol that names a whole section non-weak;
# see whole-section-weak.sh).
if $mold -v 2>&1 | grep -q mold-macho; then
  dyld_info -exports $t/exe3 > $t/exports3
  grep ' _w \[weak-def\]$' $t/exports3
  nm -m $t/exe3 > $t/nm3
  grep '(__TEXT,__const) weak external _w$' $t/nm3
fi

# A subsection keeps its address modulo its section's alignment, and
# that is the alignment the copies compare by: a copy at 8 mod 16 of a
# 16-aligned section loses to one at 0 mod 16. Before alignment come
# the kinds of copy: one that can't be hidden beats a
# .weak_def_can_be_hidden one, and a global beats a private extern,
# however aligned.
copy() {
  cat <<EOF2 | $CC -o $t/$1.o -c -xassembler -
.data
.p2align 4
.globl _pad_$1
_pad_$1: .space $2
.globl _v
$3
_v: .quad $4
.subsections_via_symbols
EOF2
}
cat <<EOF2 | $CC -o $t/vmain.o -c -xc -
#include <stdio.h>
extern long v;
int main() { printf("%ld\n", v); }
EOF2
pick() {
  $CC --ld-path=$mold -o $t/exe-$1-$2 $t/vmain.o $t/$1.o $t/$2.o
  $RUN $t/exe-$1-$2
}
copy w8 8 '.weak_definition _v' 1
copy w16 16 '.weak_definition _v' 2
copy hidden16 16 '.private_extern _v
.weak_definition _v' 3
copy hidable16 16 '.weak_def_can_be_hidden _v' 4
pick w8 w16 | grep '^2$'
pick w16 w8 | grep '^2$'
pick hidden16 w8 | grep '^1$'
pick hidable16 w8 | grep '^1$'
pick w8 hidable16 | grep '^1$'
