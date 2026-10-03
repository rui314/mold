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

[ "$(unwind_lookup $t/exe _main _g _h | tr '\n' ' ')" = '0x0 0x2001000 0x2002000 ' ]
