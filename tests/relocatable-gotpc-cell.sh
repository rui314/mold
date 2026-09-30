#!/bin/bash
source "$(dirname "$0")"/common.inc

# An arm64 4-byte pcrel GOT reference (ARM64_RELOC_POINTER_TO_GOT) -
# a CIE's personality pointer, an LSDA's type info, or a hand-written
# `.long _x@GOT - .` - carries no addend, and the assembler leaves
# arbitrary bytes in its field. ld-prime's -r output writes 4 there,
# in __eh_frame and in any other section.
[ "$ARCH" = arm64 ] || skip

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

# The 4 bytes under each type-7 (POINTER_TO_GOT) relocation of a section.
cells() {
  local off=$(otool -l $t/r.o | awk -v s=$2 '$1 == "sectname" { n = $2 }
    $1 == "offset" && n == s { print $2; exit }')
  otool -r $t/r.o | awk -v s="($1,$2)" '/^Relocation information/ { in_s = ($3 == s) }
    in_s && $5 == 7 { print $1 }' | while read addr; do
    xxd -s $((off + 0x$addr)) -l 4 -p $t/r.o
  done | sort -u
}
[ "$(cells __TEXT __eh_frame)" = 04000000 ]
[ "$(cells __TEXT __gcc_except_tab)" = 04000000 ]
[ "$(cells __DATA __gotrefs)" = 04000000 ]
