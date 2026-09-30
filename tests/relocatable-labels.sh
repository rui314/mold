#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime -r keeps every label an object defines, linker-private
# (l...) ones too: aliases of a global or of each other, labels of
# their own atoms and one on an empty section, those at one place
# listed by descending name. The ltmpN labels the arm64 assembler puts
# at each section's start go wherever another symbol names the place,
# and stay where none does.
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
nm -p $t/r.o | awk '{printf "%s ", $NF}' > $t/syms
if [ $ARCH = arm64 ]; then
  grep -qx 'l3 l2 l1 lafter_g lalone lab_e ldata ltmp3 _g ' $t/syms
else
  grep -qx 'l3 l2 l1 lafter_g lalone lab_e ldata _g ' $t/syms
fi
