#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Adjacent SHT_NOTE sections with the same flags must share one PT_NOTE
# segment. The merging loop once examined ctx.chunks with an index into a
# filtered, address-sorted copy of it. The two vectors differ when the ELF
# and program headers are not memory-mapped, which is the case if
# --section-order doesn't mention EHDR and PHDR, and each note then got
# its own PT_NOTE.

# Binutils 2.32 injects their own .note.gnu.property section interfering with the tests
test_cflags -Xassembler -mx86-used-note=no && CFLAGS="-Xassembler -mx86-used-note=no" || CFLAGS=""

cat <<EOF | $CC $CFLAGS -o $t/a.o -c -x assembler -
.text
.globl _start
_start:
  nop

.section .note.a, "a", @note
.p2align 2
.quad 42

.section .note.b, "a", @note
.p2align 2
.quad 42
EOF

./mold -static -o $t/exe $t/a.o \
  --section-order='.note.a .note.b TEXT DATA BSS RODATA'

readelf -W --segments $t/exe > $t/log
[ "$(grep -cw NOTE $t/log)" = 1 ]
grep -q '\.note\.a \.note\.b' $t/log
