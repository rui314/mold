#!/bin/bash
source "$(dirname "$0")"/common.inc

# A CIE's augmentation string names what its augmentation data holds
# ('z' the data's length, 'R' the FDE pointer encoding, 'P' a
# personality, 'L' the LSDA encoding), and some letters are flags with
# no data: 'S' for a signal frame and, on AArch64, 'B' for return
# addresses signed with the pointer-authentication B key and 'G' for an
# MTE-tagged frame. ld64 reads CIEs with libunwind's parser, which
# ignores any letter it does not know, so all of these give _f the same
# unwind info as "zR".
#
# The records are written out, with the named labels older compilers
# gave them, as no assembler directive makes a CIE with these letters.
if [ $ARCH = arm64 ]; then
  ra=30 sp=31 size=8
else
  ra=16 sp=7 size=2
fi

# link <name> <augmentation> <FDE code length>
link() {
  cat <<EOF | $CC -o $t/$1.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  ret
.globl _f
.p2align 2
_f:
  nop
  ret
.section __TEXT,__eh_frame,coalesced,no_toc+strip_static_syms+live_support
EH_frame0:
  .long Lcie_end - Lcie_start
Lcie_start:
  .long 0
  .byte 1
  .asciz "$2"
  .byte 1, 0x78, $ra
  .byte 1, 0x10
  .byte 0x0c, $sp, 8
  .p2align 3
Lcie_end:
_f.eh:
  .long Lfde_end - Lfde_start
Lfde_start:
  .long Lfde_start - EH_frame0
  .quad _f - .
  .quad $3
  .byte 0
  .p2align 3
Lfde_end:
.subsections_via_symbols
EOF
  $CC --ld-path=$mold -o $t/$1 $t/$1.o
  otool -s __TEXT __unwind_info $t/$1 | sed 1d > $t/$1.unwind
}

link zR zR $size
for aug in zRS zRB zRG zRQ zRSBG; do
  link $aug $aug $size
  cmp $t/zR.unwind $t/$aug.unwind
done
