#!/bin/bash
source "$(dirname "$0")"/common.inc

[ "$ARCH" = arm64 ] || skip

# A mergeable dylib with more than 128 MiB of code reaches far
# functions through range-extension thunks, which its record has no
# entries for: a call's fixup names the function it calls, and a
# merging link, the linker or ld-prime, makes its own thunks.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.subsections_via_symbols
.globl _far_call
.p2align 2
_far_call:
  stp x29, x30, [sp, #-16]!
  bl _target
  add w0, w0, #1
  ldp x29, x30, [sp], #16
  ret
.macro pad
_pad\@:
  .long \@
  .space 0x100000 - 4
.endm
.rept 136
pad
.endr
.globl _target
.p2align 2
_target:
  mov w0, #41
  ret
EOF

cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
int far_call(void);
int main() { printf("%d\n", far_call()); }
EOF

$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/a.o -Wl,-make_mergeable \
  -Wl,-install_name,@rpath/libfoo.dylib
otool -tv $t/libfoo.dylib | grep -A2 '^_far_call:' > $t/disasm
grep -q 'bl.*_target\.island' $t/disasm

$CC --ld-path=$mold -o $t/exe $t/main.o -L$t -Wl,-merge-lfoo
$t/exe | grep -q '^42$'
$CC -o $t/exe2 $t/main.o -L$t -Wl,-merge-lfoo
$t/exe2 | grep -q '^42$'
