#!/bin/bash
source "$(dirname "$0")"/common.inc

# A label one past a section's last byte - an array's end - belongs to
# that section, even where another section starts at the same address.
# In an object with subsections the assembler gives such a label a
# zero-size subsection of its own; in a whole-section object (no
# .subsections_via_symbols) only its section says where it goes.
# ld-prime resolves both kinds, global or local, in a final link and
# in -r.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.data
.globl _arr
_arr: .quad 1, 2
arr_end:
.globl _arr_end
_arr_end:
.section __DATA,__ptrs
.p2align 3
.globl _p_end
_p_end: .quad arr_end
EOF

cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
extern char arr[], arr_end[], *p_end;
int main() { printf("%d %d\n", (int)(arr_end - arr), (int)(p_end - arr)); }
EOF

$CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o
$t/exe | grep -q '^16 16$'

$mold -arch $ARCH -r $t/a.o -o $t/r.o
nm -m $t/r.o > $t/nm
grep -Eq '\(__DATA,__data\) external .*_arr_end$' $t/nm
grep -Eq '\(__DATA,__data\) non-external .*arr_end$' $t/nm
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/r.o
$t/exe2 | grep -q '^16 16$'

# Likewise a label on an empty section, which starts where the next
# section does.
cat <<EOF2 | $CC -o $t/b.o -c -xassembler -
.section __DATA,__empty
emptylab:
.data
.globl _bdata
_bdata: .quad 3
EOF2
$CC --ld-path=$mold -o $t/exe3 $t/main.o $t/a.o $t/b.o
nm -m $t/exe3 > $t/nm3
grep -Eq '\(__DATA,__empty\) non-external .*emptylab$' $t/nm3
