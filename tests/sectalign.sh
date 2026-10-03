#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
__attribute__((section("__DATA,__blob"))) char blob[10] = "hi";
__attribute__((section("__DATA,__vec"), aligned(16))) char vec[16] = "hi";
int main() {}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-sectalign,__DATA,__blob,0x1000
otool -l $t/exe > $t/log
# The section is 2^12-aligned.
grep -A6 'sectname __blob' $t/log | grep 'align 2\^12'
addr=$(grep -A2 'sectname __blob' $t/log | awk '/addr/{print $2}')
[ $(( addr % 0x1000 )) -eq 0 ]

# An alignment that is not a power of two stands for the largest power
# of two that divides it. The first -sectalign for a section counts.
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-sectalign,__DATA,__blob,0x300 \
  -Wl,-sectalign,__DATA,__blob,0x1000 2> $t/log2
grep -q 'alignment for -sectalign __DATA __blob is not a power of two, using 0x100' $t/log2
otool -l $t/exe | grep -A6 'sectname __blob' | grep 'align 2\^8'

# -w silences that warning.
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-w -Wl,-sectalign,__DATA,__blob,6 2> $t/log2
not grep -q 'not a power of two' $t/log2

# -sectalign lowers an alignment too, with a warning.
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-sectalign,__DATA,__vec,4 2> $t/log2
grep -q -- '-sectalign reduces alignment of __DATA,__vec from 16 to 4' $t/log2
otool -l $t/exe | grep -A6 'sectname __vec' | grep 'align 2\^2'

# ld-prime settles each section's alignment in output order, the
# linker's own sections (__got here) too, and warns about each section
# in that order: of a -sectalign that lowers the alignment, then of one
# beyond the segment's maximum.
cat <<EOF | $CC -o $t/c.o -c -xassembler -
.data
.p2align 3
_d: .quad 1
.section __DATA,__big
.p2align 16
_big: .quad 2
.section __DATA,__mid
.p2align 4
_mid: .quad 3
.section __TEXT,__tbig
.p2align 15
_tbig: .quad 4
.subsections_via_symbols
EOF
cat <<EOF | $CC -o $t/d.o -c -xc -
#include <stdio.h>
int main() { puts("hi"); }
EOF
$CC --ld-path=$mold -o $t/exe3 $t/c.o $t/d.o -Wl,-sectalign,__DATA,__mid,2 \
  -Wl,-sectalign,__DATA,__big,0x8000 -Wl,-sectalign,__DATA_CONST,__got,1 2> $t/log3
$t/exe3 | grep -q hi
awk '/alignment/ { for (i = 1; i <= NF; i++) if ($i ~ /^__[A-Z_]+,__/) { print $i; break } }' \
  $t/log3 | tr '\n' ' ' > $t/order
[ "$(cat $t/order)" = '__TEXT,__tbig __DATA_CONST,__got __DATA,__big __DATA,__big __DATA,__mid ' ]
otool -l $t/exe3 | grep -A6 'sectname __got' | grep -q 'align 2\^0'
