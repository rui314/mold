#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime warns about a compact unwind record for a function in a
# section that is not code, but gives the function its __unwind_info
# entry all the same, at its address in __DATA. That segment is placed
# after __TEXT, where __unwind_info is, so the table is encoded again
# once it has its address, and here the final encoding is the larger.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  ret
.data
.p2align 2
.space 0x100
.globl _g
_g:
  .long 0
.globl _h
_h:
  .long 0
.section __LD,__compact_unwind,regular,debug
.p2align 3
.quad _g
.long 1
.long 0x02001000
.quad 0
.quad 0
.quad _h
.long 1
.long 0x02002000
.quad 0
.quad 0
.subsections_via_symbols
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o 2> $t/log
grep -q "symbols in __DATA,__data (.*/a.o) have unwind information, but it's not a code section" $t/log

objdump --unwind-info $t/exe > $t/unwind
nm $t/exe > $t/syms
# The entry of a function: its offset from the image base.
entry() {
  local addr=$(awk -v s=$1 '$3 == s { print $1 }' $t/syms)
  printf 'function offset=0x%08x, encoding.*=%s' $((0x$addr - 0x100000000)) $2
}
grep -q "$(entry _main 0x00000000)" $t/unwind
grep -q "$(entry _g 0x02001000)" $t/unwind
grep -q "$(entry _h 0x02002000)" $t/unwind

# An object may draw both that warning and one of its subsections
# aligned less than the pointers they hold.
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  ret
.data
.globl _q0, _q1
_q0: .byte 0
_q1: .quad _main
.section __DATA,__foo
.p2align 2
.globl _g
_g:
  .long 0
.section __LD,__compact_unwind,regular,debug
.p2align 3
.quad _g
.long 1
.long 0x02001000
.quad 0
.quad 0
.subsections_via_symbols
EOF
$CC --ld-path=$mold -o $t/exe2 $t/b.o -Wl,-no_fixup_chains 2> $t/log2
grep -q "alignment (1) of atom '_q1'" $t/log2
grep -q 'symbols in __DATA,__foo (.*/b.o) have unwind information' $t/log2
