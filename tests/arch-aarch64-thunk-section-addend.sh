#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Assemblers emit calls to static functions as relocations against a
# section symbol plus an offset. A range extension thunk jumps to the
# start of a symbol, so such a call is an error if it needs a thunk.

cat <<EOF | $CC -c -o $t/a.o -xassembler -
.text
f1:
  ret
f2:
  ret

.section .far,"ax"
.globl _start
_start:
  bl f1
  bl f2
EOF

$CC -B. -nostdlib -static -o $t/exe1 $t/a.o

not $CC -B. -nostdlib -static -o $t/exe2 $t/a.o \
  -Wl,--section-start=.text=0x10000000,--section-start=.far=0x20000000 >& $t/log
grep -F 'R_AARCH64_CALL26 against .text needs a range extension thunk' $t/log
