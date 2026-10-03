#!/bin/bash
source "$(dirname "$0")"/common.inc

[ "$ARCH" = arm64 ] || skip

# 116 MiB of code between main and the functions it calls, one in a
# second code section, with the stubs after them all: far more than a
# megabyte, but every branch - forward, back, into the other section
# or to a stub - is within the +-128 MiB reach of a bl, so the linker
# makes no branch islands.
cat <<'EOF' | $CC -o $t/pad.o -c -xassembler -
.subsections_via_symbols
_pad:
  .space 0x7400000
EOF

cat <<'EOF' | $CC -o $t/far.o -c -xassembler -
.subsections_via_symbols
.globl _far
.p2align 2
_far:
  stp x29, x30, [sp, #-16]!
  bl _near
  add w0, w0, #1
  ldp x29, x30, [sp], #16
  ret

.section __TEXT,__text2,regular,pure_instructions
.globl _far2
.p2align 2
_far2:
  mov w0, #3
  ret
EOF

cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
int far();
int far2();
int near() { return 41; }
int main() { printf("%d %d\n", far(), far2()); }
EOF

$CC --ld-path=$mold -o $t/exe $t/main.o $t/pad.o $t/far.o
$t/exe | grep -q '^42 3$'

nm $t/exe > $t/syms
not grep -q island $t/syms

# -no_branch_islands makes none at all, even for a branch out of reach,
# which is then an error.
cat <<'EOF' | $CC -o $t/pad2.o -c -xassembler -
.subsections_via_symbols
_pad2:
  .space 0x8400000
EOF

not $CC --ld-path=$mold -o $t/exe2 $t/main.o $t/pad2.o $t/far.o -Wl,-no_branch_islands 2> $t/log
grep -Eq "$t/main.o: _main\+0x[0-9a-f]+: B/BL out of range \(displacement=[0-9]+, max is \+/-128MB\), from 0x[0-9A-F]+ to 0x[0-9A-F]+ \('_far'\)" $t/log
rm -f $t/pad.o $t/pad2.o $t/exe
