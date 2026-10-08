#!/usr/bin/env bash
. $(dirname $0)/common.inc

# f1 and f2 are identical, and so are their FDEs except that f2's ends
# with four more DW_CFA_nops, as LLVM pads the last FDE in .eh_frame.
# ICF must merge them.
cat <<'EOF' | $CC -c -xassembler -o $t/a.o -
.section .text.f1,"ax",@progbits
.globl f1
f1:
  mov $5, %eax
  ret

.section .text.f2,"ax",@progbits
.globl f2
f2:
  mov $5, %eax
  ret

.section .eh_frame,"a",@progbits
cie: .long 16, 0; .byte 1; .asciz "zR"; .byte 1, 0x78, 16, 1, 0x1b, 0, 0, 0
.long 16, . - cie, f1 - ., 6, 0
.long 20, . - cie, f2 - ., 6, 0, 0
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
#include <stdio.h>

int f1(void);
int f2(void);

int main() {
  printf("%d %d\n", (long)f1 == (long)f2, f2());
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,--icf=all
$QEMU $t/exe | grep '^1 5$'
