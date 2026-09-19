#!/bin/bash
source "$(dirname "$0")"/common.inc

[ $ARCH = arm64 ] || skip

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
.p2align 2
_get_addr:
L1: adrp x0, _value@PAGE
L2: add  x0, x0, _value@PAGEOFF
  ret
.loh AdrpAdd L1, L2

_get_val:
L3: adrp x8, _value@PAGE
L4: add  x8, x8, _value@PAGEOFF
L5: ldr  x0, [x8]
  ret
.loh AdrpAddLdr L3, L4, L5

_main:
  stp x29, x30, [sp, #-16]!
  bl _get_val
  cmp x0, #42
  b.ne 1f
  bl _get_addr
  ldr x0, [x0]
  cmp x0, #42
  b.ne 1f
  mov w0, #0
  ldp x29, x30, [sp], #16
  ret
1:
  mov w0, #1
  ldp x29, x30, [sp], #16
  ret

.data
.globl _value
.p2align 3
_value:
  .quad 42
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o
$t/exe
objdump -d $t/exe > $t/dis
# AdrpAdd became nop + adr; AdrpAddLdr became nops + a literal ldr.
sed -n '/<_get_addr>:/,/ret/p' $t/dis > $t/f1
grep -q 'nop' $t/f1 && grep -q ' adr ' $t/f1 && ! grep -q adrp $t/f1
sed -n '/<_get_val>:/,/ret/p' $t/dis > $t/f2
[ "$(grep -c nop $t/f2)" = 2 ] && grep -qE 'ldr.*x0, 0x' $t/f2

# -ignore_optimization_hints keeps the compiler's sequences.
$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-ignore_optimization_hints
$t/exe2
objdump -d $t/exe2 > $t/dis2
sed -n '/<_get_addr>:/,/ret/p' $t/dis2 | grep -q adrp
