#!/bin/bash
source "$(dirname "$0")"/common.inc

# An arm64 4-byte pcrel GOT reference (ARM64_RELOC_POINTER_TO_GOT) -
# a CIE's personality pointer, an LSDA's type info, or a hand-written
# `.long _x@GOT - .` - carries no addend, and the assembler leaves
# arbitrary bytes in its field. ld-prime's -r output writes 4 there,
# in __eh_frame and in any other section. An x86-64 one
# (X86_64_RELOC_GOT) holds its addend in the field, which ld-prime
# keeps, but a CIE's personality cell it writes as 4 all the same.
if [ "$ARCH" = x86_64 ]; then
  cat <<EOF | $CC -o $t/c.o -c -xassembler -
.text
.globl _f
_f:
  ret
.section __DATA,__gotrefs
.globl _r1
_r1: .long _ext1@GOTPCREL + 8
.section __TEXT,__eh_frame,coalesced,no_toc+strip_static_syms+live_support
EH_frame0:
  .long Lcie_end - Lcie_start
Lcie_start:
  .long 0
  .byte 1
  .asciz "zPR"
  .byte 1, 0x78, 16
  .byte 6, 0x9b
  .long _ext2@GOTPCREL + 8
  .byte 0x10
  .byte 0x0c, 7, 8
  .p2align 3
Lcie_end:
_f.eh:
  .long Lfde_end - Lfde_start
Lfde_start:
  .long Lfde_start - EH_frame0
  .quad _f - .
  .quad 1
  .byte 0
  .p2align 3
Lfde_end:
.subsections_via_symbols
EOF
  $mold -arch x86_64 -r $t/c.o -o $t/r.o
  type=4
else
  cat <<EOF | $CXX -o $t/a.o -c -xc++ - -fasynchronous-unwind-tables -femit-dwarf-unwind=always
#include <stdexcept>
int f(int x) {
  try { if (x) throw std::runtime_error("e"); } catch (std::exception &) { return 1; }
  return 0;
}
EOF
  cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __DATA,__gotrefs
.globl _r1
_r1: .long _ext1@GOT - .
.p2align 3
_r2: .quad 0
.long _ext2@GOT - .
.subsections_via_symbols
EOF
  $mold -arch arm64 -r $t/a.o $t/b.o -o $t/r.o
  type=7
fi

# The 4 bytes under each GOT relocation of a section.
cells() {
  local off=$(otool -l $t/r.o | awk -v s=$2 '$1 == "sectname" { n = $2 }
    $1 == "offset" && n == s { print $2; exit }')
  otool -r $t/r.o | awk -v s="($1,$2)" -v type=$type '/^Relocation information/ { in_s = ($3 == s) }
    in_s && $5 == type { print $1 }' | while read addr; do
    xxd -s $((off + 0x$addr)) -l 4 -p $t/r.o
  done | sort -u
}
[ "$(cells __TEXT __eh_frame)" = 04000000 ]
if [ "$ARCH" = x86_64 ]; then
  [ "$(cells __DATA __gotrefs)" = 08000000 ]
else
  [ "$(cells __TEXT __gcc_except_tab)" = 04000000 ]
  [ "$(cells __DATA __gotrefs)" = 04000000 ]
fi
