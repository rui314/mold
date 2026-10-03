#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Assemblers emit calls to static functions as relocations against a
# section symbol plus an offset. A range extension thunk jumps to the
# start of a symbol, so such a call must get a thunk entry that jumps to
# the offset rather than to the start of the section.

cat <<EOF | $CC -c -o $t/a.o -xassembler -
.text
f1:
  mov x0, #3
  ret
f2:
  add x0, x0, #4
  ret

.section .far,"ax"
.globl _start
_start:
  bl f1
  bl f2
  cmp x0, #7
  cset x0, ne
  mov x8, #93
  svc #0
EOF

$CC -B. -nostdlib -static -o $t/exe $t/a.o \
  -Wl,--section-start=.text=0x10000000,--section-start=.far=0x20000000
$QEMU $t/exe
