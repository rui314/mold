#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -r output keeps __objc_clsrolist, the list of class_ro_t records,
# with each pointer's relocation naming its target as the input did.
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
otool -rv $t/r.o > $t/log
grep -A5 __objc_clsrolist $t/log | awk '$1 ~ /^0000/ { print $1, $NF }' | sort > $t/relocs
[ "$(tr '\n' ' ' < $t/relocs)" = "00000000 _a 00000008 _b 00000010 _c " ]
