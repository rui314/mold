#!/bin/bash
source "$(dirname "$0")"/common.inc

[ "$ARCH" = arm64 ] || skip

# 140 MiB of code in 1 MiB subsections, twice: main, far and mid are
# each beyond the +-128 MiB reach of a bl from the one before, and
# mid's branch back to far can't reach the island main's bl goes
# through. (The subsections differ, or they would be folded into one.)
pad() {
  cat <<EOF | $CC -o $t/$1 -c -xassembler -
.subsections_via_symbols
.macro pad
_pad\@:
  .long $2 + \@
  .space 0x100000 - 4
.endm
.rept 140
pad
.endr
EOF
}
pad pad.o 0
pad pad2.o 1000

cat <<EOF | $CC -o $t/mid.o -c -xassembler -
.subsections_via_symbols
.globl _mid
.p2align 2
_mid:
  b _far
EOF

cat <<EOF | $CC -o $t/far.o -c -xassembler -
.subsections_via_symbols
.globl _far
.p2align 2
_far:
  mov w0, #42
  ret
EOF

cat <<EOF | $CC -o $t/main.o -c -g -xc -
#include <stdio.h>
int far();
int mid();
int main() { printf("%d %d\n", far(), mid()); }
EOF

$CC --ld-path=$mold -o $t/exe $t/main.o $t/pad.o $t/far.o $t/pad2.o $t/mid.o
$RUN $t/exe | grep '^42 42$'

# ld-prime names each branch island, a local symbol in the section of
# the branch, after its target: "<target>.island" for the target's
# first, "<target>.island<n>" for its n-th.
nm -m $t/exe > $t/syms
grep -q ' (__TEXT,__text) non-external _far.island$' $t/syms
grep -q ' (__TEXT,__text) non-external _far.island2$' $t/syms
grep -q ' (__TEXT,__text) non-external _printf.island$' $t/syms

# A branch that needs one goes to the island that bears the name.
objdump -d --disassemble-symbols=_main,_mid $t/exe > $t/text
grep -q 'bl.*<_far\.island>$' $t/text
grep -q 'bl.*<_printf\.island>$' $t/text
grep -q 'b.*<_far\.island2>$' $t/text

# Islands are functions but have no debug notes.
dyld_info -function_starts $t/exe | grep -q ' _far\.island$'
nm -ap $t/exe > $t/stabs
not grep -q 'FUN.*island' $t/stabs
