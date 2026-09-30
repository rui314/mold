#!/bin/bash
source "$(dirname "$0")"/common.inc

# __unwind_info's LSDA index lists a __compact_unwind record's LSDA only
# if the record's encoding has UNWIND_HAS_LSDA (0x40000000), as
# ld-prime does. The record keeps its LSDA all the same: its entry is
# not merged with the one before it, though they share an encoding.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  ret
.globl _f
_f:
  ret
.globl _g
_g:
  ret
.section __TEXT,__gcc_except_tab
_lsda1: .long 1
_lsda2: .long 2
.section __LD,__compact_unwind,regular,debug
.p2align 3
.quad _main
.long 1
.long 0x02001000
.quad 0
.quad 0
.quad _f
.long 1
.long 0x02001000
.quad 0
.quad _lsda1
.quad _g
.long 1
.long 0x42001000
.quad 0
.quad _lsda2
.subsections_via_symbols
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o
objdump --unwind-info $t/exe > $t/unwind
nm $t/exe > $t/syms
# A symbol's offset from the image base.
off() {
  printf '0x%08x' $((0x$(awk -v s=$1 '$3 == s { print $1 }' $t/syms) - 0x100000000))
}
awk '/LSDA descriptors:/ { f = 1; next } /Second level indices:/ { f = 0 } f' \
  $t/unwind > $t/lsdas
[ $(wc -l < $t/lsdas) = 1 ]
grep -q "function offset=$(off _g), LSDA offset=$(off _lsda2)" $t/lsdas
grep -q "function offset=$(off _f), encoding.*=0x02001000" $t/unwind
