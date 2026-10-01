#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime splits __objc_clsrolist into one subsection per pointer, so
# a -r output lists its relocations by ascending pointer, though mold
# keeps the list whole (each subsection's relocations go by descending
# offset).
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __DATA,__objc_const
.p2align 3
.globl _a, _b, _c
_a: .space 72
_b: .space 72
_c: .space 72
.section __DATA,__objc_clsrolist,regular,no_dead_strip
.p2align 3
l_list:
  .quad _a
  .quad _b
  .quad _c
.subsections_via_symbols
EOF

$mold -r -arch $ARCH -o $t/r.o $t/a.o
otool -r $t/r.o > $t/log
grep -A5 __objc_clsrolist $t/log | awk '$1 ~ /^0000/ { print $1 }' > $t/addrs
[ "$(tr '\n' ' ' < $t/addrs)" = "00000000 00000008 00000010 " ]
