#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A 1- or 2-byte data relocation may be at the very end of its section.
# Make sure we don't read a 4-byte instruction word at its location.

cat <<'EOF' | $CC -c -o $t/a.o -xassembler -
.section .data.x16,"aw"
.globl x16
x16: .half abs16

.section .data.xpc16,"aw"
.globl xpc16
xpc16: .half target - .

.section .data.x8,"aw"
.globl x8
x8: .byte abs8

.section .data.xpc8,"aw"
.globl xpc8
xpc8: .byte target - .

.section .data.target,"aw"
.globl target
target: .byte 0

.globl abs8, abs16
abs8 = 100
abs16 = 1000
EOF

cat <<'EOF' | $CC -c -o $t/b.o -xc -
#include <stdio.h>

extern char target;
extern unsigned char x8, xpc8;
extern unsigned short x16, xpc16;

int main() {
  printf("%d %d %d %d\n", x8, x16,
         (signed char)xpc8 == &target - (char *)&xpc8,
         (short)xpc16 == &target - (char *)&xpc16);
}
EOF

$CC -B. -no-pie -o $t/exe $t/a.o $t/b.o
$QEMU $t/exe | grep '^100 1000 1 1$'
