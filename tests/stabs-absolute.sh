#!/bin/bash
source "$(dirname "$0")"/common.inc

# An absolute symbol has no section, but ld-prime gives it a debug
# note all the same: an external (a global or a private external) an
# N_GSYM naming it, and a local an N_STSYM of its value, in no section.
if [ $ARCH = arm64 ]; then
  zero='mov w0, #0'; ret=ret
else
  zero='xorl %eax, %eax'; ret=retq
fi
cat <<EOF | $CC -o $t/a.o -c -g -xassembler -
  .text
  .globl _main
_main:
  $zero
  $ret
  .globl _abs_g
  _abs_g = 42
  .globl _abs_p
  .private_extern _abs_p
  _abs_p = 43
  _abs_l = 44
.subsections_via_symbols
EOF
$CC --ld-path=$mold -o $t/exe $t/a.o
$t/exe

nm -ap $t/exe > $t/stabs
grep -q '^0000000000000000 - 00 0000  GSYM _abs_g$' $t/stabs
grep -q '^0000000000000000 - 00 0000  GSYM _abs_p$' $t/stabs
grep -q '^000000000000002c - 00 0000 STSYM _abs_l$' $t/stabs
