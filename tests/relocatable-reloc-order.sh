#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -r output lists each section's relocations subsection by
# subsection, and each subsection's by descending offset - the order
# compilers write them - with a SUBTRACTOR and its UNSIGNED, or an arm64
# ADDEND and the PAGE21 or PAGEOFF12 it goes with, kept together in
# order (ld-prime).
if [ $ARCH = arm64 ]; then
  cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _f
.p2align 2
_f:
  adrp x0, _g@PAGE
  add x0, x0, _g@PAGEOFF
  adrp x1, _h@PAGE+16
  add x1, x1, _h@PAGEOFF+16
  bl _ext1
  bl _ext2
  ret
.globl _f2
_f2:
  bl _ext3
  ret
.data
.globl _g
.p2align 3
_g: .quad _ext1
  .quad _ext2
  .quad _f - _g
  .quad _ext3
.globl _h
_h: .quad 0, 0, 0
.subsections_via_symbols
EOF
  text='14:2 10:2 0c:10 0c:4 08:10 08:3 04:4 00:3 1c:2 '
else
  cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _f
_f:
  leaq _g(%rip), %rax
  movq _h@GOTPCREL(%rip), %rcx
  callq _ext1
  callq _ext2
  retq
.globl _f2
_f2:
  callq _ext3
  retq
.data
.globl _g
.p2align 3
_g: .quad _ext1
  .quad _ext2
  .quad _f - _g
  .quad _ext3
.globl _h
_h: .quad 0, 0, 0
.subsections_via_symbols
EOF
  text='14:2 0f:2 0a:3 03:1 1a:2 '
fi
[ $ARCH = arm64 ] && sub=1 || sub=5

$mold -arch $ARCH -r $t/a.o -o $t/r.o
relocs() {
  otool -r $t/r.o | awk -v s="$1" '/^Relocation information/ { in_s = ($3 == s) }
    in_s && /^0/ { printf "%s:%s ", substr($1, 7), $5 }'
}
[ "$(relocs '(__TEXT,__text)')" = "$text" ]
[ "$(relocs '(__DATA,__data)')" = "18:0 10:$sub 10:0 08:0 00:0 " ]
