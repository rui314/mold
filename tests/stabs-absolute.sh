#!/bin/bash
source "$(dirname "$0")"/common.inc

# An absolute symbol has no section and so no address for a debug note
# to give: it gets none, though it stays in the symbol table, and the
# unit's functions get theirs. (ld-prime notes an external one by name
# and a local one with its value.)
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
grep -q ' FUN _main$' $t/stabs
not grep -q 'SYM _abs_' $t/stabs
nm -m $t/exe > $t/nm
grep -q '(absolute) external _abs_g$' $t/nm
grep -q '(absolute) non-external (was a private external) _abs_p$' $t/nm
grep -q '(absolute) non-external _abs_l$' $t/nm
