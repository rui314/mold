#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -r output keeps every label an object defines, linker-private
# (l...) ones and the arm64 assembler's ltmpN labels too: aliases of a
# global or of each other, labels of their own subsections and one on
# an empty section. Each marks where a subsection starts, and a later
# link splits the output at them as it would the object. (ld-prime
# drops the ltmpN labels other symbols name.)
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.p2align 2
l1:
l2:
l3:
 nop
.globl _g
_g:
lafter_g:
 nop
lalone:
 nop
.section __DATA,__empty
lab_e:
.data
.p2align 3
ldata: .quad lalone
.section __TEXT,__const
.long 7
.subsections_via_symbols
EOF
$mold -arch $ARCH -r $t/a.o -o $t/r.o
nm -m $t/a.o | cut -c18- | sort > $t/syms-a
nm -m $t/r.o | cut -c18- | sort > $t/syms-r
diff $t/syms-a $t/syms-r

echo 'int main() { return 0; }' | $CC -o $t/main.o -c -xc -
$CC --ld-path=$mold -o $t/exe $t/main.o $t/r.o
$t/exe
