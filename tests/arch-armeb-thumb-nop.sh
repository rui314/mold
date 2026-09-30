#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A call to an undefined weak symbol and a TLSDESC call relaxed to
# local-exec become NOP.W, a pair of halfwords like any other 32-bit Thumb
# instruction.

cat <<'EOF' | $CC -o $t/a.o -c -xassembler -
.syntax unified
.arch armv7-a
.thumb
.globl _start
.type _start, %function
.thumb_func
_start:
  bl foo
  ldr r0, 1f
2:
  bl bar(tlscall)
  movs r0, #0
  movs r7, #1  @ __NR_exit
  svc #0
.p2align 2
1:
  .word bar(tlsdesc) + (. - 2b)

.weak foo

.section .tdata, "awT", %progbits
bar:
  .word 1
EOF

./mold -o $t/exe $t/a.o
$QEMU $t/exe
