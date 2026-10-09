#!/bin/bash
source "$(dirname "$0")"/common.inc

# -no_dwarf_unwind leaves the inputs' __eh_frame out, of a final image
# and of a -r output. A function compact unwind can't describe keeps
# its DWARF-mode entry in __unwind_info all the same, pointing at offset
# 0 of the __eh_frame there isn't; a -r output keeps its record in
# __compact_unwind.
if [ $ARCH = arm64 ]; then
  cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  .cfi_startproc
  stp x29, x30, [sp, #-16]!
  .cfi_def_cfa_offset 16
  .cfi_offset w30, -8
  .cfi_offset w29, -16
  .cfi_escape 0x2e, 0x00
  mov x0, #0
  ldp x29, x30, [sp], #16
  ret
  .cfi_endproc
EOF
  dwarf=0x03000000
else
  cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
_main:
  .cfi_startproc
  pushq %rbp
  .cfi_def_cfa_offset 16
  .cfi_offset %rbp, -16
  .cfi_escape 0x2e, 0x00
  movq %rsp, %rbp
  xorl %eax, %eax
  popq %rbp
  retq
  .cfi_endproc
EOF
  dwarf=0x04000000
fi

$CC --ld-path=$mold -o $t/exe1 $t/a.o
otool -l $t/exe1 | grep -q 'sectname __eh_frame'
objdump --unwind-info $t/exe1 > $t/log1
not grep -q "encoding\[0\]: $dwarf\$" $t/log1

$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-no_dwarf_unwind
$RUN $t/exe2
otool -l $t/exe2 > $t/lc2
not grep -q 'sectname __eh_frame' $t/lc2
grep -q 'sectname __unwind_info' $t/lc2
objdump --unwind-info $t/exe2 | grep -q "encoding\[0\]: $dwarf\$"

$CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-no_dwarf_unwind -Wl,-no_compact_unwind
otool -l $t/exe3 > $t/lc3
not grep -q 'sectname __eh_frame\|sectname __unwind_info' $t/lc3

$mold -r -arch $ARCH -o $t/r.o $t/a.o -no_dwarf_unwind
otool -l $t/r.o > $t/lc4
not grep -q 'sectname __eh_frame' $t/lc4
grep -q 'sectname __compact_unwind' $t/lc4
