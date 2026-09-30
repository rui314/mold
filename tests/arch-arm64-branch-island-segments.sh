#!/bin/bash
source "$(dirname "$0")"/common.inc

[ "$ARCH" = arm64 ] || skip

# ld-prime's branch islands serve only the code of __TEXT, whatever the
# output type: a -static executable gets them like any other image, but
# a branch in the code of another segment - a kext's __TEXT_EXEC among
# them - gets none and is a fixup error if it is out of reach. Nor does
# an atom there get the warning about atoms too large for the island
# clusters.
objs() {
  cat <<EOF | $CC -o $t/main.o -c -xassembler -
.subsections_via_symbols
.section $1,regular,pure_instructions
.globl _main
.p2align 2
_main:
  bl _far
  ret
EOF

  cat <<EOF | $CC -o $t/pad.o -c -xassembler -
.subsections_via_symbols
.section $1,regular,pure_instructions
_pad1:
  .long 1
  .space $2 - 4
_pad2:
  .long 2
  .space $3 - 4
EOF

  cat <<EOF | $CC -o $t/far.o -c -xassembler -
.subsections_via_symbols
.section $1,regular,pure_instructions
.globl _far
.p2align 2
_far:
  ret
EOF
}

objs __TEXT,__text 0x4400000 0x4400000
$mold -arch $ARCH -static -e _main -o $t/exe1 $t/main.o $t/pad.o $t/far.o
nm $t/exe1 > $t/syms1
grep -q ' _far\.island$' $t/syms1
rm -f $t/exe1

not $mold -arch $ARCH -kext -o $t/kext $t/main.o $t/pad.o $t/far.o 2> $t/log2
grep -q "fixup error (kind=arm64_b26) at '_main' from main.o, B/BL out of range" $t/log2

objs __FOO,__text 0x8400000 0x4000
not $CC --ld-path=$mold -o $t/exe3 $t/main.o $t/pad.o $t/far.o 2> $t/log3
grep -q "fixup error (kind=arm64_b26) at '_main' from main.o, B/BL out of range" $t/log3
not grep -q 'branch island clusters' $t/log3
rm -f $t/pad.o
