#!/usr/bin/env bash
. $(dirname $0)/common.inc

# In the REL relocation format, an addend is read from the relocated
# place itself, and its value is signed. The Linux kernel's real-mode
# setup code, for example, presets fields relocated with R_386_8 and
# R_386_16 with negative values such as -1 and -512. Make sure we
# sign-extend such in-place addends instead of zero-extending them.

cat <<'EOF' | $CC -c -o $t/a.o -xassembler -
.data
.globl x8, x16, xpc8, xpc16
x8:    .byte  abs8 - 1
x16:   .short abs16 - 512
xpc8:  .byte  target - . - 1
xpc16: .short target - . - 512

.section .nonalloc
.byte  abs8 - 1
.short abs16 - 512
EOF

cat <<'EOF' | $CC -c -o $t/b.o -xassembler -
.globl abs8, abs16
abs8 = 100
abs16 = 1000

.data
.globl target
target:
.byte 0
EOF

cat <<'EOF' | $CC -c -o $t/c.o -xc -
#include <stdio.h>

extern char target;
extern unsigned char x8, xpc8;
extern unsigned short x16, xpc16;

int main() {
  printf("%d %d %d %d\n", x8, x16,
         (signed char)xpc8 == (char *)&target - (char *)&xpc8 - 1,
         (short)xpc16 == (char *)&target - (char *)&xpc16 - 512);
}
EOF

$CC -B. -no-pie -o $t/exe $t/a.o $t/b.o $t/c.o
$QEMU $t/exe | grep '^99 488 1 1$'

$OBJDUMP -s -j .nonalloc $t/exe | grep -w 63e801
