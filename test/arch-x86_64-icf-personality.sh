#!/usr/bin/env bash
. $(dirname $0)/common.inc

# ICF must not merge functions whose FDEs point to different CIEs.
# fn1 and fn2 below are identical except for the personality routine
# recorded in their CIEs, so only fn1 and fn3 are mergeable.

cat <<EOF | $CC -c -o $t/a.o -xassembler -
.globl pers1, pers2
pers1:
  ret
pers2:
  ret

.section .text.fn1, "ax", @progbits
.globl fn1
fn1:
  .cfi_startproc
  .cfi_personality 0x1b, pers1
  ret
  .cfi_endproc

.section .text.fn2, "ax", @progbits
.globl fn2
fn2:
  .cfi_startproc
  .cfi_personality 0x1b, pers2
  ret
  .cfi_endproc

.section .text.fn3, "ax", @progbits
.globl fn3
fn3:
  .cfi_startproc
  .cfi_personality 0x1b, pers1
  ret
  .cfi_endproc

# The last FDE in .eh_frame is padded to align the section's end, so
# it cannot compare equal to the other FDEs. Add a function that isn't
# part of the test to absorb the padding.
.section .text.fn4, "ax", @progbits
.globl fn4
fn4:
  .cfi_startproc
  .cfi_personality 0x1b, pers2
  nop
  ret
  .cfi_endproc
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
#include <stdio.h>

extern char fn1, fn2, fn3;

int main() {
  printf("%d %d\n", (long)&fn1 == (long)&fn2, (long)&fn1 == (long)&fn3);
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,-icf=all
$QEMU $t/exe | grep '0 1'
