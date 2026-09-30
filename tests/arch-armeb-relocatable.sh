#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Relocatable output keeps big-endian code, so it must not be marked BE8.
# Compiler drivers don't pass --be8 with -r.

cat <<'EOF' | $CC -o $t/a.o -c -xassembler -
.syntax unified
.arch armv7-a
.arm
.globl _start
.type _start, %function
_start:
  mov r0, #0
  mov r7, #1  @ __NR_exit
  svc #0
EOF

./mold -m armelfb_linux_eabi -r -o $t/b.o $t/a.o
readelf -h $t/b.o | not grep BE8
$OBJDUMP -d $t/b.o | grep -E 'mov\s+r7, #1'

./mold -m armelfb_linux_eabi -r --be8 -o $t/c.o $t/a.o
readelf -h $t/c.o | not grep BE8
$OBJDUMP -d $t/c.o | grep -E 'mov\s+r7, #1'

./mold -o $t/exe $t/c.o
readelf -h $t/exe | grep BE8
$QEMU $t/exe
