#!/usr/bin/env bash
. $(dirname $0)/common.inc

[ $MACHINE = x86_64 ] || skip

cat <<'EOF' | $CC -o $t/a.o -c -x assembler -
.section .text.local,"axG",@progbits,localsig,comdat
.local localsig
localsig:
  .byte 0x11

.section .text.section,"axG",@progbits,.text.section,comdat
  .byte 0x22

.section .text.version,"axG",@progbits,"versig@V1",comdat
.globl "versig@V1"
"versig@V1":
  .byte 0x33
EOF

cat <<'EOF' | $CC -o $t/b.o -c -x assembler -
.section .text.local,"axG",@progbits,localsig,comdat
.local localsig
localsig:
  .byte 0x44

.section .text.section,"axG",@progbits,.text.section,comdat
  .byte 0x55

.section .text.version,"axG",@progbits,"versig@V1",comdat
.globl "versig@V1"
"versig@V1":
  .byte 0x66
EOF

cat <<'EOF' | $CC -o $t/c.o -c -x assembler -
.text
.globl _start
_start:
  ret
EOF

$CC -B. -nostdlib -Wl,-e,_start -o $t/exe $t/c.o $t/a.o $t/b.o
$OBJDUMP -s -j .text $t/exe | grep '112233'
